//! Per-query limits: a memory pool cap on the session, a wall-clock timeout, and caps on the
//! rows and bytes a result may carry back. Unlimited by default for embedders; the CLI applies
//! [`QueryLimits::safe`] unless told otherwise.

use datafusion::arrow::record_batch::RecordBatch;
use datafusion::execution::disk_manager::{DiskManagerBuilder, DiskManagerMode};
use datafusion::execution::runtime_env::RuntimeEnvBuilder;
use datafusion::prelude::{DataFrame, SessionConfig, SessionContext};
use futures::StreamExt;
use std::future::Future;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// The process-wide tokio runtime queries execute on when a timeout is set: one thread per
/// core, separate from whatever runtime serves clients, so a busy query cannot starve the
/// server. Built on first use.
pub fn query_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(std::thread::available_parallelism().map_or(2, |n| n.get()))
            .thread_name("tableski-query")
            .enable_all()
            .build()
            .expect("query runtime")
    })
}

/// Bounds on what one query may consume and return. `None` means unlimited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QueryLimits {
    /// Memory pool for the whole session, in bytes; every query draws from it and fails with
    /// `Resources exhausted` instead of growing the process. With a cap set, spilling to disk
    /// is disabled too, so a hostile query fails fast rather than filling the disk.
    pub memory_bytes: Option<usize>,
    /// Wall-clock budget for planning plus execution of one query.
    pub timeout: Option<Duration>,
    /// Most rows a result may carry; the rest is dropped and the result marked truncated.
    pub max_rows: Option<usize>,
    /// Most bytes (Arrow in-memory size) a result may carry; same truncation rule.
    pub max_bytes: Option<usize>,
}

impl QueryLimits {
    /// The CLI defaults: 2 GiB pool, 60 s, 10 000 rows, 16 MiB.
    pub const SAFE_MEMORY_BYTES: usize = 2 * 1024 * 1024 * 1024;
    /// See [`Self::safe`].
    pub const SAFE_TIMEOUT: Duration = Duration::from_secs(60);
    /// See [`Self::safe`].
    pub const SAFE_MAX_ROWS: usize = 10_000;
    /// See [`Self::safe`].
    pub const SAFE_MAX_BYTES: usize = 16 * 1024 * 1024;

    /// No limits at all: the pre-0.2 behaviour, and the default of [`AppState`](crate::AppState).
    pub fn unlimited() -> Self {
        Self::default()
    }

    /// Defaults that keep one query from taking the machine down; what the CLI uses.
    pub fn safe() -> Self {
        Self {
            memory_bytes: Some(Self::SAFE_MEMORY_BYTES),
            timeout: Some(Self::SAFE_TIMEOUT),
            max_rows: Some(Self::SAFE_MAX_ROWS),
            max_bytes: Some(Self::SAFE_MAX_BYTES),
        }
    }

    /// A session whose memory pool honours [`Self::memory_bytes`] (unbounded when `None`).
    /// Register tables into this context, then build the state with it.
    pub fn session_context(&self) -> Result<SessionContext, String> {
        let mut runtime = RuntimeEnvBuilder::new();
        if let Some(bytes) = self.memory_bytes {
            runtime = runtime
                .with_memory_limit(bytes, 1.0)
                .with_disk_manager_builder(
                    DiskManagerBuilder::default().with_mode(DiskManagerMode::Disabled),
                );
        }
        let runtime = runtime.build_arc().map_err(|e| e.to_string())?;
        Ok(SessionContext::new_with_config_rt(
            SessionConfig::new(),
            runtime,
        ))
    }
}

/// What [`collect_limited`] brought back.
#[derive(Debug, Clone, Default)]
pub struct Collected {
    /// The result batches, cut at the caps.
    pub batches: Vec<RecordBatch>,
    /// Rows in `batches`.
    pub rows: usize,
    /// Arrow in-memory size of `batches`.
    pub bytes: usize,
    /// Set when [`QueryLimits::max_rows`] or [`QueryLimits::max_bytes`] cut the result.
    pub truncated: bool,
}

impl Collected {
    /// One line telling the client what was cut and how to get the rest, or `None`.
    pub fn truncation_note(&self, limits: &QueryLimits) -> Option<String> {
        if !self.truncated {
            return None;
        }
        let cap = match (limits.max_rows, limits.max_bytes) {
            (Some(r), _) if self.rows >= r => format!("{r} rows"),
            (_, Some(b)) => format!("{} MiB", b / (1024 * 1024)),
            (Some(r), None) => format!("{r} rows"),
            (None, None) => "the configured cap".to_string(),
        };
        Some(format!(
            "-- result truncated at {cap} ({} rows returned); add a LIMIT / WHERE, aggregate, or use export_result",
            self.rows
        ))
    }
}

/// Run `work` under `timeout` on the [`query_runtime`]: the caller's runtime (HTTP, timers)
/// stays responsive however busy the work is, and on expiry the task is aborted (it stops at
/// its next yield point). Without a timeout, `work` runs inline.
pub async fn run_bounded<T, F>(work: F, timeout: Option<Duration>, what: &str) -> Result<T, String>
where
    T: Send + 'static,
    F: Future<Output = Result<T, String>> + Send + 'static,
{
    let Some(budget) = timeout else {
        return work.await;
    };
    let started = Instant::now();
    let mut task = query_runtime().spawn(work);
    match tokio::time::timeout(budget, &mut task).await {
        Ok(joined) => joined.map_err(|e| format!("{what} task failed: {e}"))?,
        Err(_) => {
            task.abort();
            Err(format!(
                "{what} cancelled after {:.1}s (wall-clock limit {:.0}s; narrow the query or raise --query-timeout-secs)",
                started.elapsed().as_secs_f64(),
                budget.as_secs_f64()
            ))
        }
    }
}

/// Execute `df` as a stream, stop at the row/byte caps, and give up at the timeout.
///
/// Stopping early means a runaway `SELECT *` never materialises past the cap. With a timeout
/// the query runs on [`query_runtime`] via [`run_bounded`]; the caller's runtime and the
/// session stay usable. A plan that never yields keeps its query thread busy until it does
/// (see [`check_plan`](crate::guard::check_plan) for why cartesian products are refused).
pub async fn collect_limited(df: DataFrame, limits: &QueryLimits) -> Result<Collected, String> {
    let (max_rows, max_bytes) = (limits.max_rows, limits.max_bytes);
    run_bounded(
        collect_capped(df, max_rows, max_bytes),
        limits.timeout,
        "query",
    )
    .await
    .map_err(|e| {
        if e.contains("Resources exhausted") {
            format!("{e} (memory pool limit; narrow the query or raise --max-memory-mb)")
        } else {
            e
        }
    })
}

async fn collect_capped(
    df: DataFrame,
    max_rows: Option<usize>,
    max_bytes: Option<usize>,
) -> Result<Collected, String> {
    let mut stream = df.execute_stream().await.map_err(|e| e.to_string())?;
    let mut out = Collected::default();
    while let Some(batch) = stream.next().await {
        let mut batch = batch.map_err(|e| e.to_string())?;
        if let Some(cap) = max_rows {
            let room = cap.saturating_sub(out.rows);
            if batch.num_rows() > room {
                batch = batch.slice(0, room);
                out.truncated = true;
            }
        }
        if let Some(cap) = max_bytes {
            let size = batch.get_array_memory_size();
            if out.bytes + size > cap && !out.batches.is_empty() {
                out.truncated = true;
                break;
            }
            if out.bytes + size > cap {
                out.truncated = true;
            }
        }
        out.rows += batch.num_rows();
        out.bytes += batch.get_array_memory_size();
        if batch.num_rows() > 0 {
            out.batches.push(batch);
        }
        if out.truncated {
            break;
        }
    }
    Ok(out)
}
