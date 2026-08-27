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
cargo run -- --csv fixtures/sample.csv --table data
# stateless Streamable HTTP MCP server on 0.0.0.0:8080 — tools: SQL query, schema, column stats
```

> Status: CSV works today (carried over with its HTTP smoke tests); Excel ingestion is next.
> See the issues for the build plan.

## License

MIT
