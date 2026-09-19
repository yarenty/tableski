//! Unified file registration: one model for every supported format.
//!
//! The extension picks the reader; the table name is the slugified file stem (workbooks:
//! one table per sheet, named after the sheet). Supported: `csv`, `parquet`,
//! `json` / `ndjson` / `jsonl` (newline-delimited JSON — DataFusion's built-in JSON
//! format), and `xlsx` / `xls` / `ods` workbooks.

use crate::TableEntry;
use crate::excel::{IngestOptions, register_workbook};
use datafusion::execution::options::JsonReadOptions;
use datafusion::prelude::*;
use std::path::Path;

/// Slugified file stem, used as the table name for single-table formats.
fn stem_slug(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("data")
        .to_lowercase();
    let mut slug: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    while slug.contains("__") {
        slug = slug.replace("__", "_");
    }
    let slug = slug.trim_matches('_');
    if slug.is_empty() {
        "data".to_string()
    } else if slug.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("t_{slug}")
    } else {
        slug.to_string()
    }
}

/// Register `path` by extension. Returns one [`TableEntry`] per registered table
/// (workbooks may produce several — one per sheet).
pub async fn register_path(
    ctx: &SessionContext,
    path: &Path,
    opts: &IngestOptions,
) -> Result<Vec<TableEntry>, String> {
    if !path.exists() {
        return Err(format!("file not found: {}", path.display()));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let p = path
        .to_str()
        .ok_or_else(|| format!("path must be valid UTF-8: {}", path.display()))?;
    // Dedupe against already-registered tables (e.g. sample.csv + sample.parquet).
    let base = stem_slug(path);
    let mut name = base.clone();
    let mut n = 1;
    while ctx.table_exist(&name).map_err(|e| e.to_string())? {
        n += 1;
        name = format!("{base}_{n}");
    }

    match ext.as_str() {
        "csv" => {
            ctx.register_csv(&name, p, CsvReadOptions::new())
                .await
                .map_err(|e| format!("csv {}: {e}", path.display()))?;
            Ok(vec![TableEntry::csv(&name, p)])
        }
        "parquet" => {
            ctx.register_parquet(&name, p, ParquetReadOptions::default())
                .await
                .map_err(|e| format!("parquet {}: {e}", path.display()))?;
            Ok(vec![TableEntry::csv(&name, p)])
        }
        "json" | "ndjson" | "jsonl" => {
            let dotted = format!(".{ext}");
            let json_opts = JsonReadOptions {
                file_extension: &dotted,
                ..Default::default()
            };
            ctx.register_json(&name, p, json_opts).await.map_err(|e| {
                format!(
                    "ndjson {}: {e} (note: JSON must be newline-delimited — one object per line)",
                    path.display()
                )
            })?;
            Ok(vec![TableEntry::csv(&name, p)])
        }
        "xlsx" | "xls" | "ods" => {
            let infos = register_workbook(ctx, path, opts)?;
            Ok(infos
                .iter()
                .map(|i| TableEntry::sheet(i, path.display().to_string()))
                .collect())
        }
        other => Err(format!(
            "unsupported extension `.{other}` for {}: supported are .csv .parquet .json/.ndjson/.jsonl .xlsx/.xls/.ods",
            path.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_slugify_like_sheets() {
        assert_eq!(
            stem_slug(Path::new("/tmp/My Sales 2024.csv")),
            "my_sales_2024"
        );
        assert_eq!(stem_slug(Path::new("2024.parquet")), "t_2024");
        assert_eq!(stem_slug(Path::new("***.csv")), "data");
    }
}
