//! The tableski engine: DataFusion tables over data files (CSV, Parquet, NDJSON, Excel
//! workbooks with one table per sheet) and the five MCP tools that query them.
//!
//! This crate has no transport. [`AppState`] implements [`McpHandler`], so any
//! `emperor-mcp` transport can serve it (the `tableski` CLI does stateless Streamable HTTP);
//! [`call_tool`] runs a tool directly for embedders that want no MCP at all. Every tool
//! result's text is framed as data ([`emperor_mcp::frame`], Emperor Profile E8).
//!
//! ```no_run
//! use datafusion::prelude::SessionContext;
//! use std::sync::Arc;
//! use tableski_core::{AppState, IngestOptions, call_tool, register_path};
//!
//! # async fn run() -> Result<(), String> {
//! let ctx = SessionContext::new();
//! let tables = register_path(&ctx, "orders.csv".as_ref(), &IngestOptions::default())
//!     .await
//!     .map_err(|e| e.to_string())?;
//! let state = AppState::new(Arc::new(ctx), tables);
//! let grid = call_tool(&state, "query_sql", serde_json::json!({ "sql": "SELECT count(*) FROM orders" })).await?;
//! println!("{grid}");
//! # Ok(()) }
//! ```
#![warn(missing_docs)]

use datafusion::arrow::datatypes::Schema;
use datafusion::arrow::util::pretty::pretty_format_batches;
use datafusion::execution::context::SQLOptions;
use datafusion::prelude::*;
use emperor_mcp::{FrameKind, McpHandler, frame};
use serde_json::{Value, json};
use std::sync::Arc;

pub mod excel;
pub mod export;
pub mod guard;
pub mod limits;
pub mod quota;
pub mod register;
pub mod source;
pub use excel::{HeaderMode, IngestOptions, SheetInfo, register_workbook};
pub use guard::{Rejection, SqlTrust, check_plan, check_untrusted};
pub use limits::{Collected, QueryLimits, collect_limited, query_runtime, run_bounded};
pub use quota::{Quota, QuotaExceeded, Usage, UsageMeter};
pub use register::register_path;
pub use source::{FileSource, TableSource, register_sources};

/// `Accept` value clients should send (re-exported from the shared transport).
pub use emperor_mcp::ACCEPT_STREAMABLE;

/// MCP protocol version reported on `initialize`.
pub const PROTOCOL_VERSION: &str = "2025-03-26";

/// One registered table and where it came from (CSV path or workbook sheet).
#[derive(Debug, Clone, serde::Serialize)]
pub struct TableEntry {
    /// SQL table name as registered in the session context.
    pub name: String,
    /// Path of the file the table was read from.
    pub source: String,
    /// Sheet name when the source is a workbook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    /// Data rows counted at ingest (workbook sheets only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<usize>,
    /// Columns counted at ingest (workbook sheets only).
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
    /// DataFusion session holding every registered table.
    pub ctx: Arc<SessionContext>,
    /// What is registered, in registration order; the first entry is the default table.
    pub tables: Vec<TableEntry>,
    /// Directory result exports may write into; exports are disabled when `None`.
    pub export_dir: Option<std::path::PathBuf>,
    /// Whether SQL from clients is checked by the [`guard`] before it runs.
    pub sql_trust: SqlTrust,
    /// Timeout and result caps applied to every query ([`QueryLimits::unlimited`] by default;
    /// the memory cap lives on the session, see [`QueryLimits::session_context`]).
    pub limits: QueryLimits,
    /// Daily query allowance and its meter; `None` = not metered (the default).
    pub quota: Option<(Quota, Arc<UsageMeter>)>,
}

impl AppState {
    /// Handler over an already-populated session; exports disabled until [`Self::with_export_dir`].
    pub fn new(ctx: Arc<SessionContext>, tables: Vec<TableEntry>) -> Self {
        Self {
            ctx,
            tables,
            export_dir: None,
            sql_trust: SqlTrust::Trusted,
            limits: QueryLimits::unlimited(),
            quota: None,
        }
    }

    /// Meter this state's SQL-planning tool calls against `quota.queries_per_day`. The meter
    /// is shared so several states (or a rebuilt one) can count for the same tenant.
    pub fn with_quota(mut self, quota: Quota, meter: Arc<UsageMeter>) -> Self {
        self.quota = Some((quota, meter));
        self
    }

    /// Apply `limits` (timeout, row and byte caps) to every query of this state.
    pub fn with_limits(mut self, limits: QueryLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Run a planned query under this state's limits. Every tool that returns rows goes through here.
    pub async fn collect(&self, df: DataFrame) -> Result<Collected, String> {
        collect_limited(df, &self.limits).await
    }

    /// Treat clients as untrusted: only read-only queries over registered tables run
    /// ([`check_untrusted`]), and DataFusion itself refuses DDL, DML and statements.
    pub fn untrusted(mut self) -> Self {
        self.sql_trust = SqlTrust::Untrusted;
        self
    }

    /// Plan `sql` under this state's trust level and timeout. Every tool that takes SQL goes
    /// through here. Planning runs on the query runtime like execution: the optimizer folds
    /// constants (`repeat('x', 10^9)`) and that work must not block the server either.
    pub async fn sql(&self, sql: &str) -> Result<DataFrame, String> {
        if let Some((quota, meter)) = &self.quota {
            meter.consume_query(quota).map_err(|e| e.to_string())?;
        }
        let ctx = Arc::clone(&self.ctx);
        let sql = sql.to_string();
        match self.sql_trust {
            SqlTrust::Trusted => {
                limits::run_bounded(
                    async move { ctx.sql(&sql).await.map_err(|e| e.to_string()) },
                    self.limits.timeout,
                    "planning",
                )
                .await
            }
            SqlTrust::Untrusted => {
                check_untrusted(&sql, &self.table_names()).map_err(|e| e.to_string())?;
                let options = SQLOptions::new()
                    .with_allow_ddl(false)
                    .with_allow_dml(false)
                    .with_allow_statements(false);
                limits::run_bounded(
                    async move {
                        let df = ctx
                            .sql_with_options(&sql, options)
                            .await
                            .map_err(|e| e.to_string())?;
                        let plan = df
                            .clone()
                            .create_physical_plan()
                            .await
                            .map_err(|e| e.to_string())?;
                        guard::check_plan(&plan).map_err(|e| e.to_string())?;
                        Ok(df)
                    },
                    self.limits.timeout,
                    "planning",
                )
                .await
            }
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

/// Run one tool by name with its JSON arguments and return the result text (unframed).
///
/// The names and argument shapes are those of [`tools_list`]: `list_tables`, `query_sql`,
/// `get_schema`, `export_result`, `column_statistics`. Errors are the same messages an MCP
/// client would see in the JSON-RPC error.
pub async fn call_tool(state: &AppState, name: &str, args: Value) -> Result<String, String> {
    let body = json!({ "params": { "name": name, "arguments": args } });
    let result = run_tool_call(state, &body).await?;
    Ok(result["content"][0]["text"]
        .as_str()
        .map(|t| {
            t.strip_prefix(frame(FrameKind::Computed, "").as_str())
                .unwrap_or(t)
                .to_string()
        })
        .unwrap_or_default())
}

/// The MCP `tools/list` result: the five tools with their JSON input schemas.
pub fn tools_list() -> Value {
    tools_list_json()
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
            let df = state.sql(sql).await?;
            let collected = state.collect(df).await?;
            let mut text = pretty_format_batches(&collected.batches)
                .map_err(|e| e.to_string())?
                .to_string();
            if let Some(note) = collected.truncation_note(&state.limits) {
                text.push('\n');
                text.push_str(&note);
            }
            Ok(framed_text(&text))
        }
        "get_schema" => {
            let table = state.resolve_table(&args)?;
            let sql = format!("SELECT * FROM {table} LIMIT 0");
            let df = state.sql(&sql).await?;
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
            let df = state.sql(sql).await?;
            let collected = state.collect(df).await?;
            if collected.truncated {
                return Err(format!(
                    "export refused: the result exceeds the configured cap ({} rows collected); narrow the query or raise --max-result-rows / --max-result-mb",
                    collected.rows
                ));
            }
            let summary = export::export_query(collected.batches, dir, file).await?;
            let j = serde_json::to_value(&summary).map_err(|e| e.to_string())?;
            Ok(framed_text(
                &serde_json::to_string_pretty(&j).unwrap_or_else(|_| j.to_string()),
            ))
        }
        "column_statistics" => {
            let table = state.resolve_table(&args)?;
            let sql = format!("SELECT * FROM {table}");
            let df = state.sql(&sql).await?;
            let desc = df.describe().await.map_err(|e| e.to_string())?;
            let collected = state.collect(desc).await?;
            let text = pretty_format_batches(&collected.batches)
                .map_err(|e| e.to_string())?
                .to_string();
            Ok(framed_text(&text))
        }
        _ => Err(format!("unknown tool: {name}")),
    }
}
