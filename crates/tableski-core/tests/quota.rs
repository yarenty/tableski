//! Quota primitives: counting, rollover, storage reservation, and enforcement at the state.
use datafusion::prelude::SessionContext;
use std::path::PathBuf;
use std::sync::Arc;
use tableski_core::{
    AppState, IngestOptions, Quota, QuotaExceeded, UsageMeter, call_tool, register_path,
};

/// The draft free tier from the plan: one file up to 5 MB, 100 queries a day.
fn free_tier() -> Quota {
    Quota {
        queries_per_day: Some(100),
        bytes_stored: Some(5 * 1024 * 1024),
        files: Some(1),
        bytes_per_file: Some(5 * 1024 * 1024),
        rows_per_file: Some(100_000),
    }
}

#[test]
fn daily_queries_count_and_reset_at_the_next_day() {
    let quota = Quota {
        queries_per_day: Some(3),
        ..Quota::unlimited()
    };
    let meter = UsageMeter::new();
    for _ in 0..3 {
        meter.consume_query_on(20_000, &quota).unwrap();
    }
    let err = meter.consume_query_on(20_000, &quota).unwrap_err();
    assert_eq!(err, QuotaExceeded::QueriesPerDay { limit: 3, used: 3 });
    assert!(err.to_string().contains("3 of 3 queries used today"));
    assert_eq!(meter.usage().queries_today, 3);

    meter.consume_query_on(20_001, &quota).unwrap();
    assert_eq!(
        meter.usage(),
        tableski_core::Usage {
            day: 20_001,
            queries_today: 1,
            bytes_stored: 0,
            files: 0
        }
    );

    // A host restores today's count from its records; the allowance continues.
    let restored = UsageMeter::new();
    restored.prime_queries(20_002, 2);
    restored.consume_query_on(20_002, &quota).unwrap();
    assert!(restored.consume_query_on(20_002, &quota).is_err());
    assert_eq!(restored.usage().queries_today, 3);

    let unlimited = UsageMeter::new();
    for _ in 0..1_000 {
        unlimited.consume_query_on(1, &Quota::unlimited()).unwrap();
    }
}

#[test]
fn storage_is_reserved_only_when_every_check_passes() {
    let quota = free_tier();
    let meter = UsageMeter::new();
    let too_big = meter
        .add_file(6 * 1024 * 1024, "big.xlsx", &quota)
        .unwrap_err();
    assert!(
        matches!(too_big, QuotaExceeded::BytesPerFile { .. }),
        "{too_big}"
    );
    assert_eq!(meter.usage().files, 0);

    meter.add_file(4 * 1024 * 1024, "a.xlsx", &quota).unwrap();
    let second = meter.add_file(1024, "b.csv", &quota).unwrap_err();
    assert_eq!(second, QuotaExceeded::Files { limit: 1, used: 1 });
    assert_eq!(meter.usage().bytes_stored, 4 * 1024 * 1024);

    meter.remove_file(4 * 1024 * 1024);
    assert_eq!(meter.usage().bytes_stored, 0);
    assert_eq!(meter.usage().files, 0);

    let roomy = Quota {
        files: None,
        bytes_per_file: None,
        ..quota
    };
    meter.add_file(3 * 1024 * 1024, "a.csv", &roomy).unwrap();
    let full = meter
        .add_file(3 * 1024 * 1024, "b.csv", &roomy)
        .unwrap_err();
    assert!(
        matches!(full, QuotaExceeded::BytesStored { used, requested, .. } if used == 3 * 1024 * 1024 && requested == 3 * 1024 * 1024),
        "{full}"
    );
    assert!(full.to_string().contains("delete a file first"));

    let restarted = UsageMeter::with_storage(7, 2);
    assert_eq!(restarted.usage().files, 2);
    assert_eq!(restarted.usage().bytes_stored, 7);
}

#[test]
fn rows_per_file_flows_into_ingest_options() {
    let quota = free_tier();
    let opts = quota.ingest_options(IngestOptions::default());
    assert_eq!(opts.max_rows, 100_000);
    let tighter = quota.ingest_options(IngestOptions {
        max_rows: 10,
        ..IngestOptions::default()
    });
    assert_eq!(tighter.max_rows, 10);
    assert!(quota.check_rows(100_001, "x.csv").is_err());
    assert!(quota.check_rows(100_000, "x.csv").is_ok());
    assert_eq!(
        Quota::unlimited()
            .ingest_options(IngestOptions::default())
            .max_rows,
        IngestOptions::default().max_rows
    );
}

#[tokio::test]
async fn metered_state_refuses_the_query_after_the_allowance() {
    let ctx = SessionContext::new();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample.csv");
    let tables = register_path(&ctx, &fixture, &IngestOptions::default())
        .await
        .unwrap();
    let quota = Quota {
        queries_per_day: Some(2),
        ..Quota::unlimited()
    };
    let meter = Arc::new(UsageMeter::new());
    let state = AppState::new(Arc::new(ctx), tables).with_quota(quota, meter.clone());

    let q = |sql: &str| call_tool(&state, "query_sql", serde_json::json!({ "sql": sql }));
    q("SELECT 1").await.unwrap();
    call_tool(&state, "get_schema", serde_json::json!({}))
        .await
        .unwrap(); // plans SQL: counts
    let err = q("SELECT 2").await.unwrap_err();
    assert!(err.starts_with("quota: 2 of 2 queries used today"), "{err}");
    // list_tables plans nothing and stays available.
    call_tool(&state, "list_tables", serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(meter.usage().queries_today, 2);
}
