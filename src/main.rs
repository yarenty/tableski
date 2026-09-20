use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tableski::{
    ACCEPT_STREAMABLE, AppState, FileSource, HeaderMode, IngestOptions, QueryLimits, TableSource,
    app_router, register_sources,
};

#[derive(Parser, Debug)]
#[command(name = "tableski")]
#[command(
    about = "Every spreadsheet is a table — SQL, schema, and column stats over CSV/Excel via MCP"
)]
struct Args {
    #[arg(long, default_value = "0.0.0.0:8080")]
    bind: String,
    /// CSV file to register (table name via --table).
    #[arg(long)]
    csv: Option<PathBuf>,
    /// Excel workbook (xlsx / xls / ods); every non-empty sheet becomes a table.
    #[arg(long)]
    xlsx: Option<PathBuf>,
    /// Table name for --csv.
    #[arg(long, default_value = "data")]
    table: String,
    /// First-row handling for workbook sheets.
    #[arg(long, value_enum, default_value_t = HeaderMode::Auto)]
    headers: HeaderMode,
    /// Maximum data rows per sheet (exceeding this is an error, never a silent cut).
    #[arg(long, default_value_t = 1_000_000)]
    max_rows: usize,
    /// Data file to register; repeatable. Extension picks the reader:
    /// .csv .parquet .json/.ndjson/.jsonl .xlsx/.xls/.ods. Table name = file stem
    /// (workbooks: one table per sheet).
    #[arg(long = "file")]
    files: Vec<PathBuf>,
    /// Enable the export_result tool, sandboxed to this directory.
    #[arg(long)]
    export_dir: Option<PathBuf>,
    /// Treat clients as untrusted: only read-only queries (SELECT / WITH / EXPLAIN) over the
    /// registered tables run; DDL, DML, COPY, SET and table functions are rejected.
    #[arg(long)]
    untrusted_sql: bool,
    /// Memory pool for the whole session in MiB; a query that needs more fails instead of
    /// growing the process. 0 = unlimited (and spilling to disk allowed again).
    #[arg(long, default_value_t = 2048)]
    max_memory_mb: u64,
    /// Wall-clock budget per query in seconds. 0 = unlimited.
    #[arg(long, default_value_t = 60)]
    query_timeout_secs: u64,
    /// Most rows a query_sql result may carry back; the rest is cut and the result says so.
    /// 0 = unlimited.
    #[arg(long, default_value_t = 10_000)]
    max_result_rows: usize,
    /// Most MiB (Arrow in-memory size) a result may carry back. 0 = unlimited.
    #[arg(long, default_value_t = 16)]
    max_result_mb: usize,
}

fn limits_from(args: &Args) -> QueryLimits {
    let nonzero = |v: usize| (v > 0).then_some(v);
    QueryLimits {
        memory_bytes: nonzero(args.max_memory_mb as usize).map(|mb| mb * 1024 * 1024),
        timeout: (args.query_timeout_secs > 0)
            .then(|| std::time::Duration::from_secs(args.query_timeout_secs)),
        max_rows: nonzero(args.max_result_rows),
        max_bytes: nonzero(args.max_result_mb).map(|mb| mb * 1024 * 1024),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.csv.is_none() && args.xlsx.is_none() && args.files.is_empty() {
        return Err("nothing to serve: pass --file <data-file> (or --csv/--xlsx)".into());
    }

    let limits = limits_from(&args);
    let ctx = limits.session_context()?;
    let opts = IngestOptions {
        headers: args.headers,
        max_rows: args.max_rows,
    };
    // Every flag becomes an item of one file source; other kinds of source plug in here.
    let mut files = FileSource::new();
    if let Some(csv) = &args.csv {
        files = files.csv_named(csv, &args.table);
    }
    if let Some(xlsx) = &args.xlsx {
        files = files.workbook(xlsx);
    }
    for file in &args.files {
        files = files.path(file);
    }
    let sources: Vec<Box<dyn TableSource>> = vec![Box::new(files)];
    let tables = register_sources(&ctx, &sources, &opts).await?;

    for t in &tables {
        match &t.sheet {
            Some(sheet) => eprintln!(
                "tableski: table `{}` <- sheet `{}` of `{}` ({} rows)",
                t.name,
                sheet,
                t.source,
                t.rows.unwrap_or(0)
            ),
            None => eprintln!("tableski: table `{}` <- `{}`", t.name, t.source),
        }
    }

    let mut state = AppState::new(Arc::new(ctx), tables).with_limits(limits);
    eprintln!(
        "tableski: limits: memory {} MiB, timeout {} s, result {} rows / {} MiB (0 = unlimited)",
        args.max_memory_mb, args.query_timeout_secs, args.max_result_rows, args.max_result_mb
    );
    if let Some(dir) = &args.export_dir {
        std::fs::create_dir_all(dir)?;
        eprintln!("tableski: export_result enabled -> {}", dir.display());
        state = state.with_export_dir(dir);
    }
    if args.untrusted_sql {
        eprintln!("tableski: --untrusted-sql: read-only queries over registered tables only");
        state = state.untrusted();
    }
    let app = app_router(state);
    let addr: SocketAddr = args.bind.parse()?;
    eprintln!("tableski: stateless Streamable HTTP on http://{addr}");
    eprintln!("Accept header for clients: `{ACCEPT_STREAMABLE}`");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
