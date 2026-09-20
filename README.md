# tableski

[![CI](https://github.com/yarenty/tableski/actions/workflows/ci.yml/badge.svg)](https://github.com/yarenty/tableski/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/tableski.svg)](https://crates.io/crates/tableski)
[![docs.rs](https://docs.rs/tableski/badge.svg)](https://docs.rs/tableski)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**Every spreadsheet is a table — tableski serves them to your agents in SQL.**

Excel, CSV, Parquet, JSON — one penguin-family binary that turns your data files into
queryable tables and serves them to AI agents over the
[Model Context Protocol](https://modelcontextprotocol.io).

- Single binary, stateless Streamable HTTP — built on the
  [emperor-mcp](https://github.com/yarenty/emperor-mcp) framework (Emperor Profile P1
  conformant: framed output, forwarded credentials, no session affinity).
- SQL engine powered by [Apache DataFusion](https://datafusion.apache.org/).
- Hatched in the [kowalski](https://github.com/yarenty/kowalski) rookery — the `-ski` is
  the family name.

## Install

Prebuilt binaries (macOS arm64/x86_64, Linux x86_64, Windows x86_64) are attached to
[GitHub releases](https://github.com/yarenty/tableski/releases):

```bash
curl -fsSL https://raw.githubusercontent.com/yarenty/tableski/main/install.sh | sh
```

Windows: download the `.zip` from the releases page. Or via cargo:

```bash
cargo install tableski
```

## Connect an MCP client

tableski is a standard MCP server over stateless Streamable HTTP — point any client at the
server URL:

```bash
# Claude Code
claude mcp add --transport http tableski http://127.0.0.1:8080/
```

```json
// Claude Desktop (claude_desktop_config.json) via mcp-remote
{ "mcpServers": { "tableski": { "command": "npx",
    "args": ["mcp-remote", "http://127.0.0.1:8080/"] } } }
```

Or raw HTTP: POST JSON-RPC to `/` with `Accept: application/json` (see below).

## Run

```bash
# One model for every format: --file registers by extension, table name = file stem
# (.csv .parquet .json/.ndjson/.jsonl; workbooks .xlsx/.xls/.ods = one table per sheet)
cargo run -- --file fixtures/sample.xlsx --file fixtures/sample.parquet \
             --file fixtures/sample.ndjson --export-dir ./exports
# (--csv/--xlsx flags still work; name collisions get _2/_3 suffixes)
```

Ask it something (tools: `list_tables`, `query_sql`, `get_schema`, `column_statistics`,
`export_result` — all output framed as data per Emperor Profile E8). SQL joins work across
formats: an xlsx sheet against a Parquet file against an NDJSON log. `export_result` writes
a query's rows to `.csv`/`.xlsx` — only inside the `--export-dir` sandbox (relative names,
no `..`; the tool is disabled unless the operator passes the flag):

```bash
curl -s -X POST http://127.0.0.1:8080/ \
  -H 'Content-Type: application/json' -H 'Accept: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"query_sql",
       "arguments":{"sql":"SELECT p.name, SUM(o.amount) AS total FROM people p JOIN orders o ON p.name = o.name GROUP BY p.name ORDER BY total DESC"}}}'
# => ada 150.5, linus 99.99 — a SQL join across two sheets of one workbook
```

Sheet names are slugified into table names (`People` → `people`, `2024 Orders` →
`t_2024_orders`); first rows become column names when they look like headers
(`--headers auto|first-row|none` to override). Column types are inferred: integers,
floats, booleans, dates (as timestamps), text.

Real-world workbooks are messy — tableski handles the mess: both Excel date systems
(1900 **and** 1904), formula cells (cached values ingest; `#DIV/0!`-style errors become
NULL), merged cells (top-left value, NULL continuations), display formats (raw values,
always), ragged rows, format-only padding (trimmed), unicode sheet names, and a `--max-rows`
cap that fails loudly instead of cutting silently. The nasty-workbook corpus lives in
[`fixtures/corpus/`](fixtures/corpus/) — every trait has a fixture and a test.

Try the whole flow in one go: `./scripts/demo.sh` (spreadsheet → question → SQL answer →
exported workbook). `demo.tape` renders it as a GIF with [vhs](https://github.com/charmbracelet/vhs).

> Status: CSV, Excel (xlsx/xls/ods, hardened against real-world workbooks), Parquet, and
> NDJSON all serve today; results export to csv/xlsx; binaries ship per release. Launch is next.

### Serving clients you do not trust

By default tableski runs whatever SQL DataFusion accepts, which is right for a local
single-user setup. Add `--untrusted-sql` when the server is reachable by agents or people you
do not control:

```sh
tableski --file sales.xlsx --untrusted-sql
```

Then only read-only queries run: `SELECT`, `WITH`, `VALUES`, `EXPLAIN`, one statement per
request, over registered tables only. `CREATE`, `INSERT`, `UPDATE`, `DELETE`, `DROP`, `COPY`,
`SET`, `CREATE EXTERNAL TABLE`, table functions (`read_csv(...)`, `range(...)`) and references
to unregistered tables or file paths are rejected on the parsed statement, before anything
executes, with an error that names the reason. DataFusion's own DDL/DML/statement switches
are turned off as a second fence.

### Limits

One query cannot take the machine down. Defaults, each `0` = unlimited:

| Flag | Default | What it bounds |
|---|---|---|
| `--max-memory-mb` | 2048 | DataFusion memory pool for the session; a query needing more fails with `Resources exhausted` instead of growing the process. With a cap set, spilling to disk is off, so it fails fast rather than filling the disk. |
| `--query-timeout-secs` | 60 | Wall clock per query, planning included. The query runs on a dedicated thread pool and is cancelled at DataFusion's next yield point; the server keeps answering meanwhile. |
| `--max-result-rows` | 10000 | Rows a `query_sql` result carries back. Beyond it the result is cut and ends with a `-- result truncated ...` line; `export_result` refuses instead of writing a partial file. |
| `--max-result-mb` | 16 | Same, by Arrow in-memory size. |

Cartesian products (`CROSS JOIN`, or a join without an `=` condition) are the one shape a
timeout cannot stop once running, so `--untrusted-sql` refuses them on the plan before
execution.

The timeout covers planning as well as execution, so a statement that is expensive merely to
optimise is cut too. What the pool does not meter: memory allocated by scalar functions
(`repeat`, `lpad`, ...). Run a server for untrusted clients under a container memory limit,
as the tableski.io deployment does. The whole set is exercised by `tests/hostile_sql.rs`
(DDL/DML, filesystem reach, identifier tricks, blow-ups, parser stress, export traversal); add
a line there when you find a new bad shape.

## Use as a library

The engine is its own crate, [`tableski-core`](crates/tableski-core): register files into a
DataFusion `SessionContext`, then run the tools with `call_tool` or serve `AppState` through
any [emperor-mcp](https://github.com/yarenty/emperor-mcp) transport. No HTTP, no CLI.

```toml
[dependencies]
tableski-core = "0.1"
```

```rust,ignore
let tables = register_path(&ctx, "orders.xlsx".as_ref(), &IngestOptions::default()).await?;
let state = AppState::new(Arc::new(ctx), tables);
let grid = call_tool(&state, "query_sql", json!({ "sql": "SELECT count(*) FROM orders" })).await?;
```

## How it compares

Honest positioning — different tools solve different problems:

| | tableski | openpyxl-based Excel MCP servers | Google-Sheets MCPs |
|---|---|---|---|
| Ask questions over data | **SQL engine** (DataFusion): joins, aggregates, filters — across sheets *and* file formats | cell/range reads; analysis logic lands in the model's context | per-API calls against live Sheets |
| Formats | xlsx/xls/ods + CSV + Parquet + NDJSON, one table model | xlsx (often read *and write*, incl. formatting/charts — tableski doesn't edit workbooks) | Google Sheets only |
| Deployment | single static binary, stateless Streamable HTTP ([Emperor Profile P1](https://github.com/yarenty/emperor-mcp/blob/main/PROFILE.md)) | Python runtime + deps | hosted API + OAuth |
| Output safety | every tool result framed as data (prompt-injection mitigation) | typically raw | typically raw |

If you need to *edit* workbooks (styles, charts, cell writes), pair tableski with a writer-oriented server; tableski is the analysis and export engine.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md): run the checks, keep the docs in step, and sign off
your commits (DCO, `git commit -s`).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
tableski by you shall be dual licensed as above, without any additional terms or conditions.
