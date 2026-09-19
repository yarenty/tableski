//! The embedding contract (#9): a crate that depends on `tableski-core` alone can register a
//! file and run `query_sql` — no CLI, no HTTP.
use datafusion::prelude::SessionContext;
use std::path::PathBuf;
use std::sync::Arc;
use tableski_core::{AppState, IngestOptions, call_tool, register_path, tools_list};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

#[tokio::test]
async fn register_and_query_without_transport() {
    let ctx = SessionContext::new();
    let tables = register_path(&ctx, &fixture("sample.csv"), &IngestOptions::default())
        .await
        .expect("register sample.csv");
    assert_eq!(tables.len(), 1);
    let table = tables[0].name.clone();
    let state = AppState::new(Arc::new(ctx), tables);

    let grid = call_tool(
        &state,
        "query_sql",
        serde_json::json!({ "sql": format!("SELECT count(*) AS n FROM {table}") }),
    )
    .await
    .expect("query_sql");
    assert!(grid.contains("| n"), "grid header missing: {grid}");

    let schema = call_tool(&state, "get_schema", serde_json::json!({}))
        .await
        .expect("get_schema");
    assert!(
        schema.contains(&format!("\"table\": \"{table}\"")),
        "{schema}"
    );

    let tools = tools_list();
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "list_tables",
            "query_sql",
            "get_schema",
            "export_result",
            "column_statistics"
        ]
    );

    let err = call_tool(&state, "no_such_tool", serde_json::json!({}))
        .await
        .unwrap_err();
    assert!(err.contains("unknown tool"));
}
