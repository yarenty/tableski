use clap::Parser;
use datafusion::prelude::*;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tableski::{
    ACCEPT_STREAMABLE, AppState, HeaderMode, IngestOptions, TableEntry, app_router,
    register_workbook,
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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.csv.is_none() && args.xlsx.is_none() {
        return Err("nothing to serve: pass --csv <file> and/or --xlsx <workbook>".into());
    }

    let ctx = SessionContext::new();
    let mut tables = Vec::new();

    if let Some(csv) = &args.csv {
        if !csv.exists() {
            return Err(format!("CSV not found: {}", csv.display()).into());
        }
        let path = csv.to_str().ok_or("CSV path must be valid UTF-8")?;
        ctx.register_csv(&args.table, path, CsvReadOptions::new())
            .await?;
        tables.push(TableEntry::csv(&args.table, path));
    }

    if let Some(xlsx) = &args.xlsx {
        if !xlsx.exists() {
            return Err(format!("workbook not found: {}", xlsx.display()).into());
        }
        let opts = IngestOptions {
            headers: args.headers,
            max_rows: args.max_rows,
        };
        let infos = register_workbook(&ctx, xlsx, &opts)?;
        for info in &infos {
            tables.push(TableEntry::sheet(info, xlsx.display().to_string()));
        }
    }

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

    let state = AppState::new(Arc::new(ctx), tables);
    let app = app_router(state);
    let addr: SocketAddr = args.bind.parse()?;
    eprintln!("tableski: stateless Streamable HTTP on http://{addr}");
    eprintln!("Accept header for clients: `{ACCEPT_STREAMABLE}`");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
