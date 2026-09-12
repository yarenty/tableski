# tableski — AI agent notes

**Crate**: `tableski` · **Version**: 0.1.0 (crates.io publish planned for 1.0) · **Licence**: MIT OR Apache-2.0

## Scope

This crate is a **standalone MCP HTTP server** (extracted from the [kowalski](https://github.com/yarenty/kowalski) workspace in Aug 2026). It implements the DataFusion tool dispatch as an `McpHandler` and serves it over the [`emperor-mcp`](https://github.com/yarenty/emperor-mcp) framework as **stateless Streamable HTTP** (JSON/SSE, **no `Mcp-Session-Id`**), consistent with `kowalski-core`'s MCP client.

Layout: `src/lib.rs` (handler + tools), `src/register.rs` (file -> table registration), `src/excel.rs` (workbook ingester), `src/export.rs` (sandboxed result export), `src/main.rs` (CLI). Tests in `tests/` (golden, hardening, formats/export, HTTP smoke) with the fixture corpus in `fixtures/`.

## Before you change code

1. Read [`src/lib.rs`](./src/lib.rs) for the MCP request/response flow and tool handlers.
2. Run **`cargo test`** (includes HTTP smoke tests) and **`cargo deny check`** (licences + advisories, config in `deny.toml`).
3. If changing the Docker image, rebuild with **`docker compose build`** (build context = repo root, `Dockerfile` copies the whole crate).

## Conventions

- Prefer small, testable pure functions for SQL/schema helpers; keep the `McpHandler` dispatch thin.
- **Transport is shared + stateless.** HTTP/SSE/stdio framing lives in `emperor-mcp` (crates.io dependency); don't reimplement it here. The server must stay stateless (no session id).
- **Hostile input is the norm.** New ingest paths get a fixture in `fixtures/corpus/` and a test in `tests/excel_hardening.rs` or `tests/formats_export.rs`; export stays sandboxed to `--export-dir`.
- **CLI flags are a public contract.** Existing `--file/--csv/--xlsx/--export-dir/--bind` behaviour must not change without a major version.
- Licence is dual `MIT OR Apache-2.0` (`LICENSE-MIT`, `LICENSE-APACHE`); keep `Cargo.toml`, the README badge and the README licence section in agreement.

## Documentation closure (mandatory)

After any refactor or behaviour change, update **[`README.md`](./README.md)** and **[`ROADMAP.md`](./ROADMAP.md)**; add a `CHANGELOG.md` entry once the file exists (first release). **Shipping code without updating docs is incomplete work.**

## Related docs

- [`README.md`](./README.md) — install, connect, run, comparison
- [`ROADMAP.md`](./ROADMAP.md) — crate roadmap
- [`ANNOUNCEMENT.md`](./ANNOUNCEMENT.md) — launch drafts (manual publish)
- [`landing/`](./landing/) — tableski.io landing page

## Business planning

Product vision, hosted-service (tableski.io) plans and server operations live in a **private** planning repo, not here. This repo stays the public OSS engine; keep ROADMAP.md about the crate only.
