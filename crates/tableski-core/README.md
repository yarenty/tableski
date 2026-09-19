# tableski-core

The engine behind [tableski](https://github.com/yarenty/tableski): Excel (xlsx / xls / ods),
CSV, Parquet and NDJSON files become [Apache DataFusion](https://datafusion.apache.org/)
tables, and five MCP tools (`list_tables`, `query_sql`, `get_schema`, `export_result`,
`column_statistics`) query them with every result framed as data.

No transport in here. `AppState` implements `emperor_mcp::McpHandler`, so any emperor-mcp
transport can serve it; `call_tool` runs a tool directly when you want no MCP at all.

```rust,no_run
use datafusion::prelude::SessionContext;
use std::sync::Arc;
use tableski_core::{AppState, IngestOptions, call_tool, register_path};

# async fn run() -> Result<(), String> {
let ctx = SessionContext::new();
let tables = register_path(&ctx, "orders.xlsx".as_ref(), &IngestOptions::default())
    .await
    .map_err(|e| e.to_string())?;           // one table per sheet
let state = AppState::new(Arc::new(ctx), tables).with_export_dir("exports");
let grid = call_tool(&state, "query_sql", serde_json::json!({
    "sql": "SELECT region, sum(amount) FROM orders GROUP BY region"
})).await?;
println!("{grid}");
# Ok(()) }
```

Features: `clap` derives `clap::ValueEnum` on `HeaderMode` for CLIs that expose `--headers`.

The CLI and HTTP server live in the `tableski` crate. Licence: MIT OR Apache-2.0.
