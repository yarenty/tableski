//! Untrusted-SQL guard: decide on the **parsed** statement whether a client that we do not
//! trust may run it. Read-only queries over registered tables pass; everything else is
//! rejected before DataFusion sees it.
//!
//! Two layers apply in [`SqlTrust::Untrusted`] mode: this module's AST check (one statement,
//! `SELECT`/`WITH`/`VALUES`/`EXPLAIN` only, no table functions, every table reference
//! registered or a CTE), then DataFusion's own [`SQLOptions`] with DDL, DML and
//! statements disabled as a second fence.
//!
//! [`SQLOptions`]: datafusion::execution::context::SQLOptions

use datafusion::physical_plan::ExecutionPlan;
use datafusion::physical_plan::joins::{CrossJoinExec, NestedLoopJoinExec};
use datafusion::sql::parser::{DFParser, Statement as DfStatement};
use datafusion::sql::sqlparser::ast::{
    ObjectName, ObjectNamePart, Query, Statement, TableFactor, Visit, Visitor,
};
use std::fmt;
use std::ops::ControlFlow;
use std::sync::Arc;

/// How much the server trusts the client sending SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SqlTrust {
    /// Anything DataFusion accepts runs (local single-user CLI, the default).
    #[default]
    Trusted,
    /// Only read-only queries over registered tables run; see [`check_untrusted`].
    Untrusted,
}

/// Why a statement was refused in untrusted mode. The `Display` text is what the client sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    /// The text did not parse as SQL.
    Unparsable(String),
    /// Nothing to run.
    Empty,
    /// More than one statement in one request.
    MultipleStatements(usize),
    /// A statement kind other than a query (`CREATE`, `INSERT`, `COPY`, `SET`, ...).
    NotAQuery(String),
    /// A table function such as `read_csv(...)`, `range(...)`.
    TableFunction(String),
    /// A table reference that is neither registered nor a CTE of the same query.
    UnknownTable(String),
    /// The plan contains a cartesian product (cross join, or a join without an equality
    /// condition), which cannot be bounded by time once it runs.
    CartesianJoin,
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unparsable(e) => write!(f, "rejected: not valid SQL ({e})"),
            Self::Empty => write!(f, "rejected: empty statement"),
            Self::MultipleStatements(n) => {
                write!(
                    f,
                    "rejected: {n} statements in one request; send one query at a time"
                )
            }
            Self::NotAQuery(kind) => write!(
                f,
                "rejected: `{kind}` is not allowed for untrusted clients; only read-only queries (SELECT / WITH / EXPLAIN) over registered tables run here"
            ),
            Self::TableFunction(name) => write!(
                f,
                "rejected: table function `{name}` is not allowed for untrusted clients; query registered tables only (see list_tables)"
            ),
            Self::UnknownTable(name) => write!(
                f,
                "rejected: `{name}` is not a registered table (see list_tables)"
            ),
            Self::CartesianJoin => write!(
                f,
                "rejected: cartesian product (cross join, or a join without an equality condition) is not allowed for untrusted clients; join on a column with `=`"
            ),
        }
    }
}

impl std::error::Error for Rejection {}

/// Parse `sql` and decide whether an untrusted client may run it against `registered` tables.
///
/// Returns the single parsed statement on success so the caller can hand the same text to
/// DataFusion. Decisions are made on the AST, never on the SQL text.
pub fn check_untrusted(sql: &str, registered: &[String]) -> Result<DfStatement, Rejection> {
    let mut statements =
        DFParser::parse_sql(sql).map_err(|e| Rejection::Unparsable(e.to_string()))?;
    match statements.len() {
        0 => return Err(Rejection::Empty),
        1 => {}
        n => return Err(Rejection::MultipleStatements(n)),
    }
    let statement = statements.pop_front().expect("one statement");
    check_statement(&statement, registered)?;
    Ok(statement)
}

fn check_statement(statement: &DfStatement, registered: &[String]) -> Result<(), Rejection> {
    match statement {
        DfStatement::Statement(inner) => check_sql_statement(inner, registered),
        DfStatement::Explain(explain) => check_statement(&explain.statement, registered),
        DfStatement::CreateExternalTable(_) => {
            Err(Rejection::NotAQuery("CREATE EXTERNAL TABLE".into()))
        }
        DfStatement::CopyTo(_) => Err(Rejection::NotAQuery("COPY".into())),
        DfStatement::Reset(_) => Err(Rejection::NotAQuery("RESET".into())),
    }
}

fn check_sql_statement(statement: &Statement, registered: &[String]) -> Result<(), Rejection> {
    match statement {
        Statement::Query(query) => check_query(query, registered),
        Statement::Explain { statement, .. } => check_sql_statement(statement, registered),
        other => Err(Rejection::NotAQuery(statement_kind(other))),
    }
}

/// First keywords of the statement, for the rejection message (`CREATE TABLE`, `INSERT INTO`).
fn statement_kind(statement: &Statement) -> String {
    statement
        .to_string()
        .split_whitespace()
        .take(2)
        .map(|w| w.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(" ")
}

fn check_query(query: &Query, registered: &[String]) -> Result<(), Rejection> {
    let mut visitor = TableVisitor {
        registered,
        ctes: Vec::new(),
    };
    match query.visit(&mut visitor) {
        ControlFlow::Continue(()) => Ok(()),
        ControlFlow::Break(rejection) => Err(rejection),
    }
}

/// Walks every table factor of a query, including subqueries in expressions and CTE bodies.
struct TableVisitor<'a> {
    registered: &'a [String],
    /// CTE names seen so far (pre-order, so a CTE is known before its uses).
    ctes: Vec<String>,
}

impl TableVisitor<'_> {
    fn is_known(&self, name: &ObjectName) -> bool {
        let Some(ObjectNamePart::Identifier(ident)) = name.0.last() else {
            return false;
        };
        let value = ident.value.as_str();
        let unquoted = ident.quote_style.is_none();
        let matches = |candidate: &str| {
            candidate == value || (unquoted && candidate.eq_ignore_ascii_case(value))
        };
        // Schema-qualified names (`public.people`) are not how tables are registered here.
        name.0.len() == 1
            && (self.registered.iter().any(|r| matches(r)) || self.ctes.iter().any(|c| matches(c)))
    }
}

impl Visitor for TableVisitor<'_> {
    type Break = Rejection;

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<Self::Break> {
        if let Some(with) = &query.with {
            self.ctes.extend(
                with.cte_tables
                    .iter()
                    .map(|cte| cte.alias.name.value.clone()),
            );
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, table_factor: &TableFactor) -> ControlFlow<Self::Break> {
        match table_factor {
            TableFactor::Table {
                name,
                args: Some(_),
                ..
            } => ControlFlow::Break(Rejection::TableFunction(name.to_string())),
            TableFactor::Function { name, .. } => {
                ControlFlow::Break(Rejection::TableFunction(name.to_string()))
            }
            TableFactor::Table { name, .. } if !self.is_known(name) => {
                ControlFlow::Break(Rejection::UnknownTable(name.to_string()))
            }
            _ => ControlFlow::Continue(()),
        }
    }
}

/// Second decision, on the physical plan: refuse cartesian products. A cross join or a
/// nested-loop join over registered tables is the one query shape a wall-clock limit cannot
/// stop once it runs (DataFusion yields too rarely inside it), so it never starts.
pub fn check_plan(plan: &Arc<dyn ExecutionPlan>) -> Result<(), Rejection> {
    let any: &dyn std::any::Any = plan.as_ref();
    if any.is::<CrossJoinExec>() || any.is::<NestedLoopJoinExec>() {
        return Err(Rejection::CartesianJoin);
    }
    plan.children().into_iter().try_for_each(check_plan)
}
