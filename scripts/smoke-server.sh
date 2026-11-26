#!/usr/bin/env bash
# Prove the CLI serves the UI, the manifest and a working invoke API end to end.
# Usage: bash scripts/smoke-server.sh [port]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=repo-root.sh
source "$SCRIPT_DIR/repo-root.sh"

PORT="${1:-4311}"
BASE="http://127.0.0.1:${PORT}"
TMP_DIR="/tmp/fnscope-smoke-$$"
PIDFILE="$TMP_DIR/server.pid"
LOGFILE="$TMP_DIR/server.log"

cd "$REPO_ROOT"

# Track generated .fnscope directories so cleanup never deletes a pre-existing one.
FNSCOPE_AT_ROOT=0
FNSCOPE_AT_FIXTURE=0
if [[ -e "$REPO_ROOT/.fnscope" ]]; then FNSCOPE_AT_ROOT=1; fi
if [[ -e "$REPO_ROOT/tests/fixtures/sample_app/.fnscope" ]]; then FNSCOPE_AT_FIXTURE=1; fi

terminate_tree() {
  local parent="$1" depth="$2" child
  command -v pgrep >/dev/null 2>&1 || return 0
  for child in $(pgrep -P "$parent" 2>/dev/null || true); do
    if [[ "$depth" -lt 4 ]]; then
      terminate_tree "$child" "$((depth + 1))"
    fi
    kill "$child" 2>/dev/null || true
  done
}

stop_server() {
  local pid="" i=0
  if [[ -f "$PIDFILE" ]]; then
    pid="$(cat "$PIDFILE" 2>/dev/null || true)"
  fi
  if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
    # cargo run spawns the real binary as a child: stop descendants first.
    terminate_tree "$pid" 0
    kill "$pid" 2>/dev/null || true
    while kill -0 "$pid" 2>/dev/null && [[ "$i" -lt 20 ]]; do
      sleep 0.1
      i=$((i + 1))
    done
    if kill -0 "$pid" 2>/dev/null; then
      terminate_tree "$pid" 0
      kill -9 "$pid" 2>/dev/null || true
    fi
  fi
  rm -rf "$TMP_DIR"
  if [[ "$FNSCOPE_AT_ROOT" -eq 0 ]]; then
    rm -rf "$REPO_ROOT/.fnscope"
  fi
  if [[ "$FNSCOPE_AT_FIXTURE" -eq 0 ]]; then
    rm -rf "$REPO_ROOT/tests/fixtures/sample_app/.fnscope"
  fi
}

mkdir -p "$TMP_DIR"
trap stop_server EXIT

echo "[smoke-server] starting fnscope open on $BASE (log: $LOGFILE)"
cargo run --quiet -p fnscope -- open "$REPO_ROOT/tests/fixtures/sample_app" \
  --port "$PORT" --host 127.0.0.1 --no-watch --invoke simulate \
  >"$LOGFILE" 2>&1 &
echo $! >"$PIDFILE"

# Readiness is a separate, bounded poll: at most 120 attempts, 0.25s apart (30s).
# A cold `cargo run` can compile the binary before the server binds, so the
# window must cover compilation, not just process startup.
ready=0
for ((attempt = 1; attempt <= 120; attempt++)); do
  if curl -fsS --max-time 2 "$BASE/api/health" >/dev/null 2>&1; then
    ready=1
    break
  fi
  server_pid="$(cat "$PIDFILE" 2>/dev/null || true)"
  if [[ -n "$server_pid" ]] && ! kill -0 "$server_pid" 2>/dev/null; then
    break
  fi
  sleep 0.25
done
if [[ "$ready" -ne 1 ]]; then
  echo "smoke-server: server did not become healthy after 120 attempts" >&2
  tail -n 40 "$LOGFILE" >&2 || true
  exit 1
fi

curl -fsS --max-time 5 "$BASE/" -o "$TMP_DIR/index.html"
grep -q 'id="root"' "$TMP_DIR/index.html"

curl -fsS --max-time 5 "$BASE/app.js" -o "$TMP_DIR/app.js"
[[ -s "$TMP_DIR/app.js" ]]

curl -fsS --max-time 5 "$BASE/app.css" -o "$TMP_DIR/app.css"
[[ -s "$TMP_DIR/app.css" ]]

curl -fsS --max-time 5 "$BASE/api/fn-graph.json" -o "$TMP_DIR/fn-graph.json"
node --input-type=commonjs -e '
const fs = require("node:fs");
const graph = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
if (graph.tool !== "fnscope") {
  console.error("SMOKE_FAIL: served manifest tool must be fnscope");
  process.exit(1);
}
if (!Array.isArray(graph.functions) || graph.functions.length === 0) {
  console.error("SMOKE_FAIL: served manifest has no functions");
  process.exit(1);
}
' "$TMP_DIR/fn-graph.json"

FIRST_ID="$(node --input-type=commonjs -e '
const fs = require("node:fs");
const graph = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
process.stdout.write(String(graph.functions[0].id));
' "$TMP_DIR/fn-graph.json")"

curl -fsS --max-time 5 -H "Content-Type: application/json" \
  -d "{\"fnId\":\"${FIRST_ID}\",\"args\":{}}" \
  "$BASE/api/invoke" -o "$TMP_DIR/invoke.json"

node --input-type=commonjs -e '
const fs = require("node:fs");
const response = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
const problems = [];
if (response.ok !== true) problems.push("invoke response ok must be true");
if (!response.trace || typeof response.trace !== "object" || Array.isArray(response.trace)) {
  problems.push("invoke response must contain a trace object");
}
if (problems.length > 0) {
  for (const problem of problems) console.error("SMOKE_FAIL: " + problem);
  process.exit(1);
}
' "$TMP_DIR/invoke.json"

# Cleanup runs before the verdict so a successful marker always means no server is left behind.
stop_server
trap - EXIT
echo "SERVER_SMOKE_OK"
