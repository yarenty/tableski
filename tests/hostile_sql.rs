//! Hostile-SQL suite: bad SQL against a guarded, limited server built from the fixture
//! corpus. Every case must be rejected or bounded within the limits, quickly, and the server
//! must answer the next query. Data-driven: add a line to `CASES` (or `EXPORT_CASES`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tableski::{AppState, IngestOptions, QueryLimits, call_tool, register_path};

/// What a hostile statement is allowed to produce.
#[derive(Debug, Clone, Copy)]
enum Expect {
    /// Refused before execution; the error contains this text.
    Rejected(&'static str),
    /// May run, but comes back within the limits: either truncated, or a limit error.
    Bounded,
    /// A legitimate query that must keep working under the guard (control case).
    Works,
}
use Expect::*;

/// Per-case wall clock; the whole suite must stay well under a minute in CI.
const CASE_BUDGET: Duration = Duration::from_secs(10);

fn cases() -> Vec<(&'static str, String, Expect)> {
    let deep_parens = format!("SELECT {}1{}", "(".repeat(2_000), ")".repeat(2_000));
    let deep_subqueries = (0..300).fold("SELECT * FROM sample".to_string(), |q, _| {
        format!("SELECT * FROM ({q}) AS t")
    });
    let long_in_list = format!(
        "SELECT * FROM sample WHERE id IN ({})",
        (0..20_000)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    let megabyte_of_sql = format!(
        "SELECT * FROM sample WHERE name = '{}'",
        "x".repeat(1 << 20)
    );
    vec![
        // --- DDL / DML / statements
        (
            "create table",
            "CREATE TABLE t (a INT)".into(),
            Rejected("CREATE TABLE"),
        ),
        (
            "ctas",
            "CREATE TABLE t AS SELECT * FROM sample".into(),
            Rejected("not allowed"),
        ),
        (
            "create external table",
            "CREATE EXTERNAL TABLE pw STORED AS CSV LOCATION '/etc/passwd'".into(),
            Rejected("CREATE EXTERNAL TABLE"),
        ),
        (
            "create view",
            "CREATE VIEW v AS SELECT * FROM sample".into(),
            Rejected("not allowed"),
        ),
        (
            "insert",
            "INSERT INTO sample VALUES (9, 'x', 1.0, true)".into(),
            Rejected("INSERT"),
        ),
        (
            "update",
            "UPDATE sample SET name = 'x'".into(),
            Rejected("not allowed"),
        ),
        (
            "delete",
            "DELETE FROM sample".into(),
            Rejected("not allowed"),
        ),
        ("drop", "DROP TABLE sample".into(), Rejected("DROP TABLE")),
        (
            "copy to file",
            "COPY (SELECT * FROM sample) TO '/tmp/hostile.csv'".into(),
            Rejected("COPY"),
        ),
        (
            "set",
            "SET datafusion.execution.batch_size = 1".into(),
            Rejected("not allowed"),
        ),
        (
            "reset",
            "RESET datafusion.execution.batch_size".into(),
            Rejected("RESET"),
        ),
        ("show tables", "SHOW TABLES".into(), Rejected("not allowed")),
        (
            "explain of ddl",
            "EXPLAIN CREATE TABLE t (a INT)".into(),
            Rejected("not allowed"),
        ),
        (
            "two statements",
            "SELECT 1; DROP TABLE sample".into(),
            Rejected("statements in one request"),
        ),
        (
            "comment then statement",
            "SELECT * FROM sample /* still one */ ; DROP TABLE sample -- x".into(),
            Rejected("statements in one request"),
        ),
        // --- filesystem and catalog reach
        (
            "read_csv",
            "SELECT * FROM read_csv('/etc/passwd')".into(),
            Rejected("table function"),
        ),
        (
            "read_parquet in subquery",
            "SELECT * FROM sample WHERE id IN (SELECT 1 FROM read_parquet('x.parquet'))".into(),
            Rejected("table function"),
        ),
        (
            "range table function",
            "SELECT * FROM range(1, 1000000000)".into(),
            Rejected("table function"),
        ),
        (
            "generate_series in cte",
            "WITH g AS (SELECT * FROM generate_series(1, 1000000000)) SELECT count(*) FROM g"
                .into(),
            Rejected("table function"),
        ),
        (
            "path as table",
            "SELECT * FROM '/etc/passwd'".into(),
            Rejected("not a registered table"),
        ),
        (
            "url as table",
            "SELECT * FROM 'https://example.com/x.csv'".into(),
            Rejected("not a registered table"),
        ),
        (
            "information_schema",
            "SELECT * FROM information_schema.tables".into(),
            Rejected("not a registered table"),
        ),
        (
            "catalog-qualified",
            "SELECT * FROM datafusion.public.sample".into(),
            Rejected("not a registered table"),
        ),
        // --- identifier tricks
        (
            "cyrillic homoglyph table",
            "SELECT * FROM ѕample".into(),
            Rejected("not a registered table"),
        ),
        (
            "quoted wrong case",
            "SELECT * FROM \"Sample\"".into(),
            Rejected("not a registered table"),
        ),
        ("unquoted any case", "SELECT * FROM SAMPLE".into(), Works),
        (
            "null byte",
            "SELECT * FROM sample\0 WHERE id = 1".into(),
            Rejected("rejected"),
        ),
        (
            "unicode whitespace",
            "SELECT\u{2003}*\u{a0}FROM sample".into(),
            Works,
        ),
        // --- blow-ups
        (
            "cross join",
            "SELECT count(*) FROM sample a CROSS JOIN sample b".into(),
            Rejected("cartesian product"),
        ),
        (
            "implicit cross join",
            "SELECT count(*) FROM sample a, sample b".into(),
            Rejected("cartesian product"),
        ),
        (
            "inequality join",
            "SELECT count(*) FROM big a JOIN big b ON a.value < b.value".into(),
            Rejected("cartesian product"),
        ),
        (
            "hash join blow-up",
            "SELECT * FROM big a JOIN big b ON a.value % 2 = b.value % 2".into(),
            Bounded,
        ),
        ("huge result", "SELECT * FROM big".into(), Bounded),
        (
            "huge sort",
            "SELECT * FROM big ORDER BY value DESC".into(),
            Bounded,
        ),
        (
            "huge string",
            "SELECT repeat('x', 200000000) AS s FROM big LIMIT 3".into(),
            Bounded,
        ),
        (
            "recursive cte forever",
            "WITH RECURSIVE r AS (SELECT 1 AS n UNION ALL SELECT n + 1 FROM r) SELECT * FROM r"
                .into(),
            Bounded,
        ),
        (
            "wide union",
            (0..200)
                .map(|_| "SELECT * FROM sample")
                .collect::<Vec<_>>()
                .join(" UNION ALL "),
            Bounded,
        ),
        (
            "twelve-way self join",
            format!(
                "SELECT count(*) FROM sample s0 {}",
                (1..12)
                    .map(|i| format!("JOIN sample s{i} ON s{i}.id = s0.id"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            Works,
        ),
        // --- parser and planner stress
        ("2000 nested parentheses", deep_parens, Bounded),
        ("300 nested subqueries", deep_subqueries, Bounded),
        ("20k-item IN list", long_in_list, Bounded),
        ("1 MiB of SQL", megabyte_of_sql, Bounded),
        ("empty", "".into(), Rejected("empty")),
        (
            "garbage",
            "SELECT * FROM sample WHERE (".into(),
            Rejected("not valid SQL"),
        ),
        // --- controls: the guard must not break honest work
        (
            "join across formats",
            "SELECT count(*) FROM sample s JOIN people p ON p.name = s.name".into(),
            Works,
        ),
        (
            "aggregate",
            "SELECT active, count(*), avg(score) FROM sample GROUP BY active ORDER BY 1".into(),
            Works,
        ),
        (
            "explain select",
            "EXPLAIN SELECT * FROM sample WHERE id > 1".into(),
            Works,
        ),
    ]
}

/// `export_result` file names: `true` = must be refused, `false` = may be written but only
/// inside the export dir (a literal `~` or `C:` is just an odd folder name on this side).
const EXPORT_CASES: &[(&str, &str, bool)] = &[
    ("parent traversal", "../escape.csv", true),
    ("nested traversal", "reports/../../escape.csv", true),
    ("absolute path", "/tmp/escape.csv", true),
    ("wrong extension", "run.sh", true),
    ("double extension", "x.csv.exe", true),
    ("empty name", "", true),
    ("null byte", "ok.csv\0.exe", true),
    ("home path", "~/escape.csv", false),
    ("windows drive", "C:\\escape.csv", false),
];

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

/// Guarded, limited server over the fixtures plus a 2M-row in-memory table `big`.
async fn hostile_target() -> (AppState, PathBuf) {
    let limits = QueryLimits {
        memory_bytes: Some(64 * 1024 * 1024),
        timeout: Some(Duration::from_secs(5)),
        max_rows: Some(1_000),
        max_bytes: Some(1024 * 1024),
    };
    let ctx = limits.session_context().unwrap();
    let opts = IngestOptions::default();
    let mut tables = Vec::new();
    tables.extend(
        register_path(&ctx, &fixture("sample.csv"), &opts)
            .await
            .unwrap(),
    );
    tables.extend(
        register_path(&ctx, &fixture("sample.xlsx"), &opts)
            .await
            .unwrap(),
    );
    ctx.sql("CREATE TABLE big AS SELECT * FROM range(1, 2000000)")
        .await
        .unwrap()
        .collect()
        .await
        .unwrap();
    tables.push(tableski::TableEntry::csv("big", "memory"));
    let export_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hostile_exports");
    let state = AppState::new(Arc::new(ctx), tables)
        .with_export_dir(&export_dir)
        .with_limits(limits)
        .untrusted();
    assert!(
        state.tables.iter().any(|t| t.name == "people"),
        "sample.xlsx should register a `people` sheet: {:?}",
        state.tables.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
    (state, export_dir)
}

async fn alive(state: &AppState, after: &str) {
    let probe = call_tool(
        state,
        "query_sql",
        serde_json::json!({ "sql": "SELECT count(*) AS n FROM sample" }),
    );
    let text = tokio::time::timeout(CASE_BUDGET, probe)
        .await
        .unwrap_or_else(|_| panic!("server unresponsive after `{after}`"))
        .unwrap_or_else(|e| panic!("server broken after `{after}`: {e}"));
    assert!(text.contains("| n"), "after `{after}`: {text}");
}

#[tokio::test]
async fn every_hostile_statement_is_rejected_or_bounded() {
    let (state, _) = hostile_target().await;
    let suite_started = Instant::now();
    let mut failures = Vec::new();
    for (name, sql, expect) in cases() {
        let started = Instant::now();
        let outcome = tokio::time::timeout(
            CASE_BUDGET,
            call_tool(&state, "query_sql", serde_json::json!({ "sql": sql })),
        )
        .await;
        let elapsed = started.elapsed();
        let verdict = match (&outcome, expect) {
            (Err(_), _) => Err(format!("hung past {CASE_BUDGET:?}")),
            (Ok(Err(e)), Rejected(needle)) if e.contains(needle) => Ok("rejected"),
            (Ok(Err(e)), Rejected(needle)) => Err(format!(
                "rejected for another reason (wanted `{needle}`): {e}"
            )),
            (Ok(Ok(text)), Rejected(_)) => Err(format!("ran and returned {} bytes", text.len())),
            (Ok(Ok(text)), Bounded) if text.contains("result truncated") => Ok("truncated"),
            (Ok(Ok(text)), Bounded) if text.lines().count() <= 1_100 => Ok("completed within caps"),
            (Ok(Ok(text)), Bounded) => {
                Err(format!("unbounded result: {} lines", text.lines().count()))
            }
            (Ok(Err(e)), Bounded)
                if e.contains("Resources exhausted") || e.contains("cancelled") =>
            {
                Ok("limited")
            }
            (Ok(Err(_)), Bounded) => Ok("errored"),
            (Ok(Ok(_)), Works) => Ok("works"),
            (Ok(Err(e)), Works) => Err(format!("honest query refused: {e}")),
        };
        eprintln!(
            "{name:32} {elapsed:>8.2?}  {}",
            match &verdict {
                Ok(v) => v.to_string(),
                Err(e) => format!("FAIL: {e}"),
            }
        );
        if let Err(e) = verdict {
            failures.push(format!("{name}: {e}"));
        }
        alive(&state, name).await;
    }
    assert!(
        failures.is_empty(),
        "hostile cases failed:\n  {}",
        failures.join("\n  ")
    );
    assert!(
        suite_started.elapsed() < Duration::from_secs(60),
        "suite too slow: {:?}",
        suite_started.elapsed()
    );
}

#[tokio::test]
async fn export_never_leaves_the_export_dir() {
    let (state, export_dir) = hostile_target().await;
    for (name, file, must_refuse) in EXPORT_CASES {
        let result = call_tool(
            &state,
            "export_result",
            serde_json::json!({ "sql": "SELECT * FROM sample", "file": file }),
        )
        .await;
        match (result, must_refuse) {
            (Err(e), true) => assert!(e.contains("export file"), "{name}: {e}"),
            (Ok(text), true) => panic!("{name}: accepted: {text}"),
            (Ok(text), false) => assert!(
                text.contains(&export_dir.to_string_lossy().to_string()),
                "{name}: written outside the export dir: {text}"
            ),
            (Err(e), false) => {
                panic!("{name}: refused, expected a file inside the export dir: {e}")
            }
        }
    }
    let escaped: Vec<_> = std::fs::read_dir(export_dir.parent().unwrap())
        .map(|d| {
            d.flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("escape"))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        escaped.is_empty(),
        "files escaped the export dir: {escaped:?}"
    );
    alive(&state, "export cases").await;
}
