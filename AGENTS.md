# tableski — AI agent notes

**Crate**: `tableski` · **Version**: 0.1.0 (on crates.io) · **Licence**: MIT OR Apache-2.0 · **Rust**: latest stable (`rust-version` in `Cargo.toml` and the Dockerfile base track it; bump both together)

## Scope

This crate is a **standalone MCP HTTP server** (extracted from the [kowalski](https://github.com/yarenty/kowalski) workspace in Aug 2026). It implements the DataFusion tool dispatch as an `McpHandler` and serves it over the [`emperor-mcp`](https://github.com/yarenty/emperor-mcp) framework as **stateless Streamable HTTP** (JSON/SSE, **no `Mcp-Session-Id`**), consistent with `kowalski-core`'s MCP client.

Layout (cargo workspace, #9): the root package is the **CLI** `tableski` (`src/main.rs`; `src/lib.rs` = `app_router` + re-export of the core), the engine is **`crates/tableski-core`** (`lib.rs` handler + tools + `call_tool`, `register.rs` file -> table registration, `excel.rs` workbook ingester, `export.rs` sandboxed result export; `#![warn(missing_docs)]`, no transport, clap only behind the `clap` feature). Integration tests stay at the root in `tests/` (golden, hardening, formats/export, HTTP smoke, **hostile SQL**) with the fixture corpus in `fixtures/`; the embedding contract is `crates/tableski-core/tests/library.rs`. `cargo test --workspace` runs everything; plain `cargo test` only the CLI package.

## Before you change code

1. Read [`crates/tableski-core/src/lib.rs`](./crates/tableski-core/src/lib.rs) for the MCP request/response flow and tool handlers.
2. Run **`cargo test --workspace`** (includes HTTP smoke tests), **`cargo clippy --workspace --all-targets -- -D warnings`**, **`cargo doc -p tableski-core --no-deps`** (must stay warning-free) and **`cargo deny check`** (licences + advisories, config in `deny.toml`).
3. If changing the Docker image, rebuild with **`docker compose build`** (build context = repo root, `Dockerfile` copies the whole crate).

## Conventions

- Prefer small, testable pure functions for SQL/schema helpers; keep the `McpHandler` dispatch thin.
- **Transport is shared + stateless.** HTTP/SSE/stdio framing lives in `emperor-mcp` (crates.io dependency); don't reimplement it here. The server must stay stateless (no session id).
- **Hostile input is the norm.** New ingest paths get a fixture in `fixtures/corpus/` and a test in `tests/excel_hardening.rs` or `tests/formats_export.rs`; export stays sandboxed to `--export-dir`. Bad SQL goes into `tests/hostile_sql.rs`: one line in `cases()` (`Rejected(text)` / `Bounded` / `Works`) or `EXPORT_CASES`; every case must resolve within 10 s against the guarded, limited server and the server must answer afterwards. Run it before touching the guard or the limits.
- **Engine vs CLI.** Anything a third crate could want (ingest, tools, limits, guards) goes into `tableski-core`; the root crate only wires transport and flags. Core must not grow a dependency on axum, or on clap outside the `clap` feature. tokio is used for `time`/`rt` only (the process-wide `query_runtime()` that timed queries run on); no HTTP, no `#[tokio::main]`.
- **Untrusted SQL goes through `guard.rs`.** `AppState::sql` is the only way tools plan SQL; in `SqlTrust::Untrusted` it runs `check_untrusted` (AST decision: one statement, query-only, no table functions, registered tables or CTEs only) and then DataFusion `SQLOptions` with DDL/DML/statements off. New tools that take SQL call `state.sql`, never `ctx.sql`. Decisions are tested on parsed statements in `crates/tableski-core/tests/sql_guard.rs`; extend that file, not string checks.
- **Limits live at one choke point.** `AppState::sql` plans (guard), `AppState::collect` executes (`limits.rs`: streaming collection cut at row/byte caps, timeout via the dedicated query runtime, memory pool on the `SessionContext` from `QueryLimits::session_context`). Tools never call `df.collect()` themselves. Planning is bounded too (`AppState::sql` runs on the query runtime under the same timeout): the optimizer constant-folds expressions like `repeat('x', 2*10^8)` and that alone blocked the server for 19 s before this. Known gap: scalar-function allocations are outside DataFusion's memory pool, so run a public server under a container memory limit. Cartesian products are refused on the physical plan in untrusted mode (`guard::check_plan`) because DataFusion yields too rarely inside them for a timeout to bite; a test that needs a slow-but-cancellable query uses a hash join build, not a cross join. Tests: `crates/tableski-core/tests/limits.rs`.
- **CLI flags are a public contract.** Existing `--file/--csv/--xlsx/--export-dir/--bind` behaviour must not change without a major version.
- Licence is dual `MIT OR Apache-2.0` (`LICENSE-MIT`, `LICENSE-APACHE`); keep `Cargo.toml`, the README badge and the README licence section in agreement. Contributions are signed off (DCO, see `CONTRIBUTING.md`).

## Documentation closure (mandatory)

After any refactor or behaviour change, update **[`README.md`](./README.md)** and **[`ROADMAP.md`](./ROADMAP.md)**; add a `CHANGELOG.md` entry once the file exists (first release). **Shipping code without updating docs is incomplete work.**

## Related docs

- [`README.md`](./README.md) — install, connect, run, comparison
- [`ROADMAP.md`](./ROADMAP.md) — crate roadmap
- [`ANNOUNCEMENT.md`](./ANNOUNCEMENT.md) — launch drafts (manual publish)
- [`landing/`](./landing/) — tableski.io landing page

## Business planning

Product vision, hosted-service (tableski.io) plans and server operations live in a **private** planning repo, not here. This repo stays the public OSS engine; keep ROADMAP.md about the crate only.
