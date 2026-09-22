#!/usr/bin/env sh
# tableski demo: upload the two-sheet workbook, list its tables, ask a question that joins them.
#   Hosted:  TABLESKI_TOKEN=tsk_... ./demo.sh
#   Local:   TABLESKI_URL=http://127.0.0.1:8080/ ./demo.sh     (tableski --file sample.xlsx running)
set -eu
cd "$(dirname "$0")"
MCP="${TABLESKI_URL:-https://mcp.tableski.io/}"
API="${TABLESKI_API:-https://api.tableski.io}"
AUTH=""
if [ -n "${TABLESKI_TOKEN:-}" ]; then AUTH="Authorization: Bearer $TABLESKI_TOKEN"; fi
if [ -z "$AUTH" ] && [ -z "${TABLESKI_URL:-}" ]; then
  echo "set TABLESKI_TOKEN (from https://tableski.io/app/tokens) for the hosted service, or TABLESKI_URL for a local tableski"; exit 1
fi

# Results come framed as DATA for AI clients; people only need the middle.
UNFRAME='import json,sys
r=json.load(sys.stdin)
if "error" in r: print("error:", r["error"]["message"]); sys.exit(1)
for l in r["result"]["content"][0]["text"].splitlines():
    if l.strip() and not l.startswith("----- ") and not l.startswith("The following is a computed result"): print(l)'

say() { printf '\n\033[1;36m%s\033[0m\n' "$*"; }
rpc() {  # $1 = tool name, $2 = arguments json
  curl -sS -X POST "$MCP" -H 'Content-Type: application/json' -H 'Accept: application/json' ${AUTH:+-H "$AUTH"} \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"$1\",\"arguments\":$2}}" \
  | python3 -c "$UNFRAME"
}

if [ -n "$AUTH" ] && [ -z "${TABLESKI_URL:-}" ]; then
  say "1. Upload sample.xlsx to your account"
  curl -sS -X POST "$API/v1/files" -H "$AUTH" -F file=@sample.xlsx | python3 -c 'import json,sys
r=json.load(sys.stdin)
if "error" in r: print("upload:", r["error"]); sys.exit(0 if "already" in r["error"] else 1)
print("uploaded", r["name"], "->", ", ".join(t["name"] for t in r["tables"]))'
fi

say "2. What tables do I have?  (list_tables)"
rpc list_tables '{}'

say "3. Who spent the most, across the two sheets?  (query_sql)"
rpc query_sql '{"sql":"SELECT p.name, SUM(o.amount) AS total FROM people p JOIN orders o ON p.name = o.name GROUP BY p.name ORDER BY total DESC"}'

say "Next: ask in plain words. See README.md for kowalski, Claude Desktop, Claude Code or Cursor."
