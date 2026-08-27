//! **Stateless** Streamable HTTP (JSON + SSE) MCP server: DataFusion tools over registered
//! data files — CSV and Excel workbooks (one table per sheet).
//!
//! Transport (stdio / stateless HTTP, SSE framing, parse errors) lives in
//! [`emperor_mcp::transport`]; this crate only implements the DataFusion tool dispatch via
//! [`McpHandler`]. No `Mcp-Session-Id` is issued or required — every POST is independent.
//! Every tool result's text is framed as data ([`emperor_mcp::frame`], Emperor Profile E8).

use axum::Router;
use datafusion::arrow::datatypes::Schema;
use datafusion::arrow::util::pretty::pretty_format_batches;
use datafusion::prelude::*;
use emperor_mcp::{FrameKind, McpHandler, frame, http_router};
use serde_json::{Value, json};
use std::sync::Arc;

pub mod excel;
pub mod export;
pub mod register;
pub use excel::{HeaderMode, IngestOptions, SheetInfo, register_workbook};
pub use register::register_path;

/// `Accept` value clients should send (re-exported from the shared transport).
pub use emperor_mcp::ACCEPT_STREAMABLE;

/// MCP protocol version reported on `initialize`.
pub const PROTOCOL_VERSION: &str = "2025-03-26";

/// One registered table and where it came from (CSV path or workbook sheet).
#[derive(Debug, Clone, serde::Serialize)]
pub struct TableEntry {
    pub name: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<usize>,
}

impl TableEntry {
    /// A CSV-backed table.
    pub fn csv(name: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: path.into(),
            sheet: None,
            rows: None,
            columns: None,
        }
    }

    /// A workbook-sheet-backed table.
    pub fn sheet(info: &SheetInfo, workbook: impl Into<String>) -> Self {
        Self {
            name: info.table.clone(),
            source: workbook.into(),
            sheet: Some(info.sheet.clone()),
            rows: Some(info.rows),
            columns: Some(info.columns),
        }
    }
}

/// DataFusion-backed MCP handler: registered tables plus the session context to query them.
#[derive(Clone)]
pub struct AppState {
    pub ctx: Arc<SessionContext>,
    pub tables: Vec<TableEntry>,
    /// Directory result exports may write into; exports are disabled when `None`.
    pub export_dir: Option<std::path::PathBuf>,
}

impl AppState {
    pub fn new(ctx: Arc<SessionContext>, tables: Vec<TableEntry>) -> Self {
        Self {
            ctx,
            tables,
            export_dir: None,
        }
    }

    /// Enable the `export_result` tool, sandboxed to `dir`.
    pub fn with_export_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.export_dir = Some(dir.into());
        self
    }

    /// Resolve the target table for schema/statistics tools: the explicit argument if
    /// given (must be registered), else the only/first registered table.
    fn resolve_table(&self, args: &Value) -> Result<String, String> {
        match args.get("table").and_then(Value::as_str) {
            Some(t) => {
                if self.tables.iter().any(|e| e.name == t) {
                    Ok(t.to_string())
                } else {
                    Err(format!(
                        "unknown table `{t}` — registered: {}",
                        self.table_names().join(", ")
                    ))
                }
            }
            None => self
                .tables
                .first()
                .map(|e| e.name.clone())
                .ok_or_else(|| "no tables registered".to_string()),
        }
    }

    fn table_names(&self) -> Vec<String> {
        self.tables.iter().map(|e| e.name.clone()).collect()
    }
}

impl McpHandler for AppState {
    fn handle(&self, request: Value) -> impl std::future::Future<Output = Option<Value>> + Send {
        let state = self.clone();
        async move { dispatch_mcp(&state, request).await }
    }
}

/// Build the stateless Streamable HTTP router over the registered tables.
pub fn app_router(state: AppState) -> Router {
    http_router(Arc::new(state))
}

/// A successful tool result whose text content is framed as computed data (Profile E8).
fn framed_text(text: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": frame(FrameKind::Computed, text) }]
    })
}

async fn dispatch_mcp(state: &AppState, body: Value) -> Option<Value> {
    // JSON-RPC notifications (no `id`, e.g. notifications/initialized) get no reply.
    let id = body.get("id")?.clone();
    let method = body.get("method").and_then(|m| m.as_str()).unwrap_or("");

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "serverInfo": {
                "name": "tableski",
                "version": env!("CARGO_PKG_VERSION")
            },
            "capabilities": { "tools": {} }
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools_list_json()),
        "tools/call" => run_tool_call(state, &body).await,
        other => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("method not found: {other}") }
            }));
        }
    };

    Some(match result {
        Ok(v) => json!({ "jsonrpc": "2.0", "id": id, "result": v }),
        Err(e) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32000, "message": e }
        }),
    })
}

fn tools_list_json() -> Value {
    json!({
        "tools": [
            {
                "name": "list_tables",
                "description": "List every registered table: name, source file, source sheet (for workbooks), rows and columns.",
                "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
            },
            {
                "name": "query_sql",
                "description": "Run an arbitrary SQL query against the registered tables (joins across tables/sheets work) and return a pretty-printed result grid.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sql": { "type": "string", "description": "SQL statement; use list_tables for the available table names" }
                    },
                    "required": ["sql"]
                }
            },
            {
                "name": "get_schema",
                "description": "Return column names, Arrow data types, and nullability for a registered table (LIMIT 0 scan).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "table": { "type": "string", "description": "Table name (default: the first registered table)" }
                    },
                    "additionalProperties": false
                }
            },
            {
                "name": "export_result",
                "description": "Run a SQL query and write the full result set to a .csv or .xlsx file inside the server's configured export directory. Returns the written path and dimensions.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sql": { "type": "string", "description": "SQL statement producing the rows to export" },
                        "file": { "type": "string", "description": "Relative file name ending in .csv or .xlsx (subdirectories allowed, no ..)" }
                    },
                    "required": ["sql", "file"]
                }
            },
            {
                "name": "column_statistics",
                "description": "High-level statistics for every column of a registered table: count, null_count, mean, std, min, max (DataFusion describe).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "table": { "type": "string", "description": "Table name (default: the first registered table)" }
                    },
                    "additionalProperties": false
                }
            }
        ]
    })
}

fn schema_to_json(table: &str, schema: &Schema) -> Value {
    let columns: Vec<Value> = schema
        .fields()
        .iter()
        .map(|f| {
            json!({
                "name": f.name(),
                "data_type": format!("{}", f.data_type()),
                "nullable": f.is_nullable(),
            })
        })
        .collect();
    json!({
        "table": table,
        "columns": columns
    })
}

async fn run_tool_call(state: &AppState, body: &Value) -> Result<Value, String> {
    let params = body.get("params").cloned().unwrap_or_else(|| json!({}));
    let name = params["name"].as_str().unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match name {
        "list_tables" => {
            let j = json!({ "tables": state.tables });
            Ok(framed_text(
                &serde_json::to_string_pretty(&j).unwrap_or_else(|_| j.to_string()),
            ))
        }
        "query_sql" => {
            let sql = args["sql"]
                .as_str()
                .ok_or_else(|| "missing arguments.sql".to_string())?;
            let df = state.ctx.sql(sql).await.map_err(|e| e.to_string())?;
            let batches = df.collect().await.map_err(|e| e.to_string())?;
            let text = pretty_format_batches(&batches)
                .map_err(|e| e.to_string())?
                .to_string();
            Ok(framed_text(&text))
        }
        "get_schema" => {
            let table = state.resolve_table(&args)?;
            let sql = format!("SELECT * FROM {table} LIMIT 0");
            let df = state.ctx.sql(&sql).await.map_err(|e| e.to_string())?;
            let j = schema_to_json(&table, df.schema().as_arrow());
            Ok(framed_text(
                &serde_json::to_string_pretty(&j).unwrap_or_else(|_| j.to_string()),
            ))
        }
        "export_result" => {
            let dir = state.export_dir.as_deref().ok_or_else(|| {
                "exports are disabled — start the server with --export-dir".to_string()
            })?;
            let sql = args["sql"]
                .as_str()
                .ok_or_else(|| "missing arguments.sql".to_string())?;
            let file = args["file"]
                .as_str()
                .ok_or_else(|| "missing arguments.file".to_string())?;
            let summary = export::export_query(&state.ctx, sql, dir, file).await?;
            let j = serde_json::to_value(&summary).map_err(|e| e.to_string())?;
            Ok(framed_text(
                &serde_json::to_string_pretty(&j).unwrap_or_else(|_| j.to_string()),
            ))
        }
        "column_statistics" => {
            let table = state.resolve_table(&args)?;
            let sql = format!("SELECT * FROM {table}");
            let df = state.ctx.sql(&sql).await.map_err(|e| e.to_string())?;
            let desc = df.describe().await.map_err(|e| e.to_string())?;
            let batches = desc.collect().await.map_err(|e| e.to_string())?;
            let text = pretty_format_batches(&batches)
                .map_err(|e| e.to_string())?
                .to_string();
            Ok(framed_text(&text))
        }
        _ => Err(format!("unknown tool: {name}")),
    }
}
