# tableski roadmap

> The public OSS engine only. Work is tracked in this repo's
> [issues](https://github.com/yarenty/tableski/issues); tick items here when they ship.

Crate version **0.1.0** ([crates.io](https://crates.io/crates/tableski), Aug 2026), licence
`MIT OR Apache-2.0`. **1.0.0** marks the stable engine API.

## Done (0.1.0, Aug 2026)

- [x] Standalone crate on [emperor-mcp](https://github.com/yarenty/emperor-mcp); stateless
      Streamable HTTP, framed output, no session affinity (#1).
- [x] Excel ingestion: xlsx/xls/ods, one table per sheet, typed columns, header detection (#2).
- [x] Real-world hardening: 1904 dates, cached formula values, error cells as NULL, merged
      cells, ragged rows, unicode sheet names, `--max-rows` cap; committed fixture corpus (#3).
- [x] Format breadth: `--file` registers csv/parquet/json/ndjson/jsonl by extension; cross-format
      joins; `export_result` tool (csv/xlsx) sandboxed to `--export-dir` (#4).
- [x] Distribution: release workflow (4 targets), `install.sh`, MCP client config snippets,
      vhs demo (#5). Launch kit: README, announcement drafts, landing page (#6).

## 1.0 — stable engine (#8)

- [x] Split into a cargo workspace: `tableski-core` (ingest, registration, tools, framing,
      export, `call_tool` for embedders) + `tableski` (CLI, root package). CLI flags identical;
      the existing tests passed untouched (#9, 2026-09-19).
- [ ] `TableSource` trait so tables can come from something other than files on disk (#10).
- [x] Untrusted-SQL guard (`--untrusted-sql`, `AppState::untrusted()`): one read-only query per
      request over registered tables; DDL/DML/COPY/SET, external tables, table functions and
      unregistered table references rejected on the AST, DataFusion `SQLOptions` as a second
      fence (#11, 2026-09-19).
- [x] Per-query limits: memory pool cap (`--max-memory-mb`, spill off), wall-clock timeout on a
      dedicated query runtime (`--query-timeout-secs`), result row/byte caps with a truncation
      note (`--max-result-rows`, `--max-result-mb`); safe defaults, `0` = unlimited; cartesian
      products refused on the plan in untrusted mode (#12, 2026-09-19).
- [x] Hostile-SQL test suite `tests/hostile_sql.rs`: 47 data-driven statements (DDL/DML,
      filesystem and catalog reach, identifier tricks, cartesian and join blow-ups, huge result /
      sort / string, recursive CTE, parser stress) + 9 export path cases, each bounded within
      10 s, server answers after each; runs in ~11 s (#13, 2026-09-19).
- [ ] Document performance expectations for large files (what is streamed, what is loaded) (#14).
- [ ] `CHANGELOG.md`, semver policy, docs.rs green for every release (#8).

## Later

- [ ] Live/refreshing table sources behind `TableSource` (e.g. periodically re-read files,
      in-memory tables updated by a feed). These may need subscriptions or per-client state;
      stateless HTTP stays the default and anything stateful is opt-in, designed when it comes.
- [ ] More formats on request (Arrow IPC, Avro) where DataFusion already has readers.
- [ ] Optional distributed path (Ballista) only if it keeps the same MCP surface.

## Out of scope

Editing workbooks (styles, charts, cell writes), authentication and multi-tenancy — those
belong in a hosting layer, not in the engine.
