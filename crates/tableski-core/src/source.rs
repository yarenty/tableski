//! Where tables come from: the [`TableSource`] seam between a supplier of tables and the
//! DataFusion [`SessionContext`] that serves them.
//!
//! # Model
//!
//! A *registry* is one `SessionContext` plus the [`TableEntry`] list describing what is in
//! it; [`AppState`](crate::AppState) wraps exactly one registry. A registry is built once by
//! calling [`register_sources`] over a list of sources, each of which registers the tables
//! it knows about. The server itself stays **stateless**: it never mutates the registry
//! after the build, every tool call is independent, and nothing about a client is stored
//! between calls.
//!
//! Today's only shipped source is [`FileSource`], the `--file` / `--csv` / `--xlsx` model.
//!
//! # Refreshing and streaming sources (Phase 4: live bars)
//!
//! A source that changes after registration keeps its state on **its own side** of the
//! seam and hands DataFusion a [`TableProvider`](datafusion::catalog::TableProvider) whose
//! `scan` reads the current state:
//!
//! - **Appending feed** (ticks, bars): the provider holds `Arc<RwLock<Vec<RecordBatch>>>`;
//!   the feed pushes batches, `scan` snapshots the vector. Every query sees the rows that
//!   exist when it is planned; no query blocks on the feed. The test-only source in
//!   `tests/table_source.rs` is exactly this shape.
//! - **Periodic re-read** (a file that is rewritten): the same provider, with a task that
//!   swaps the batches on a timer, or a provider whose `scan` re-opens the file when its
//!   mtime changed. Registration stays a one-time event.
//! - **Windowing / retention** belongs to the source (drop batches older than N), not to the
//!   registry: the registry does not know what the data means.
//!
//! What such a source must implement: [`TableSource::register`] (once), a `TableProvider`
//! whose `scan` is cheap and never awaits the feed, and its own locking. What it must not
//! do: call back into the registry, or hold a reference to the `SessionContext` after
//! `register` returns.
//!
//! # A registry scoped to a client or a session (Phase 2-3: tableski-cloud)
//!
//! Nothing here assumes one registry per process. A multi-tenant server keeps a map
//! `tenant -> AppState`, each built with [`register_sources`] over that tenant's sources
//! (uploaded files, later a feed subscription), lazily and with LRU eviction. Per-tenant
//! memory pools come from [`QueryLimits::session_context`](crate::QueryLimits::session_context).
//! The file-based path does not change for that: a tenant with uploaded files is a
//! `FileSource` over its storage directory.

use crate::TableEntry;
use crate::excel::{IngestOptions, register_workbook};
use crate::register::register_path;
use datafusion::prelude::*;
use futures::future::BoxFuture;
use std::path::PathBuf;

/// A supplier of tables. Implementations register whatever they know about into the
/// session and describe it; see the module docs for stateful sources.
pub trait TableSource: Send + Sync {
    /// Short label for logs and diagnostics, e.g. `"files"`.
    fn kind(&self) -> &'static str;

    /// Register every table of this source into `ctx` and return one entry per table.
    /// Called once when the registry is built. Table names must not collide with tables
    /// already in `ctx` (use `ctx.table_exist`, as [`FileSource`] does).
    fn register<'a>(
        &'a self,
        ctx: &'a SessionContext,
        opts: &'a IngestOptions,
    ) -> BoxFuture<'a, Result<Vec<TableEntry>, String>>;
}

/// Build a registry: register `sources` in order into `ctx` and collect their entries.
pub async fn register_sources(
    ctx: &SessionContext,
    sources: &[Box<dyn TableSource>],
    opts: &IngestOptions,
) -> Result<Vec<TableEntry>, String> {
    let mut tables = Vec::new();
    for source in sources {
        tables.extend(
            source
                .register(ctx, opts)
                .await
                .map_err(|e| format!("{} source: {e}", source.kind()))?,
        );
    }
    Ok(tables)
}

/// Files on disk: the CLI's `--file` (extension picks the reader, table = file stem),
/// `--csv` with an explicit table name, and `--xlsx` (one table per sheet).
#[derive(Debug, Clone, Default)]
pub struct FileSource {
    items: Vec<FileItem>,
}

#[derive(Debug, Clone)]
enum FileItem {
    /// Reader by extension, table named after the file stem (`--file`).
    Path(PathBuf),
    /// A CSV under an explicit table name (`--csv` + `--table`).
    CsvNamed { path: PathBuf, table: String },
    /// A workbook, one table per non-empty sheet (`--xlsx`).
    Workbook(PathBuf),
}

impl FileSource {
    /// An empty source; add files with the builder methods.
    pub fn new() -> Self {
        Self::default()
    }

    /// Everything `--file` accepts, in order.
    pub fn from_paths<I, P>(paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        Self {
            items: paths
                .into_iter()
                .map(|p| FileItem::Path(p.into()))
                .collect(),
        }
    }

    /// Add a data file; the extension picks the reader and the stem names the table.
    pub fn path(mut self, path: impl Into<PathBuf>) -> Self {
        self.items.push(FileItem::Path(path.into()));
        self
    }

    /// Add a CSV under an explicit table name.
    pub fn csv_named(mut self, path: impl Into<PathBuf>, table: impl Into<String>) -> Self {
        self.items.push(FileItem::CsvNamed {
            path: path.into(),
            table: table.into(),
        });
        self
    }

    /// Add a workbook (xlsx / xls / ods); every non-empty sheet becomes a table.
    pub fn workbook(mut self, path: impl Into<PathBuf>) -> Self {
        self.items.push(FileItem::Workbook(path.into()));
        self
    }

    /// Number of files queued.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// `true` when nothing was added.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl TableSource for FileSource {
    fn kind(&self) -> &'static str {
        "files"
    }

    fn register<'a>(
        &'a self,
        ctx: &'a SessionContext,
        opts: &'a IngestOptions,
    ) -> BoxFuture<'a, Result<Vec<TableEntry>, String>> {
        Box::pin(async move {
            let mut tables = Vec::new();
            for item in &self.items {
                match item {
                    FileItem::Path(path) => tables.extend(register_path(ctx, path, opts).await?),
                    FileItem::CsvNamed { path, table } => {
                        if !path.exists() {
                            return Err(format!("CSV not found: {}", path.display()));
                        }
                        let p = path
                            .to_str()
                            .ok_or_else(|| "CSV path must be valid UTF-8".to_string())?;
                        ctx.register_csv(table, p, CsvReadOptions::new())
                            .await
                            .map_err(|e| e.to_string())?;
                        tables.push(TableEntry::csv(table, p));
                    }
                    FileItem::Workbook(path) => {
                        if !path.exists() {
                            return Err(format!("workbook not found: {}", path.display()));
                        }
                        for info in register_workbook(ctx, path, opts)? {
                            tables.push(TableEntry::sheet(&info, path.display().to_string()));
                        }
                    }
                }
            }
            Ok(tables)
        })
    }
}
