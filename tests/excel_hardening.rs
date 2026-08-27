//! Hardening tests over the COMMITTED corpus (`fixtures/corpus/`): every nasty real-world
//! workbook trait has a fixture and an assertion; failures are structured errors
//! (Emperor Profile E10), never panics.

use datafusion::arrow::util::pretty::pretty_format_batches;
use datafusion::prelude::*;
use std::path::PathBuf;
use tableski::{HeaderMode, IngestOptions, register_workbook};

fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/corpus")
        .join(name)
}

async fn pretty(ctx: &SessionContext, sql: &str) -> String {
    let batches = ctx.sql(sql).await.unwrap().collect().await.unwrap();
    pretty_format_batches(&batches).unwrap().to_string()
}

#[tokio::test]
async fn date_system_1904_and_cached_formulas_and_error_cells() {
    let ctx = SessionContext::new();
    let infos = register_workbook(&ctx, &corpus("nasty_1904.xlsx"), &IngestOptions::default())
        .expect("register 1904 workbook");
    assert_eq!(infos[0].table, "nasty");

    // Serial 100 in the 1904 date system is 1904-04-10 (1904 IS a leap year);
    // in the 1900 system it would read 1900-04-09 — this asserts date1904 is honored.
    let out = pretty(
        &ctx,
        "SELECT wh.when FROM nasty wh ORDER BY wh.when LIMIT 1",
    )
    .await;
    assert!(
        out.contains("1904-04-10"),
        "expected 1904-system date, got:\n{out}"
    );

    // Formula cells ingest their cached numeric values.
    let out = pretty(&ctx, "SELECT SUM(calc) AS s FROM nasty").await;
    assert!(out.contains('6'), "cached formula values 2+4, got:\n{out}");

    // Error cells (#DIV/0!) become NULL, and the column doesn't poison type inference.
    let out = pretty(
        &ctx,
        "SELECT COUNT(*) AS total, COUNT(broken) AS non_null FROM nasty",
    )
    .await;
    assert!(
        out.contains('2') && out.contains('0'),
        "errors → NULL:\n{out}"
    );
}

#[tokio::test]
async fn merged_cells_keep_top_left_value_and_nulls() {
    let ctx = SessionContext::new();
    register_workbook(&ctx, &corpus("merged.xlsx"), &IngestOptions::default()).unwrap();
    let out = pretty(&ctx, "SELECT a, b, c FROM merged ORDER BY a").await;
    assert!(out.contains("wide value"), "{out}");
    // The merged row's continuation cell is NULL, not a duplicated value.
    let nulls = pretty(
        &ctx,
        "SELECT COUNT(*) FROM merged WHERE b = 'wide value' AND c IS NULL",
    )
    .await;
    assert!(
        nulls.contains('1'),
        "top-left value + NULL continuation:\n{nulls}"
    );
}

#[tokio::test]
async fn display_formats_do_not_change_raw_values() {
    let ctx = SessionContext::new();
    register_workbook(&ctx, &corpus("formats.xlsx"), &IngestOptions::default()).unwrap();
    let out = pretty(&ctx, "SELECT price, growth FROM formats").await;
    assert!(out.contains("1234.5"), "currency format reads raw: {out}");
    assert!(
        out.contains("0.42"),
        "percent format reads raw fraction: {out}"
    );
}

#[tokio::test]
async fn ragged_rows_pad_with_nulls() {
    let ctx = SessionContext::new();
    register_workbook(&ctx, &corpus("ragged.xlsx"), &IngestOptions::default()).unwrap();
    let out = pretty(
        &ctx,
        "SELECT COUNT(*) AS rows, COUNT(b) AS b_vals, COUNT(c) AS c_vals FROM ragged",
    )
    .await;
    // 2 data rows; only the full row has b/c values.
    assert!(out.contains('2') && out.contains('1'), "{out}");
}

#[tokio::test]
async fn unicode_and_symbol_sheet_names_get_safe_unique_tables() {
    let ctx = SessionContext::new();
    let infos =
        register_workbook(&ctx, &corpus("unicode.xlsx"), &IngestOptions::default()).unwrap();
    let names: Vec<_> = infos.iter().map(|i| i.table.as_str()).collect();
    // All-non-ascii names degrade to the `sheet` fallback and stay unique; SQL works.
    assert_eq!(names.len(), 3);
    assert!(names.iter().all(|n| !n.is_empty()));
    let unique: std::collections::HashSet<_> = names.iter().collect();
    assert_eq!(unique.len(), 3, "table names must be unique: {names:?}");
    for n in &names {
        let out = pretty(&ctx, &format!("SELECT COUNT(*) FROM {n}")).await;
        assert!(out.contains('1'), "{n}: {out}");
    }
}

#[tokio::test]
async fn format_only_trailing_cells_are_trimmed() {
    let ctx = SessionContext::new();
    let infos = register_workbook(&ctx, &corpus("padded.xlsx"), &IngestOptions::default()).unwrap();
    let padded = &infos[0];
    // The blank formatted cell at (10, 5) widened calamine's range; ingestion trims to 1x1.
    assert_eq!((padded.rows, padded.columns), (1, 1), "{padded:?}");
    let out = pretty(&ctx, "SELECT n FROM padded").await;
    assert!(out.contains('7'), "{out}");
}

#[tokio::test]
async fn row_cap_is_a_clear_error_not_a_silent_cut() {
    let ctx = SessionContext::new();
    let err = register_workbook(
        &ctx,
        &corpus("rowcap.xlsx"),
        &IngestOptions {
            headers: HeaderMode::Auto,
            max_rows: 10,
        },
    )
    .unwrap_err();
    assert!(
        err.contains("20 data rows") && err.contains("row cap") && err.contains("max-rows"),
        "clear actionable error, got: {err}"
    );
    // Within the cap it ingests fully.
    let ctx = SessionContext::new();
    let infos = register_workbook(
        &ctx,
        &corpus("rowcap.xlsx"),
        &IngestOptions {
            headers: HeaderMode::Auto,
            max_rows: 20,
        },
    )
    .unwrap();
    assert_eq!(infos[0].rows, 20);
}

#[tokio::test]
async fn garbage_file_fails_with_clean_error_never_panics() {
    let ctx = SessionContext::new();
    let err = register_workbook(
        &ctx,
        &corpus("not_a_workbook.xlsx"),
        &IngestOptions::default(),
    )
    .unwrap_err();
    assert!(
        err.contains("cannot open workbook"),
        "structured error, got: {err}"
    );
}
