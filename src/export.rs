//! Result export: run a query and write the result set as CSV or XLSX — only into the
//! operator-configured export directory (no caller-controlled paths outside it).

use datafusion::arrow::array::{Array, BooleanArray, Float64Array, Int64Array};
use datafusion::arrow::datatypes::DataType;
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::arrow::util::display::array_value_to_string;
use datafusion::prelude::SessionContext;
use rust_xlsxwriter::Workbook;
use std::path::{Component, Path, PathBuf};

/// What a successful export produced.
#[derive(Debug, serde::Serialize)]
pub struct ExportSummary {
    pub path: String,
    pub format: String,
    pub rows: usize,
    pub columns: usize,
}

/// Validate the caller-supplied file name: relative, no parent components, `.csv`/`.xlsx`.
fn safe_target(export_dir: &Path, file: &str) -> Result<(PathBuf, String), String> {
    let rel = Path::new(file);
    if rel.is_absolute() {
        return Err("export file must be a relative name, not an absolute path".to_string());
    }
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err("export file must not contain `..` or other path components".to_string());
    }
    let format = match rel.extension().and_then(|e| e.to_str()) {
        Some("csv") => "csv".to_string(),
        Some("xlsx") => "xlsx".to_string(),
        _ => return Err("export file must end in .csv or .xlsx".to_string()),
    };
    Ok((export_dir.join(rel), format))
}

/// Run `sql` and write the full result set to `file` under `export_dir`.
pub async fn export_query(
    ctx: &SessionContext,
    sql: &str,
    export_dir: &Path,
    file: &str,
) -> Result<ExportSummary, String> {
    let (target, format) = safe_target(export_dir, file)?;
    let df = ctx.sql(sql).await.map_err(|e| e.to_string())?;
    let batches = df.collect().await.map_err(|e| e.to_string())?;
    let rows: usize = batches.iter().map(RecordBatch::num_rows).sum();
    let columns = batches.first().map_or(0, RecordBatch::num_columns);

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    match format.as_str() {
        "csv" => write_csv(&target, &batches)?,
        _ => write_xlsx(&target, &batches)?,
    }
    Ok(ExportSummary {
        path: target.display().to_string(),
        format,
        rows,
        columns,
    })
}

fn write_csv(target: &Path, batches: &[RecordBatch]) -> Result<(), String> {
    let file = std::fs::File::create(target).map_err(|e| e.to_string())?;
    let mut writer = datafusion::arrow::csv::WriterBuilder::new()
        .with_header(true)
        .build(file);
    for batch in batches {
        writer.write(batch).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_xlsx(target: &Path, batches: &[RecordBatch]) -> Result<(), String> {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    let mut row: u32 = 0;

    if let Some(first) = batches.first() {
        for (c, field) in first.schema().fields().iter().enumerate() {
            ws.write_string(0, c as u16, field.name())
                .map_err(|e| e.to_string())?;
        }
        row = 1;
    }
    for batch in batches {
        for r in 0..batch.num_rows() {
            for c in 0..batch.num_columns() {
                let col = batch.column(c);
                let cell = (row, c as u16);
                if col.is_null(r) {
                    continue;
                }
                match col.data_type() {
                    DataType::Int64 => {
                        let v = col.as_any().downcast_ref::<Int64Array>().unwrap().value(r);
                        ws.write_number(cell.0, cell.1, v as f64)
                            .map_err(|e| e.to_string())?;
                    }
                    DataType::Float64 => {
                        let v = col
                            .as_any()
                            .downcast_ref::<Float64Array>()
                            .unwrap()
                            .value(r);
                        ws.write_number(cell.0, cell.1, v)
                            .map_err(|e| e.to_string())?;
                    }
                    DataType::Boolean => {
                        let v = col
                            .as_any()
                            .downcast_ref::<BooleanArray>()
                            .unwrap()
                            .value(r);
                        ws.write_boolean(cell.0, cell.1, v)
                            .map_err(|e| e.to_string())?;
                    }
                    _ => {
                        let v = array_value_to_string(col, r).map_err(|e| e.to_string())?;
                        ws.write_string(cell.0, cell.1, &v)
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            row += 1;
        }
    }
    wb.save(target).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_target_rejects_escapes() {
        let dir = Path::new("/tmp/exports");
        assert!(safe_target(dir, "../evil.csv").is_err());
        assert!(safe_target(dir, "/abs/evil.csv").is_err());
        assert!(safe_target(dir, "ok.txt").is_err());
        let (p, f) = safe_target(dir, "reports/q1.xlsx").unwrap();
        assert_eq!(f, "xlsx");
        assert!(p.ends_with("reports/q1.xlsx"));
    }
}
