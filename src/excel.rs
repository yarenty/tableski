//! Excel ingestion: calamine → Arrow `RecordBatch` → registered DataFusion tables.
//!
//! One workbook, one table per sheet. Sheet names are slugified into table names
//! (lowercase, `[a-z0-9_]`, digit-safe, deduplicated). Header handling per [`HeaderMode`];
//! column types are inferred over the data rows (Int64 / Float64 / Boolean /
//! Timestamp(ms) / Utf8, with empty cells as NULL).

use calamine::{Data, Range, Reader, open_workbook_auto};
use chrono::NaiveDateTime;
use datafusion::arrow::array::{
    ArrayRef, BooleanArray, Float64Array, Int64Array, StringArray, TimestampMillisecondArray,
};
use datafusion::arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::datasource::MemTable;
use datafusion::prelude::SessionContext;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

/// How the first row of each sheet is interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum HeaderMode {
    /// First row becomes column names when every cell is a distinct non-empty string;
    /// otherwise synthetic `col_1..col_N` names are used.
    #[default]
    Auto,
    /// Always treat the first row as column names.
    FirstRow,
    /// Never treat the first row as column names (`col_1..col_N`).
    None,
}

/// One registered sheet: table name, source sheet name, and data dimensions.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SheetInfo {
    pub table: String,
    pub sheet: String,
    pub rows: usize,
    pub columns: usize,
}

/// Ingestion options for [`register_workbook`].
#[derive(Debug, Clone)]
pub struct IngestOptions {
    /// First-row handling per sheet.
    pub headers: HeaderMode,
    /// Maximum data rows per sheet; exceeding it is a clear error, never a silent cut.
    pub max_rows: usize,
}

impl Default for IngestOptions {
    fn default() -> Self {
        Self {
            headers: HeaderMode::Auto,
            max_rows: 1_000_000,
        }
    }
}

/// Open `path` (xlsx / xls / ods), convert every non-empty sheet to an Arrow
/// `RecordBatch`, and register each as a DataFusion table. Returns the registered sheets.
pub fn register_workbook(
    ctx: &SessionContext,
    path: &Path,
    opts: &IngestOptions,
) -> Result<Vec<SheetInfo>, String> {
    let mut workbook = open_workbook_auto(path)
        .map_err(|e| format!("cannot open workbook {}: {e}", path.display()))?;
    let names = workbook.sheet_names().to_vec();
    if names.is_empty() {
        return Err(format!("workbook {} has no sheets", path.display()));
    }

    let mut used = HashSet::new();
    let mut infos = Vec::new();
    for sheet in names {
        let range = workbook
            .worksheet_range(&sheet)
            .map_err(|e| format!("cannot read sheet `{sheet}`: {e}"))?;
        if range.is_empty() {
            continue;
        }
        let table = unique_slug(&sheet, &mut used);
        let (batch, rows) =
            sheet_to_batch(&range, opts).map_err(|e| format!("sheet `{sheet}`: {e}"))?;
        let columns = batch.num_columns();
        let mem = MemTable::try_new(batch.schema(), vec![vec![batch]])
            .map_err(|e| format!("sheet `{sheet}`: {e}"))?;
        ctx.register_table(&table, Arc::new(mem))
            .map_err(|e| format!("register `{table}`: {e}"))?;
        infos.push(SheetInfo {
            table,
            sheet,
            rows,
            columns,
        });
    }
    if infos.is_empty() {
        return Err(format!("workbook {} has only empty sheets", path.display()));
    }
    Ok(infos)
}

/// Slugify a sheet name into a table identifier; ensure uniqueness within the workbook.
fn unique_slug(sheet: &str, used: &mut HashSet<String>) -> String {
    let mut slug: String = sheet
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    while slug.contains("__") {
        slug = slug.replace("__", "_");
    }
    let slug = slug.trim_matches('_');
    let mut base = if slug.is_empty() {
        "sheet".to_string()
    } else if slug.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("t_{slug}")
    } else {
        slug.to_string()
    };
    let mut n = 1;
    while !used.insert(base.clone()) {
        n += 1;
        base = format!("{base}_{n}");
    }
    base
}

/// Column type decided by scanning all data cells of one column.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ColType {
    Int,
    Float,
    Bool,
    DateTime,
    Text,
    /// Column had only empty cells; stored as nullable Utf8.
    Empty,
}

fn merge(a: ColType, cell: &Data) -> ColType {
    use ColType::*;
    let c = match cell {
        // Empty cells and formula-error cells (#DIV/0!, #N/A, …) don't influence the
        // column type; they materialize as NULL.
        Data::Empty | Data::Error(_) => return a,
        Data::Int(_) => Int,
        // xlsx numbers usually arrive as Float; all-integral Float columns are promoted
        // to Int64 in a second pass in `sheet_to_batch`.
        Data::Float(_) => Float,
        Data::Bool(_) => Bool,
        Data::DateTime(_) => DateTime,
        _ => Text,
    };
    match (a, c) {
        (Empty, x) => x,
        (x, y) if x == y => x,
        (Int, Float) | (Float, Int) => Float,
        _ => Text,
    }
}

fn header_names(range: &Range<Data>, width: usize, mode: HeaderMode) -> (bool, Vec<String>) {
    let first_row: Vec<&Data> = (0..width)
        .map(|c| range.get((0, c)).unwrap_or(&Data::Empty))
        .collect();
    let all_named = first_row.iter().all(|d| match d {
        Data::String(s) => !s.trim().is_empty(),
        _ => false,
    });
    let distinct = {
        let mut seen = HashSet::new();
        first_row.iter().all(|d| match d {
            Data::String(s) => seen.insert(s.trim().to_lowercase()),
            _ => false,
        })
    };
    let use_first = match mode {
        HeaderMode::FirstRow => true,
        HeaderMode::None => false,
        HeaderMode::Auto => all_named && distinct && range.height() > 1,
    };
    let names = if use_first {
        first_row
            .iter()
            .enumerate()
            .map(|(i, d)| match d {
                Data::String(s) if !s.trim().is_empty() => s.trim().to_string(),
                _ => format!("col_{}", i + 1),
            })
            .collect()
    } else {
        (0..width).map(|i| format!("col_{}", i + 1)).collect()
    };
    (use_first, names)
}

fn cell_to_millis(dt: &calamine::ExcelDateTime) -> Option<i64> {
    dt.as_datetime()
        .map(|ndt: NaiveDateTime| ndt.and_utc().timestamp_millis())
}

/// Effective (height, width): the used range minus trailing rows/columns that contain
/// only empty or formula-error cells (format-only cells widen calamine's range).
fn effective_dims(range: &Range<Data>) -> (usize, usize) {
    let is_blank = |d: &Data| matches!(d, Data::Empty | Data::Error(_));
    let mut height = 0;
    let mut width = 0;
    for r in 0..range.height() {
        for c in 0..range.width() {
            if !is_blank(range.get((r, c)).unwrap_or(&Data::Empty)) {
                height = height.max(r + 1);
                width = width.max(c + 1);
            }
        }
    }
    (height, width)
}

/// Convert one sheet range into a typed `RecordBatch`. Returns (batch, data_row_count).
fn sheet_to_batch(
    range: &Range<Data>,
    opts: &IngestOptions,
) -> Result<(RecordBatch, usize), String> {
    let (height, width) = effective_dims(range);
    if height == 0 || width == 0 {
        return Err("no data cells".to_string());
    }
    let (skip_first, names) = header_names(range, width, opts.headers);
    let start = usize::from(skip_first);
    let rows = height - start;
    if rows > opts.max_rows {
        return Err(format!(
            "{rows} data rows exceed the row cap of {} — raise --max-rows to ingest this sheet",
            opts.max_rows
        ));
    }

    let mut types = vec![ColType::Empty; width];
    for r in start..height {
        for (c, ty) in types.iter_mut().enumerate() {
            *ty = merge(*ty, range.get((r, c)).unwrap_or(&Data::Empty));
        }
    }
    // A column that is Int-typed only if EVERY numeric cell was an Int cell; calamine xlsx
    // numbers usually arrive as Float — promote all-integral Float columns to Int64.
    for (c, ty) in types.iter_mut().enumerate() {
        if *ty == ColType::Float {
            let all_integral =
                (start..height).all(|r| match range.get((r, c)).unwrap_or(&Data::Empty) {
                    Data::Float(f) => f.fract() == 0.0,
                    Data::Int(_) | Data::Empty => true,
                    _ => true,
                });
            if all_integral {
                *ty = ColType::Int;
            }
        }
    }

    let mut fields = Vec::with_capacity(width);
    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(width);
    for (c, name) in names.iter().enumerate() {
        let ty = types[c];
        let get = |r: usize| range.get((r, c)).unwrap_or(&Data::Empty);
        match ty {
            ColType::Int => {
                let vals: Vec<Option<i64>> = (start..height)
                    .map(|r| match get(r) {
                        Data::Int(i) => Some(*i),
                        Data::Float(f) => Some(*f as i64),
                        _ => None,
                    })
                    .collect();
                fields.push(Field::new(name, DataType::Int64, true));
                arrays.push(Arc::new(Int64Array::from(vals)));
            }
            ColType::Float => {
                let vals: Vec<Option<f64>> = (start..height)
                    .map(|r| match get(r) {
                        Data::Float(f) => Some(*f),
                        Data::Int(i) => Some(*i as f64),
                        _ => None,
                    })
                    .collect();
                fields.push(Field::new(name, DataType::Float64, true));
                arrays.push(Arc::new(Float64Array::from(vals)));
            }
            ColType::Bool => {
                let vals: Vec<Option<bool>> = (start..height)
                    .map(|r| match get(r) {
                        Data::Bool(b) => Some(*b),
                        _ => None,
                    })
                    .collect();
                fields.push(Field::new(name, DataType::Boolean, true));
                arrays.push(Arc::new(BooleanArray::from(vals)));
            }
            ColType::DateTime => {
                let vals: Vec<Option<i64>> = (start..height)
                    .map(|r| match get(r) {
                        Data::DateTime(dt) => cell_to_millis(dt),
                        _ => None,
                    })
                    .collect();
                fields.push(Field::new(
                    name,
                    DataType::Timestamp(TimeUnit::Millisecond, None),
                    true,
                ));
                arrays.push(Arc::new(TimestampMillisecondArray::from(vals)));
            }
            ColType::Text | ColType::Empty => {
                let vals: Vec<Option<String>> = (start..height)
                    .map(|r| match get(r) {
                        Data::Empty | Data::Error(_) => None,
                        Data::String(s) => Some(s.clone()),
                        Data::Float(f) => Some(f.to_string()),
                        Data::Int(i) => Some(i.to_string()),
                        Data::Bool(b) => Some(b.to_string()),
                        Data::DateTime(dt) => Some(
                            dt.as_datetime()
                                .map(|n| n.to_string())
                                .unwrap_or_else(|| dt.as_f64().to_string()),
                        ),
                        other => Some(format!("{other}")),
                    })
                    .collect();
                fields.push(Field::new(name, DataType::Utf8, true));
                arrays.push(Arc::new(StringArray::from(vals)));
            }
        }
    }

    let schema = Arc::new(Schema::new(fields));
    let batch = RecordBatch::try_new(schema, arrays).map_err(|e| e.to_string())?;
    Ok((batch, rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slug(s: &str) -> String {
        unique_slug(s, &mut HashSet::new())
    }

    #[test]
    fn slugs_are_sql_safe_and_unique() {
        assert_eq!(slug("Sheet 1"), "sheet_1");
        assert_eq!(slug("2024 Orders"), "t_2024_orders");
        assert_eq!(slug("Umsätze (EUR)"), "ums_tze_eur");
        assert_eq!(slug("___"), "sheet");
        let mut used = HashSet::new();
        assert_eq!(unique_slug("Data", &mut used), "data");
        assert_eq!(unique_slug("data", &mut used), "data_2");
    }
}
