//! Excel golden tests: a real xlsx (written by rust_xlsxwriter, read by calamine) goes
//! through registration, typed schema inference, cross-sheet SQL joins, header-mode
//! overrides, and the framed HTTP tool path.

use datafusion::prelude::*;
use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use tableski::{AppState, HeaderMode, IngestOptions, TableEntry, app_router, register_workbook};

/// Write the golden workbook: `People` (typed columns incl. a date), `2024 Orders`
/// (joinable), `raw` (no headers).
fn write_golden(path: &PathBuf) {
    let mut wb = Workbook::new();
    let date_fmt = Format::new().set_num_format("yyyy-mm-dd");

    let people = wb.add_worksheet().set_name("People").unwrap();
    for (c, h) in ["name", "age", "active", "joined"].iter().enumerate() {
        people.write_string(0, c as u16, *h).unwrap();
    }
    let rows = [
        ("ada", 36.0, true, "2024-01-15"),
        ("grace", 45.0, false, "2023-11-02"),
        ("linus", 54.0, true, "2024-06-30"),
    ];
    for (r, (name, age, active, joined)) in rows.iter().enumerate() {
        let r = (r + 1) as u32;
        people.write_string(r, 0, *name).unwrap();
        people.write_number(r, 1, *age).unwrap();
        people.write_boolean(r, 2, *active).unwrap();
        let dt = ExcelDateTime::parse_from_str(joined).unwrap();
        people
            .write_datetime_with_format(r, 3, &dt, &date_fmt)
            .unwrap();
    }

    let orders = wb.add_worksheet().set_name("2024 Orders").unwrap();
    for (c, h) in ["name", "amount"].iter().enumerate() {
        orders.write_string(0, c as u16, *h).unwrap();
    }
    for (r, (name, amount)) in [("ada", 120.5), ("ada", 30.0), ("linus", 99.99)]
        .iter()
        .enumerate()
    {
        let r = (r + 1) as u32;
        orders.write_string(r, 0, *name).unwrap();
        orders.write_number(r, 1, *amount).unwrap();
    }

    // No header row: first row is data (numbers), Auto must fall back to col_N names.
    let raw = wb.add_worksheet().set_name("raw").unwrap();
    for (r, v) in [1.0, 2.0, 3.0].iter().enumerate() {
        raw.write_number(r as u32, 0, *v).unwrap();
        raw.write_number(r as u32, 1, v * 10.0).unwrap();
    }

    wb.save(path).unwrap();
}

fn golden_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    p.push(name);
    p
}

#[tokio::test]
async fn workbook_registers_typed_tables_and_joins_across_sheets() {
    let path = golden_path("golden.xlsx");
    write_golden(&path);

    let ctx = SessionContext::new();
    let infos = register_workbook(&ctx, &path, &IngestOptions::default()).expect("register");

    // Three sheets, slugified names, correct dimensions.
    let names: Vec<_> = infos.iter().map(|i| i.table.as_str()).collect();
    assert_eq!(names, vec!["people", "t_2024_orders", "raw"]);
    assert_eq!(infos[0].rows, 3);
    assert_eq!(infos[0].columns, 4);

    // Typed schema: Utf8 / Int64 (integral floats promoted) / Boolean / Timestamp.
    let df = ctx.sql("SELECT * FROM people LIMIT 0").await.unwrap();
    let schema = df.schema().as_arrow().clone();
    let types: Vec<String> = schema
        .fields()
        .iter()
        .map(|f| format!("{}:{}", f.name(), f.data_type()))
        .collect();
    assert_eq!(
        types,
        vec![
            "name:Utf8",
            "age:Int64",
            "active:Boolean",
            "joined:Timestamp(ms)"
        ]
    );

    // Cross-sheet join with aggregation.
    let batches = ctx
        .sql(
            "SELECT p.name, SUM(o.amount) AS total \
             FROM people p JOIN t_2024_orders o ON p.name = o.name \
             GROUP BY p.name ORDER BY total DESC",
        )
        .await
        .unwrap()
        .collect()
        .await
        .unwrap();
    let pretty = datafusion::arrow::util::pretty::pretty_format_batches(&batches)
        .unwrap()
        .to_string();
    assert!(
        pretty.contains("ada") && pretty.contains("150.5"),
        "{pretty}"
    );
    assert!(pretty.contains("linus") && pretty.contains("99.99"));

    // Headerless sheet got synthetic names; date column queries like a timestamp.
    let df = ctx.sql("SELECT col_1, col_2 FROM raw").await.unwrap();
    assert_eq!(df.collect().await.unwrap()[0].num_rows(), 3);
    let one = ctx
        .sql("SELECT name FROM people WHERE joined >= '2024-01-01' ORDER BY name")
        .await
        .unwrap()
        .collect()
        .await
        .unwrap();
    let pretty = datafusion::arrow::util::pretty::pretty_format_batches(&one)
        .unwrap()
        .to_string();
    assert!(pretty.contains("ada") && pretty.contains("linus") && !pretty.contains("grace"));
}

#[tokio::test]
async fn header_mode_override_forces_first_row_as_data() {
    let path = golden_path("golden_none.xlsx");
    write_golden(&path);

    let ctx = SessionContext::new();
    let infos = register_workbook(
        &ctx,
        &path,
        &IngestOptions {
            headers: HeaderMode::None,
            ..Default::default()
        },
    )
    .expect("register");
    // With None, People keeps its header row as a DATA row (4 rows) and col_N names.
    let people = infos.iter().find(|i| i.table == "people").unwrap();
    assert_eq!(people.rows, 4);
    let df = ctx.sql("SELECT col_1 FROM people").await.unwrap();
    assert_eq!(df.collect().await.unwrap()[0].num_rows(), 4);
}

#[tokio::test]
async fn http_tools_serve_workbook_with_framed_output() {
    let path = golden_path("golden_http.xlsx");
    write_golden(&path);

    let ctx = SessionContext::new();
    let infos = register_workbook(&ctx, &path, &IngestOptions::default()).expect("register");
    let tables: Vec<TableEntry> = infos
        .iter()
        .map(|i| TableEntry::sheet(i, path.display().to_string()))
        .collect();
    let state = AppState::new(Arc::new(ctx), tables);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://127.0.0.1:{}/",
        listener.local_addr().unwrap().port()
    );
    tokio::spawn(async move {
        axum::serve(listener, app_router(state)).await.unwrap();
    });

    let c = reqwest::Client::new();
    let call = |name: &str, args: Value| {
        let c = c.clone();
        let url = url.clone();
        let name = name.to_string();
        async move {
            c.post(&url)
                .header("Accept", "application/json")
                .json(&json!({
                    "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                    "params": { "name": name, "arguments": args }
                }))
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        }
    };

    // list_tables inventories the sheets, framed.
    let lt = call("list_tables", json!({})).await;
    let text = lt["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("BEGIN_DATA_") && text.contains("END_DATA_"));
    assert!(text.contains("people") && text.contains("t_2024_orders") && text.contains("raw"));
    assert!(text.contains("\"sheet\": \"2024 Orders\""));

    // get_schema honours the table argument; unknown tables error cleanly.
    let gs = call("get_schema", json!({ "table": "t_2024_orders" })).await;
    let text = gs["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("amount") && text.contains("BEGIN_DATA_"));
    let bad = call("get_schema", json!({ "table": "nope" })).await;
    assert!(
        bad["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown table")
    );

    // Cross-sheet SQL over HTTP, framed.
    let q = call(
        "query_sql",
        json!({ "sql": "SELECT p.name FROM people p JOIN t_2024_orders o ON p.name = o.name GROUP BY p.name ORDER BY p.name" }),
    )
    .await;
    let text = q["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("ada") && text.contains("linus") && text.contains("BEGIN_DATA_"));
}
