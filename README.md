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

## Run

```bash
# Excel: every non-empty sheet becomes a queryable table (typed columns, auto headers)
cargo run -- --xlsx fixtures/sample.xlsx
# ...or CSV, or both together:
cargo run -- --csv fixtures/sample.csv --table data --xlsx fixtures/sample.xlsx
```

Ask it something (tools: `list_tables`, `query_sql`, `get_schema`, `column_statistics` —
all output framed as data per Emperor Profile E8):

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

> Status: CSV + Excel (xlsx/xls/ods) work today. Hardening for messy real-world workbooks,
> Parquet/JSON breadth, and result export are next — see the issues for the build plan.

## License

MIT
