//! `TableSource`: the file source behaves like before, and a test-only in-memory
//! source that a feed appends to shows the next query seeing the new rows, with the server
//! side untouched. This is the shape a live-bars source will take.
use async_trait::async_trait;
use datafusion::arrow::array::{Float64Array, Int64Array};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;
use datafusion::catalog::{Session, TableProvider};
use datafusion::datasource::TableType;
use datafusion::datasource::memory::MemorySourceConfig;
use datafusion::error::Result as DfResult;
use datafusion::logical_expr::Expr;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::prelude::SessionContext;
use futures::future::BoxFuture;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tableski_core::{
    AppState, FileSource, IngestOptions, TableEntry, TableSource, call_tool, register_sources,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

#[tokio::test]
async fn file_source_registers_like_the_flags_did() {
    let ctx = SessionContext::new();
    let files = FileSource::new()
        .csv_named(fixture("sample.csv"), "named")
        .workbook(fixture("sample.xlsx"))
        .path(fixture("sample.parquet"));
    assert_eq!(files.len(), 3);
    let sources: Vec<Box<dyn TableSource>> = vec![Box::new(files)];
    let tables = register_sources(&ctx, &sources, &IngestOptions::default())
        .await
        .unwrap();
    let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"named"), "{names:?}");
    assert!(names.contains(&"people"), "{names:?}");
    assert!(names.contains(&"sample"), "{names:?}");
    let missing: Vec<Box<dyn TableSource>> = vec![Box::new(FileSource::new().path("/nope.csv"))];
    let err = register_sources(&ctx, &missing, &IngestOptions::default())
        .await
        .unwrap_err();
    assert!(err.starts_with("files source: file not found"), "{err}");
}

/// Rows live on the source's side; `scan` snapshots them. A feed appends between queries.
#[derive(Debug)]
struct Bars {
    schema: SchemaRef,
    batches: Arc<RwLock<Vec<RecordBatch>>>,
}

impl Bars {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            schema: Arc::new(Schema::new(vec![
                Field::new("ts", DataType::Int64, false),
                Field::new("close", DataType::Float64, false),
            ])),
            batches: Arc::default(),
        })
    }

    fn push(&self, bars: &[(i64, f64)]) {
        let batch = RecordBatch::try_new(
            self.schema.clone(),
            vec![
                Arc::new(Int64Array::from_iter_values(bars.iter().map(|b| b.0))),
                Arc::new(Float64Array::from_iter_values(bars.iter().map(|b| b.1))),
            ],
        )
        .unwrap();
        self.batches.write().unwrap().push(batch);
    }
}

#[async_trait]
impl TableProvider for Bars {
    fn schema(&self) -> SchemaRef {
        self.schema.clone()
    }
    fn table_type(&self) -> TableType {
        TableType::Base
    }
    async fn scan(
        &self,
        _state: &dyn Session,
        projection: Option<&Vec<usize>>,
        _filters: &[Expr],
        _limit: Option<usize>,
    ) -> DfResult<Arc<dyn ExecutionPlan>> {
        let snapshot = self.batches.read().unwrap().clone();
        Ok(MemorySourceConfig::try_new_exec(
            &[snapshot],
            self.schema.clone(),
            projection.cloned(),
        )?)
    }
}

/// The source: registers the provider once; the feed keeps the `Arc<Bars>` and pushes.
struct LiveBars {
    table: String,
    bars: Arc<Bars>,
}

impl TableSource for LiveBars {
    fn kind(&self) -> &'static str {
        "live-bars"
    }
    fn register<'a>(
        &'a self,
        ctx: &'a SessionContext,
        _opts: &'a IngestOptions,
    ) -> BoxFuture<'a, Result<Vec<TableEntry>, String>> {
        Box::pin(async move {
            ctx.register_table(&self.table, self.bars.clone())
                .map_err(|e| e.to_string())?;
            Ok(vec![TableEntry::csv(&self.table, "feed://bars")])
        })
    }
}

#[tokio::test]
async fn appending_source_is_visible_to_the_next_query_without_re_registration() {
    let bars = Bars::new();
    bars.push(&[(1, 10.0), (2, 11.0), (3, 12.0)]);
    let ctx = SessionContext::new();
    let sources: Vec<Box<dyn TableSource>> = vec![
        Box::new(FileSource::new().path(fixture("sample.csv"))),
        Box::new(LiveBars {
            table: "bars".into(),
            bars: bars.clone(),
        }),
    ];
    let tables = register_sources(&ctx, &sources, &IngestOptions::default())
        .await
        .unwrap();
    assert_eq!(tables.len(), 2);
    let state = AppState::new(Arc::new(ctx), tables).untrusted();

    let q = |sql: &str| call_tool(&state, "query_sql", serde_json::json!({ "sql": sql }));
    let first = q("SELECT count(*) AS n, max(close) AS hi FROM bars")
        .await
        .unwrap();
    assert!(first.contains("| 3 "), "{first}");
    assert!(first.contains("12.0"), "{first}");

    // The feed appends; the registry is untouched.
    bars.push(&[(4, 13.5), (5, 9.0)]);
    let second = q("SELECT count(*) AS n, max(close) AS hi FROM bars")
        .await
        .unwrap();
    assert!(second.contains("| 5 "), "{second}");
    assert!(second.contains("13.5"), "{second}");

    // Joins across a file and a feed work like any two tables.
    let joined = q("SELECT count(*) FROM bars b JOIN sample s ON s.id = b.ts")
        .await
        .unwrap();
    assert!(joined.contains("| 3 "), "{joined}");
}
