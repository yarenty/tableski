//! Quota primitives: what a tenant may use, and an in-memory meter that enforces it.
//!
//! Plain types on purpose: the hosted server decides the numbers per tier and persists usage
//! (Postgres, for billing and display); this module only counts and says no. Self-hosters get
//! the same protection with one [`UsageMeter`] per process.
//!
//! - **Queries per day**: consumed once per tool call that plans SQL
//!   ([`AppState::sql`](crate::AppState::sql)); the day is a UTC calendar day.
//! - **Bytes stored / files / bytes per file**: reserved when a file is accepted, released when
//!   it is deleted; the upload path calls [`UsageMeter::add_file`].
//! - **Rows per file**: the ingest cap that already exists ([`IngestOptions::max_rows`]);
//!   [`Quota::ingest_options`] applies it.

use crate::excel::IngestOptions;
use std::fmt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// The allowance of one tenant. `None` = unlimited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Quota {
    /// Tool calls that plan SQL, per UTC day.
    pub queries_per_day: Option<u64>,
    /// Total bytes of files kept for the tenant.
    pub bytes_stored: Option<u64>,
    /// Number of files kept for the tenant.
    pub files: Option<usize>,
    /// Size of any single file.
    pub bytes_per_file: Option<u64>,
    /// Data rows in any single file or sheet.
    pub rows_per_file: Option<usize>,
}

impl Quota {
    /// No limits (the default).
    pub fn unlimited() -> Self {
        Self::default()
    }

    /// Ingest options with [`Self::rows_per_file`] applied on top of `base` (the smaller wins).
    pub fn ingest_options(&self, base: IngestOptions) -> IngestOptions {
        IngestOptions {
            max_rows: match self.rows_per_file {
                Some(cap) => base.max_rows.min(cap),
                None => base.max_rows,
            },
            ..base
        }
    }

    /// Check one file's size against the per-file cap.
    pub fn check_file_bytes(&self, bytes: u64, file: &str) -> Result<(), QuotaExceeded> {
        match self.bytes_per_file {
            Some(cap) if bytes > cap => Err(QuotaExceeded::BytesPerFile {
                limit: cap,
                bytes,
                file: file.to_string(),
            }),
            _ => Ok(()),
        }
    }

    /// Check a row count against the per-file cap (for sources that count before ingest).
    pub fn check_rows(&self, rows: usize, file: &str) -> Result<(), QuotaExceeded> {
        match self.rows_per_file {
            Some(cap) if rows > cap => Err(QuotaExceeded::RowsPerFile {
                limit: cap,
                rows,
                file: file.to_string(),
            }),
            _ => Ok(()),
        }
    }
}

/// Which allowance ran out. The `Display` text is what the client sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuotaExceeded {
    /// Daily query allowance used up.
    QueriesPerDay {
        /// The allowance.
        limit: u64,
        /// Queries already counted today.
        used: u64,
    },
    /// Adding the file would exceed the stored-bytes allowance.
    BytesStored {
        /// The allowance.
        limit: u64,
        /// Bytes stored before this request.
        used: u64,
        /// Bytes the request wanted to add.
        requested: u64,
    },
    /// Adding the file would exceed the file-count allowance.
    Files {
        /// The allowance.
        limit: usize,
        /// Files stored before this request.
        used: usize,
    },
    /// One file is larger than allowed.
    BytesPerFile {
        /// The allowance.
        limit: u64,
        /// The file's size.
        bytes: u64,
        /// The file.
        file: String,
    },
    /// One file or sheet has more rows than allowed.
    RowsPerFile {
        /// The allowance.
        limit: usize,
        /// Rows found.
        rows: usize,
        /// The file.
        file: String,
    },
}

impl fmt::Display for QuotaExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueriesPerDay { limit, used } => write!(
                f,
                "quota: {used} of {limit} queries used today; the allowance resets at midnight UTC"
            ),
            Self::BytesStored {
                limit,
                used,
                requested,
            } => write!(
                f,
                "quota: storing {requested} more bytes would exceed {limit} bytes ({used} in use); delete a file first"
            ),
            Self::Files { limit, used } => {
                write!(
                    f,
                    "quota: {used} of {limit} files in use; delete a file first"
                )
            }
            Self::BytesPerFile { limit, bytes, file } => {
                write!(
                    f,
                    "quota: `{file}` is {bytes} bytes, the limit per file is {limit}"
                )
            }
            Self::RowsPerFile { limit, rows, file } => {
                write!(
                    f,
                    "quota: `{file}` has {rows} rows, the limit per file is {limit}"
                )
            }
        }
    }
}

impl std::error::Error for QuotaExceeded {}

/// Days since the Unix epoch, UTC. The unit of the daily counter.
pub fn today() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0)
}

/// Point-in-time usage, for display and for the hosted server's metering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct Usage {
    /// The UTC day (days since epoch) `queries_today` refers to.
    pub day: u64,
    /// Queries counted on `day`.
    pub queries_today: u64,
    /// Bytes currently stored.
    pub bytes_stored: u64,
    /// Files currently stored.
    pub files: usize,
}

/// Thread-safe in-memory counters for one tenant, checked against a [`Quota`].
///
/// Process-local: restarting the server resets the day counter and storage must be re-added
/// from what is on disk. Durable accounting is the hosted server's job.
#[derive(Debug, Default)]
pub struct UsageMeter {
    queries: Mutex<(u64, u64)>, // (day, count)
    bytes: AtomicU64,
    files: AtomicUsize,
}

impl UsageMeter {
    /// Fresh counters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Counters primed with what already exists on disk (after a restart).
    pub fn with_storage(bytes: u64, files: usize) -> Self {
        Self {
            queries: Mutex::new((0, 0)),
            bytes: AtomicU64::new(bytes),
            files: AtomicUsize::new(files),
        }
    }

    /// Count one query against today's allowance.
    pub fn consume_query(&self, quota: &Quota) -> Result<(), QuotaExceeded> {
        self.consume_query_on(today(), quota)
    }

    /// Count one query on an explicit day (tests, replay). A new day resets the counter.
    pub fn consume_query_on(&self, day: u64, quota: &Quota) -> Result<(), QuotaExceeded> {
        let mut q = self.queries.lock().unwrap_or_else(|e| e.into_inner());
        if q.0 != day {
            *q = (day, 0);
        }
        if let Some(limit) = quota.queries_per_day
            && q.1 >= limit
        {
            return Err(QuotaExceeded::QueriesPerDay { limit, used: q.1 });
        }
        q.1 += 1;
        Ok(())
    }

    /// Accept a file of `bytes`: checks per-file size, file count and total bytes, then
    /// reserves them. Nothing is reserved when any check fails.
    pub fn add_file(&self, bytes: u64, file: &str, quota: &Quota) -> Result<(), QuotaExceeded> {
        quota.check_file_bytes(bytes, file)?;
        let files = self.files.load(Ordering::SeqCst);
        if let Some(limit) = quota.files
            && files >= limit
        {
            return Err(QuotaExceeded::Files { limit, used: files });
        }
        let used = self.bytes.load(Ordering::SeqCst);
        if let Some(limit) = quota.bytes_stored
            && used.saturating_add(bytes) > limit
        {
            return Err(QuotaExceeded::BytesStored {
                limit,
                used,
                requested: bytes,
            });
        }
        self.bytes.fetch_add(bytes, Ordering::SeqCst);
        self.files.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    /// A file of `bytes` was deleted.
    pub fn remove_file(&self, bytes: u64) {
        let _ = self
            .bytes
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |b| {
                Some(b.saturating_sub(bytes))
            });
        let _ = self
            .files
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |f| {
                Some(f.saturating_sub(1))
            });
    }

    /// Snapshot of the counters.
    pub fn usage(&self) -> Usage {
        let q = *self.queries.lock().unwrap_or_else(|e| e.into_inner());
        Usage {
            day: q.0,
            queries_today: q.1,
            bytes_stored: self.bytes.load(Ordering::SeqCst),
            files: self.files.load(Ordering::SeqCst),
        }
    }
}
