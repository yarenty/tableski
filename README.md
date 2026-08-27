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

> Status: under construction — extraction from the kowalski workspace in progress. See the
> issues for the build plan.

## License

MIT
