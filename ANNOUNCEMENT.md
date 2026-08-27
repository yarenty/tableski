# Announcement drafts

> Maintainer publishes manually; nothing here is auto-posted.

---

## Blog post

### Your spreadsheets already are a database. tableski makes them answer like one.

Most of the world's operational data doesn't live in data warehouses. It lives in
spreadsheets — quarterly numbers, inventory counts, exported reports, that one workbook
everyone's afraid to touch. And now that AI agents can call tools, everyone wants to point
an agent at those files and just ask questions.

The usual answer is a wrapper that reads cells and dumps ranges into the model's context —
making the model do arithmetic over pasted text. That falls over exactly when it matters:
big sheets, multi-sheet joins, questions with a GROUP BY in their soul.

**[tableski](https://github.com/yarenty/tableski)** takes the other road: every spreadsheet
is a *table*. It loads xlsx/xls/ods (plus CSV, Parquet, NDJSON) into
[Apache DataFusion](https://datafusion.apache.org/)-powered SQL tables and serves them to any
MCP client as five tools: list the tables, read a schema, get column statistics, run SQL —
including joins across sheets and across file formats — and export a result set back out as
a fresh .csv or .xlsx.

Real-world workbooks are messy, so tableski is hardened for the mess: both Excel date systems
(1900 *and* 1904), cached formula values, `#DIV/0!` errors as NULLs, merged cells, ragged
rows, format-only padding, unicode sheet names — every trait has a committed fixture and a
test. Type inference is real: dates become timestamps you can `WHERE joined >= '2024-01-01'`
against.

And because it's built on [emperor-mcp](https://github.com/yarenty/emperor-mcp), it ships
with production posture: a single static binary, stateless Streamable HTTP (no session
affinity), every tool result framed as data so a hostile cell can't smuggle instructions to
your model, and exports sandboxed to an operator-chosen directory.

`cargo install tableski`, or grab a binary for macOS/Linux/Windows from the releases page.
One penguin-family binary — it hatched in the same rookery as
[kowalski](https://github.com/yarenty/kowalski). MIT, on
[GitHub](https://github.com/yarenty/tableski) and [crates.io](https://crates.io/crates/tableski).

---

## LinkedIn post

Everyone wants to point an AI agent at their spreadsheets. Most tools do it by pasting cell
ranges into the model's context — and then the model does arithmetic over text. That breaks
exactly when the question matters.

**tableski** does it the database way: every spreadsheet is a table.

📊 xlsx/xls/ods + CSV + Parquet + NDJSON → real SQL tables (Apache DataFusion)
🔗 Joins across sheets — and across file formats
🧮 Typed columns: Excel dates become real timestamps (both 1900 & 1904 date systems)
🛡️ Single binary, stateless MCP over HTTP, output framed against prompt injection
📤 Results export back to .csv/.xlsx, sandboxed

Hardened for real workbooks: merged cells, cached formulas, #DIV/0!, ragged rows — every
nasty trait has a fixture and a test.

`cargo install tableski` · github.com/yarenty/tableski · MIT

#rust #mcp #excel #sql #ai #datafusion #opensource
