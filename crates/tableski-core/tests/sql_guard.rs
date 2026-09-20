//! Untrusted-SQL guard: decisions on the parsed statement, then the same through the
//! tools of an untrusted `AppState`, and proof that trusted mode is unchanged.
use datafusion::prelude::SessionContext;
use std::path::PathBuf;
use std::sync::Arc;
use tableski_core::{
    AppState, IngestOptions, Rejection, call_tool, check_untrusted, register_path,
};

fn tables() -> Vec<String> {
    vec!["people".into(), "orders".into()]
}

fn rejection(sql: &str) -> Rejection {
    check_untrusted(sql, &tables()).expect_err(sql)
}

#[test]
fn read_only_queries_over_registered_tables_pass() {
    for sql in [
        "SELECT * FROM people",
        "select name, count(*) from People p join orders o on o.name = p.name group by 1",
        "WITH big AS (SELECT * FROM orders WHERE amount > 10) SELECT * FROM big JOIN people USING (name)",
        "SELECT * FROM people WHERE name IN (SELECT name FROM orders)",
        "SELECT name FROM people UNION ALL SELECT name FROM orders",
        "SELECT * FROM (SELECT * FROM orders) AS o",
        "EXPLAIN SELECT * FROM people",
        "SELECT 1 + 1",
        "VALUES (1, 'a')",
        "SELECT * FROM \"people\"",
    ] {
        check_untrusted(sql, &tables()).unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
}

#[test]
fn ddl_dml_and_statements_are_not_queries() {
    assert!(
        matches!(rejection("CREATE TABLE t (a INT)"), Rejection::NotAQuery(k) if k == "CREATE TABLE")
    );
    assert!(matches!(
        rejection("CREATE TABLE t AS SELECT * FROM people"),
        Rejection::NotAQuery(_)
    ));
    assert!(matches!(
        rejection("CREATE EXTERNAL TABLE x STORED AS CSV LOCATION '/etc/passwd'"),
        Rejection::NotAQuery(k) if k == "CREATE EXTERNAL TABLE"
    ));
    assert!(
        matches!(rejection("INSERT INTO people VALUES ('x', 1, true, '2024-01-01')"), Rejection::NotAQuery(k) if k == "INSERT INTO")
    );
    assert!(matches!(
        rejection("UPDATE people SET name = 'x'"),
        Rejection::NotAQuery(_)
    ));
    assert!(matches!(
        rejection("DELETE FROM people"),
        Rejection::NotAQuery(_)
    ));
    assert!(matches!(rejection("DROP TABLE people"), Rejection::NotAQuery(k) if k == "DROP TABLE"));
    assert!(
        matches!(rejection("COPY (SELECT * FROM people) TO '/tmp/out.csv'"), Rejection::NotAQuery(k) if k == "COPY")
    );
    assert!(matches!(
        rejection("SET datafusion.execution.batch_size = 1"),
        Rejection::NotAQuery(_)
    ));
    assert!(matches!(rejection("SHOW TABLES"), Rejection::NotAQuery(_)));
    assert!(matches!(
        rejection("EXPLAIN DROP TABLE people"),
        Rejection::NotAQuery(_)
    ));
}

#[test]
fn one_statement_per_request() {
    assert_eq!(
        rejection("SELECT 1; SELECT 2"),
        Rejection::MultipleStatements(2)
    );
    assert_eq!(
        rejection("SELECT * FROM people; DROP TABLE people"),
        Rejection::MultipleStatements(2)
    );
    assert_eq!(rejection("   "), Rejection::Empty);
    assert!(matches!(
        rejection("SELECT * FROM people WHERE ("),
        Rejection::Unparsable(_)
    ));
}

#[test]
fn table_functions_and_unknown_tables_are_rejected_everywhere_in_the_query() {
    assert!(
        matches!(rejection("SELECT * FROM read_csv('/etc/passwd')"), Rejection::TableFunction(f) if f == "read_csv")
    );
    assert!(matches!(
        rejection("SELECT * FROM range(1, 10)"),
        Rejection::TableFunction(_)
    ));
    assert!(matches!(
        rejection("SELECT * FROM people WHERE name IN (SELECT name FROM read_parquet('x.parquet'))"),
        Rejection::TableFunction(f) if f == "read_parquet"
    ));
    assert!(matches!(
        rejection("WITH x AS (SELECT * FROM generate_series(1, 3)) SELECT * FROM x"),
        Rejection::TableFunction(_)
    ));
    assert!(matches!(
        rejection("SELECT * FROM 'data.parquet'"),
        Rejection::UnknownTable(_)
    ));
    assert!(matches!(rejection("SELECT * FROM nope"), Rejection::UnknownTable(t) if t == "nope"));
    assert!(matches!(
        rejection("SELECT * FROM information_schema.tables"),
        Rejection::UnknownTable(_)
    ));
    assert!(
        matches!(rejection("SELECT * FROM people p JOIN secrets s ON true"), Rejection::UnknownTable(t) if t == "secrets")
    );
    // Quoted identifiers are case-sensitive; unquoted ones are not.
    assert!(matches!(
        rejection("SELECT * FROM \"People\""),
        Rejection::UnknownTable(_)
    ));
}

#[test]
fn rejection_messages_name_the_reason() {
    assert!(
        rejection("DROP TABLE people")
            .to_string()
            .contains("`DROP TABLE` is not allowed")
    );
    assert!(
        rejection("SELECT * FROM range(3)")
            .to_string()
            .contains("table function `range`")
    );
    assert!(
        rejection("SELECT * FROM nope")
            .to_string()
            .contains("`nope` is not a registered table")
    );
}

async fn state(untrusted: bool) -> AppState {
    let ctx = SessionContext::new();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample.csv");
    let tables = register_path(&ctx, &fixture, &IngestOptions::default())
        .await
        .unwrap();
    let state = AppState::new(Arc::new(ctx), tables);
    if untrusted { state.untrusted() } else { state }
}

#[tokio::test]
async fn untrusted_state_rejects_before_execution_and_still_serves_queries() {
    let s = state(true).await;
    let err = call_tool(
        &s,
        "query_sql",
        serde_json::json!({ "sql": "CREATE TABLE t AS SELECT 1" }),
    )
    .await
    .unwrap_err();
    assert!(err.starts_with("rejected:"), "{err}");
    // Nothing was executed: `t` does not exist afterwards, even for a trusted look.
    assert!(s.ctx.sql("SELECT * FROM t").await.is_err());

    let err = call_tool(
        &s,
        "query_sql",
        serde_json::json!({ "sql": "SELECT * FROM range(5)" }),
    )
    .await
    .unwrap_err();
    assert!(err.contains("table function"), "{err}");

    let grid = call_tool(
        &s,
        "query_sql",
        serde_json::json!({ "sql": "SELECT count(*) AS n FROM sample" }),
    )
    .await
    .unwrap();
    assert!(grid.contains("| n"), "{grid}");
    let plan = call_tool(
        &s,
        "query_sql",
        serde_json::json!({ "sql": "EXPLAIN SELECT * FROM sample" }),
    )
    .await
    .unwrap();
    assert!(plan.contains("plan"), "{plan}");
    // Tools that build their own SQL keep working.
    call_tool(&s, "get_schema", serde_json::json!({}))
        .await
        .unwrap();
    call_tool(&s, "column_statistics", serde_json::json!({}))
        .await
        .unwrap();
}

#[tokio::test]
async fn untrusted_state_refuses_cartesian_products_on_the_plan() {
    let s = state(true).await;
    for sql in [
        "SELECT count(*) FROM sample a CROSS JOIN sample b",
        "SELECT count(*) FROM sample a, sample b WHERE a.name <> b.name",
        "SELECT count(*) FROM sample a JOIN sample b ON a.name <> b.name",
    ] {
        let err = call_tool(&s, "query_sql", serde_json::json!({ "sql": sql }))
            .await
            .unwrap_err();
        assert!(err.contains("cartesian product"), "{sql}: {err}");
    }
    // An equi-join is fine.
    call_tool(&s, "query_sql", serde_json::json!({ "sql": "SELECT count(*) FROM sample a JOIN sample b ON a.name = b.name" }))
        .await
        .unwrap();
}

#[tokio::test]
async fn trusted_state_is_unchanged() {
    let s = state(false).await;
    call_tool(
        &s,
        "query_sql",
        serde_json::json!({ "sql": "CREATE TABLE t AS SELECT 1 AS one" }),
    )
    .await
    .unwrap();
    let grid = call_tool(
        &s,
        "query_sql",
        serde_json::json!({ "sql": "SELECT * FROM t" }),
    )
    .await
    .unwrap();
    assert!(grid.contains("one"), "{grid}");
    call_tool(
        &s,
        "query_sql",
        serde_json::json!({ "sql": "SELECT * FROM range(3)" }),
    )
    .await
    .unwrap();
}
