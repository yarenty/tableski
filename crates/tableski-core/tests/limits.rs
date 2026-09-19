//! Per-query limits (#12): result caps truncate with a note, the timeout and the memory pool
//! cut a runaway query with a clean error, and the session answers the next query.
use datafusion::prelude::SessionContext;
use std::sync::Arc;
use std::time::Duration;
use tableski_core::{AppState, QueryLimits, TableEntry, call_tool};

fn state_with(ctx: SessionContext, limits: QueryLimits) -> AppState {
    AppState::new(Arc::new(ctx), Vec::<TableEntry>::new()).with_limits(limits)
}

async fn query(state: &AppState, sql: &str) -> Result<String, String> {
    call_tool(state, "query_sql", serde_json::json!({ "sql": sql })).await
}

#[tokio::test]
async fn row_cap_truncates_and_says_so() {
    let limits = QueryLimits {
        max_rows: Some(100),
        ..QueryLimits::unlimited()
    };
    let s = state_with(SessionContext::new(), limits);
    let text = query(&s, "SELECT * FROM range(1, 1000000)").await.unwrap();
    let data_rows = text
        .lines()
        .filter(|l| l.starts_with("| ") && !l.contains("value"))
        .count();
    assert_eq!(data_rows, 100, "{text}");
    assert!(
        text.contains("result truncated at 100 rows (100 rows returned)"),
        "{text}"
    );

    let text = query(&s, "SELECT * FROM range(1, 50)").await.unwrap();
    assert!(!text.contains("truncated"), "{text}");
}

#[tokio::test]
async fn byte_cap_truncates_too() {
    let limits = QueryLimits {
        max_bytes: Some(64 * 1024),
        ..QueryLimits::unlimited()
    };
    let s = state_with(SessionContext::new(), limits);
    let text = query(&s, "SELECT * FROM range(1, 1000000)").await.unwrap();
    assert!(text.contains("result truncated"), "{text}");
    assert!(text.lines().count() < 20_000, "{}", text.lines().count());
}

#[tokio::test]
async fn timeout_cancels_and_session_survives() {
    let limits = QueryLimits {
        timeout: Some(Duration::from_millis(100)),
        ..QueryLimits::unlimited()
    };
    let s = state_with(SessionContext::new(), limits);
    // Building the hash table over 10M rows takes well over 100 ms and yields between batches.
    let err = query(
        &s,
        "SELECT count(*) FROM range(1, 10000000) a JOIN range(1, 10000000) b ON a.value = b.value",
    )
    .await
    .unwrap_err();
    assert!(err.contains("query cancelled after"), "{err}");
    assert!(err.contains("--query-timeout-secs"), "{err}");

    let text = query(&s, "SELECT 1 AS alive").await.unwrap();
    assert!(text.contains("alive"), "{text}");
}

#[tokio::test]
async fn memory_pool_cuts_a_big_sort_and_session_survives() {
    let limits = QueryLimits {
        memory_bytes: Some(8 * 1024 * 1024),
        timeout: Some(Duration::from_secs(60)),
        ..QueryLimits::unlimited()
    };
    let ctx = limits.session_context().unwrap();
    let s = state_with(ctx, limits);
    let err = query(&s, "SELECT * FROM range(1, 20000000) ORDER BY value DESC")
        .await
        .unwrap_err();
    assert!(err.contains("Resources exhausted"), "{err}");
    assert!(err.contains("--max-memory-mb"), "{err}");

    let text = query(&s, "SELECT count(*) FROM range(1, 1000)")
        .await
        .unwrap();
    assert!(text.contains("999"), "{text}");
}

#[tokio::test]
async fn export_refuses_a_truncated_result() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("limits_export");
    let limits = QueryLimits {
        max_rows: Some(10),
        ..QueryLimits::unlimited()
    };
    let s = state_with(SessionContext::new(), limits).with_export_dir(&dir);
    let err = call_tool(
        &s,
        "export_result",
        serde_json::json!({ "sql": "SELECT * FROM range(1, 1000)", "file": "big.csv" }),
    )
    .await
    .unwrap_err();
    assert!(err.starts_with("export refused"), "{err}");
    assert!(!dir.join("big.csv").exists());

    let ok = call_tool(
        &s,
        "export_result",
        serde_json::json!({ "sql": "SELECT * FROM range(1, 5)", "file": "small.csv" }),
    )
    .await
    .unwrap();
    assert!(ok.contains("\"rows\": 4"), "{ok}");
}

#[test]
fn safe_defaults_are_what_the_readme_says() {
    let l = QueryLimits::safe();
    assert_eq!(l.memory_bytes, Some(2 * 1024 * 1024 * 1024));
    assert_eq!(l.timeout, Some(Duration::from_secs(60)));
    assert_eq!(l.max_rows, Some(10_000));
    assert_eq!(l.max_bytes, Some(16 * 1024 * 1024));
    assert_eq!(QueryLimits::unlimited(), QueryLimits::default());
}
