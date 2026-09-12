# tableski roadmap

> The public OSS engine only. Work is tracked in this repo's
> [issues](https://github.com/yarenty/tableski/issues); tick items here when they ship.

Crate version **0.1.0**, licence `MIT OR Apache-2.0`. Not yet on crates.io — the first
crates.io release will be **1.0.0**.

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

## 1.0 — stable engine on crates.io

- [ ] Split into a cargo workspace: `tableski-core` (ingest, registration, tools, framing,
      export) + `tableski` (CLI). CLI flags stay identical; existing tests are the regression fence.
- [ ] `TableSource` trait so tables can come from something other than files on disk.
- [ ] Untrusted-SQL guard: reject DDL/DML, external tables and `read_*` table functions when
      serving clients that are not trusted (opt-in flag).
- [ ] Per-query limits: memory pool cap, wall-clock timeout, result row/byte cap.
- [ ] Hostile-SQL test suite (DDL, external table, cartesian blow-up, huge result).
- [ ] Document performance expectations for large files (what is streamed, what is loaded).
- [ ] `CHANGELOG.md`, semver policy, `cargo publish`, docs.rs green.

## Later

- [ ] Live/refreshing table sources behind `TableSource` (e.g. periodically re-read files,
      in-memory tables updated by a feed).
- [ ] More formats on request (Arrow IPC, Avro) where DataFusion already has readers.
- [ ] Optional distributed path (Ballista) only if it keeps the same MCP surface.

## Out of scope

Editing workbooks (styles, charts, cell writes), authentication and multi-tenancy — those
belong in a hosting layer, not in the engine.
