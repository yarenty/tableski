# tableski

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

## How it compares

Honest positioning — different tools solve different problems:

| | tableski | openpyxl-based Excel MCP servers | Google-Sheets MCPs |
|---|---|---|---|
| Ask questions over data | **SQL engine** (DataFusion): joins, aggregates, filters — across sheets *and* file formats | cell/range reads; analysis logic lands in the model's context | per-API calls against live Sheets |
| Formats | xlsx/xls/ods + CSV + Parquet + NDJSON, one table model | xlsx (often read *and write*, incl. formatting/charts — tableski doesn't edit workbooks) | Google Sheets only |
| Deployment | single static binary, stateless Streamable HTTP ([Emperor Profile P1](https://github.com/yarenty/emperor-mcp/blob/main/PROFILE.md)) | Python runtime + deps | hosted API + OAuth |
| Output safety | every tool result framed as data (prompt-injection mitigation) | typically raw | typically raw |

If you need to *edit* workbooks (styles, charts, cell writes), pair tableski with a writer-oriented server; tableski is the analysis and export engine.

## License

MIT
