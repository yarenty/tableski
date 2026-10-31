# Changelog

All notable changes to tableski and tableski-core. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [semantic versioning](https://semver.org/), with 1.0.0 marking the stable engine API.

## [Unreleased]

## [0.2.1] - 2026-10-31

> Security update of the Excel dependencies; no change in behaviour.

### Changed
- Minimum Rust version is 1.99 (`rust-version` and the Docker build image); the quota counters use `try_update`, the new name of the deprecated `fetch_update`.

### Security
- **zip 9.1 via calamine 0.37 and rust_xlsxwriter 0.101** - RUSTSEC-2026-0367, RUSTSEC-2026-0368 and RUSTSEC-2026-0369 (SUID/SGID bits kept, symlinks outside the destination, and permissions applied through symlinks, all when extracting an archive to disk). tableski, calamine and rust_xlsxwriter only read and write archives in memory and never extract one, so tableski was not exposed. zip is now a single version (9.1) instead of 7.4 and 8.6.
- **zstd 0.13 via DataFusion 54** - RUSTSEC-2026-0391, RUSTSEC-2026-0398 and RUSTSEC-2026-0403 are recorded as accepted in `deny.toml`: arrow and parquet compress and decompress with `zstd::bulk` only, never the stream API the advisories concern, and the `seekable` feature is not built. The fixes are in zstd 0.14, which arrives with arrow 60; the entries go when DataFusion moves there.

## [0.2.0] - 2026-09-27

> The engine as its own crate, safe to point at untrusted SQL.

### Added
- **`tableski-core` on crates.io** - ingest, registration, tools, framing, export and `call_tool` for embedders; `tableski` is the CLI on top. CLI flags unchanged.
- **`TableSource` trait** - `FileSource` behind `--file` / `--csv` / `--xlsx`; a new kind of table is a new source, not a special case.
- **Untrusted-SQL guard** (`--untrusted-sql`) - one read-only query per request over registered tables; DDL, DML, COPY, SET, external tables, table functions and unregistered tables are refused.
- **Per-query limits** - memory cap (`--max-memory-mb`), timeout (`--query-timeout-secs`), result row and byte caps with a truncation note (`--max-result-rows`, `--max-result-mb`); cartesian products refused in untrusted mode.
- **Quota primitives** - `Quota` and `UsageMeter` (queries per day, bytes stored, files, bytes and rows per file) for hosts that meter tenants.
- Hostile-SQL test suite: 47 statements and 9 export paths, each bounded within 10 s.

## [0.1.0] - 2026-08-27

> Every spreadsheet is a table.

### Added
- Standalone MCP server on [emperor-mcp](https://github.com/yarenty/emperor-mcp): stateless Streamable HTTP, framed output.
- Excel ingestion (xlsx, xls, ods): one table per sheet, typed columns, header detection; 1904 dates, cached formula values, error cells as NULL, merged cells, ragged rows, `--max-rows` cap.
- CSV, Parquet, JSON and NDJSON by extension, cross-format joins, and `export_result` (CSV or xlsx) sandboxed to `--export-dir`.
- Release binaries for four targets, `install.sh`, MCP client config snippets.

[Unreleased]: https://github.com/yarenty/tableski/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/yarenty/tableski/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/yarenty/tableski/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/yarenty/tableski/releases/tag/v0.1.0
