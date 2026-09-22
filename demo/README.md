# tableski in five minutes

A spreadsheet, an AI, real answers. This folder is the whole demo: a two-sheet workbook
(`sample.xlsx`: `people` and `orders`), a script that puts it into tableski and asks the first
question, and a kowalski config that lets you ask the rest in plain words.

Two ways to run it. **Hosted** needs nothing installed but `curl`. **Local** runs the engine on
your machine and needs no account.

## Hosted (tableski.io)

1. Sign in at https://tableski.io/app/ (Google, Microsoft, GitHub or email). Free, no card.
2. On **Tokens**, create a token and copy it once.
3. Run the script; it uploads the workbook, lists the tables and asks the first question:

```sh
export TABLESKI_TOKEN=tsk_...        # the token from step 2
./demo.sh
```

You should see two tables, `people` and `orders`, and this answer computed by a JOIN across the
two sheets:

```
name   | total
-------+-------
ada    | 150.5
linus  | 99.99
```

The same file and the same tables are now on the **Query** page of the app, where you can type
SQL yourself.

## Local (your machine, no account)

```sh
curl -fsSL https://raw.githubusercontent.com/yarenty/tableski/main/install.sh | sh
tableski --file sample.xlsx --export-dir ./exports        # listens on http://127.0.0.1:8080/
TABLESKI_URL=http://127.0.0.1:8080/ ./demo.sh             # in another terminal, no token needed
```

## Ask in plain words with kowalski

[kowalski](https://github.com/yarenty/kowalski) is an agent runtime that speaks MCP. Give it the
tableski server and a language model, and the SQL is written for you.

```sh
curl -fsSL https://raw.githubusercontent.com/yarenty/kowalski/main/install.sh | bash
ollama pull llama3.2                                       # or any model kowalski supports; local = free
cp kowalski.toml ~/.config/kowalski/config.toml            # tableski.io + your token, or 127.0.0.1:8080 for local
kowalski-cli mcp tools -c ~/.config/kowalski/config.toml   # lists list_tables, query_sql, get_schema, ...
kowalski-cli run -c ~/.config/kowalski/config.toml
```

Then type, for example:

- *Which tables do I have and what is in them?*
- *Who spent the most, across both sheets?*
- *List everyone from Dublin with their total order amount, highest first.*
- *Save the totals per person as a spreadsheet called totals.xlsx.*

The agent calls `list_tables`, writes the SQL, calls `query_sql`, and answers with the rows. The
SQL it used is in the tool call, so a wrong number can always be checked.

The same setup works from Claude Desktop, Claude Code or Cursor: the **Connect** page of the app
prints the exact config for each with your token filled in.

## Your own file

Replace `sample.xlsx` with any workbook (one table per sheet) or CSV, and ask about that instead.
On the free plan a file is 5 MB and 100,000 rows at most and is kept for 24 hours.
