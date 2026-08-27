#!/usr/bin/env bash
# The tableski demo: a spreadsheet, a question, a SQL answer, an exported file.
# Record it with vhs (https://github.com/charmbracelet/vhs): `vhs demo.tape`
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n\033[1;36m$ %s\033[0m\n' "$*"; }

step "tableski --file fixtures/sample.xlsx --export-dir ./exports &"
cargo run -q -- --file fixtures/sample.xlsx --export-dir ./exports &
SRV=$!
trap 'kill $SRV 2>/dev/null || true' EXIT
for _ in $(seq 1 60); do
  curl -s -o /dev/null -X POST http://127.0.0.1:8080/ -d '{}' && break || sleep 0.5
done

q() {
  curl -s -X POST http://127.0.0.1:8080/ \
    -H 'Content-Type: application/json' -H 'Accept: application/json' \
    -d "$1" | python3 -c "import json,sys
r=json.load(sys.stdin)
t=r.get('result',{}).get('content',[{}])[0].get('text', r.get('error',{}).get('message',''))
print(t)"
}

step "What tables do I have?  (list_tables)"
q '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_tables","arguments":{}}}'

step "Who spent the most, across two sheets?  (query_sql)"
q '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"query_sql","arguments":{"sql":"SELECT p.name, SUM(o.amount) AS total FROM people p JOIN orders o ON p.name = o.name GROUP BY p.name ORDER BY total DESC"}}}'

step "Save that as a spreadsheet.  (export_result)"
q '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"export_result","arguments":{"sql":"SELECT p.name, SUM(o.amount) AS total FROM people p JOIN orders o ON p.name = o.name GROUP BY p.name ORDER BY total DESC","file":"totals.xlsx"}}}'

step "ls exports/"
ls -la exports/
