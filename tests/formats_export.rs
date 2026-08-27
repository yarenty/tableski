//! Format breadth + export: the same tool surface serves CSV, Excel, Parquet, and NDJSON;
//! `export_result` writes csv/xlsx sandboxed to the export dir; xlsx round-trips.

use datafusion::prelude::*;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use tableski::{AppState, IngestOptions, app_router, register_path, register_workbook};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

async fn state_all_formats(export_dir: Option<PathBuf>) -> AppState {
    let ctx = SessionContext::new();
    let opts = IngestOptions::default();
    let mut tables = Vec::new();
    for f in [
        "sample.csv",
        "sample.xlsx",
        "sample.parquet",
        "sample.ndjson",
    ] {
        tables.extend(register_path(&ctx, &fixture(f), &opts).await.unwrap());
    }
    let mut state = AppState::new(Arc::new(ctx), tables);
    if let Some(dir) = export_dir {
        state = state.with_export_dir(dir);
    }
    state
}

async fn spawn(state: AppState) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://127.0.0.1:{}/",
        listener.local_addr().unwrap().port()
    );
    tokio::spawn(async move {
        axum::serve(listener, app_router(state)).await.unwrap();
    });
    url
}

async fn call(url: &str, name: &str, args: Value) -> Value {
    reqwest::Client::new()
        .post(url)
        .header("Accept", "application/json")
        .json(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": args }
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

fn text(v: &Value) -> &str {
    v["result"]["content"][0]["text"].as_str().expect("text")
}

#[tokio::test]
async fn same_sql_tool_serves_all_four_formats() {
    let url = spawn(state_all_formats(None).await).await;

    // Table inventory: csv stem `sample`, workbook sheets `people`/`orders`, and the
    // colliding parquet/ndjson stems deduplicated to `sample_2`/`sample_3`.
    let lt = call(&url, "list_tables", json!({})).await;
    let t = text(&lt);
    for expected in [
        "\"sample\"",
        "\"people\"",
        "\"orders\"",
        "\"sample_2\"",
        "\"sample_3\"",
    ] {
        assert!(t.contains(expected), "missing {expected} in {t}");
    }

    // One query per format through the SAME tool, all framed.
    for (table, needle) in [
        ("sample", "BEGIN_DATA_"), // csv
        ("people", "3"),           // xlsx sheet
        ("sample_2", "3"),         // parquet (3 rows of sample.csv)
        ("sample_3", "3"),         // ndjson (3 objects)
    ] {
        let q = call(
            &url,
            "query_sql",
            json!({ "sql": format!("SELECT COUNT(*) AS n FROM {table}") }),
        )
        .await;
        let out = text(&q);
        assert!(out.contains(needle), "{table}: {out}");
        assert!(out.contains("BEGIN_DATA_"), "{table} framed: {out}");
    }

    // And a cross-FORMAT join: ndjson (dept/salary) joined to the xlsx sheet by name.
    let j = call(
        &url,
        "query_sql",
        json!({ "sql": "SELECT p.name, s.dept FROM people p JOIN sample_3 s ON p.name = s.name ORDER BY p.name" }),
    )
    .await;
    let out = text(&j);
    assert!(
        out.contains("engineering") && out.contains("kernel"),
        "{out}"
    );
}

#[tokio::test]
async fn export_round_trip_xlsx_and_csv() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("exports");
    let url = spawn(state_all_formats(Some(dir.clone())).await).await;

    // Export a cross-sheet aggregation to xlsx.
    let exp = call(
        &url,
        "export_result",
        json!({
            "sql": "SELECT p.name, SUM(o.amount) AS total FROM people p JOIN orders o ON p.name = o.name GROUP BY p.name ORDER BY total DESC",
            "file": "totals.xlsx"
        }),
    )
    .await;
    let t = text(&exp);
    assert!(
        t.contains("BEGIN_DATA_") && t.contains("totals.xlsx") && t.contains("\"rows\": 2"),
        "{t}"
    );

    // Re-ingest the exported workbook: same data comes back.
    let ctx = SessionContext::new();
    register_workbook(&ctx, &dir.join("totals.xlsx"), &IngestOptions::default()).unwrap();
    let batches = ctx
        .sql("SELECT SUM(total) AS s, COUNT(*) AS n FROM sheet1")
        .await
        .unwrap()
        .collect()
        .await
        .unwrap();
    let pretty = datafusion::arrow::util::pretty::pretty_format_batches(&batches)
        .unwrap()
        .to_string();
    assert!(
        pretty.contains("250.49") && pretty.contains('2'),
        "round-trip totals: {pretty}"
    );

    // CSV export too.
    let exp = call(
        &url,
        "export_result",
        json!({ "sql": "SELECT name FROM people ORDER BY name", "file": "names.csv" }),
    )
    .await;
    assert!(text(&exp).contains("names.csv"));
    let csv = std::fs::read_to_string(dir.join("names.csv")).unwrap();
    assert_eq!(csv.lines().count(), 4, "header + 3 rows: {csv}");
    assert!(csv.starts_with("name\n"));
}

#[tokio::test]
async fn export_is_sandboxed_and_gated() {
    // Escape attempts are structured errors.
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("exports_gate");
    let url = spawn(state_all_formats(Some(dir)).await).await;
    for bad in ["../evil.csv", "/abs/evil.xlsx", "notes.txt"] {
        let r = call(
            &url,
            "export_result",
            json!({ "sql": "SELECT 1", "file": bad }),
        )
        .await;
        assert!(
            r["error"]["message"].as_str().is_some(),
            "{bad} must be rejected: {r}"
        );
    }

    // Without --export-dir the tool is disabled with a clear error.
    let url = spawn(state_all_formats(None).await).await;
    let r = call(
        &url,
        "export_result",
        json!({ "sql": "SELECT 1", "file": "x.csv" }),
    )
    .await;
    assert!(
        r["error"]["message"]
            .as_str()
            .unwrap()
            .contains("export-dir"),
        "{r}"
    );
}
