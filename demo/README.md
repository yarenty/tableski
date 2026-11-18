# tableski in five minutes

A spreadsheet, an AI, real answers. This folder is the whole demo: a two-sheet workbook
(`sample.xlsx`: `people` and `orders`), a script that puts it into tableski and asks the first
question, and a kowalski config that lets you ask the rest in plain words.

It runs the engine on your machine and needs no account. (The service at tableski.io is work in
progress and takes no new accounts.)

## Local (your machine, no account)

```sh
curl -fsSL https://raw.githubusercontent.com/yarenty/tableski/main/install.sh | sh
tableski --file sample.xlsx --export-dir ./exports        # listens on http://127.0.0.1:8080/
TABLESKI_URL=http://127.0.0.1:8080/ ./demo.sh             # in another terminal, no token needed
```

You should see two tables, `people` and `orders`, and this answer computed by a JOIN across the
two sheets:

```
name   | total
-------+-------
ada    | 150.5
linus  | 99.99
```

## With an existing tableski.io account

If you already have an account, the same script works against it: create a token on **Tokens**,
then

```sh
export TABLESKI_TOKEN=tsk_...
./demo.sh
```

## Ask in plain words with kowalski

[kowalski](https://github.com/yarenty/kowalski) is an agent runtime that speaks MCP. Give it the
tableski server and a language model, and the SQL is written for you.

```sh
curl -fsSL https://raw.githubusercontent.com/yarenty/kowalski/main/install.sh | bash
cp kowalski.toml ~/.config/kowalski/config.toml            # 127.0.0.1:8080 for local
export OPENAI_API_KEY=...                                  # key for the chat model named in [llm]
kowalski-cli mcp tools -c ~/.config/kowalski/config.toml   # lists list_tables, query_sql, get_schema, ...
kowalski-cli run -c ~/.config/kowalski/config.toml
```

The config names an OpenAI-compatible chat endpoint and model; any Chat Completions server works,
and the key comes from the environment so it never sits in a file. No key at all? Swap the `[llm]`
block for `[ollama]` with a local model, free and offline.

Then type, for example:

- *Which tables do I have and what is in them?*
- *Who spent the most, across both sheets?*
- *List everyone from Dublin with their total order amount, highest first.*
- *Save the totals per person as a spreadsheet called totals.xlsx.*

The agent calls `list_tables`, writes the SQL, calls `query_sql`, and answers with the rows. The
SQL it used is in the tool call, so a wrong number can always be checked.

The same setup works from Claude Desktop, Claude Code or Cursor; the main README shows the
client configuration.

## Your own file

Replace `sample.xlsx` with any workbook (one table per sheet) or CSV, for example an export of
your positions or trades, and ask about that instead.
