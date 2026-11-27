#!/usr/bin/env bash
# Post-impl simulation for the workspace UI pack.
#
# Starts the real server against tests/fixtures/workspace, then proves scope
# isolation, call edges, compatibility routes, invoke behaviour and the served
# manifest contract. This is the repeatable half of the recorded browser audit.
#
# Usage: bash docs/features/workspace-ui/simulate.sh [port]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
PORT="${1:-4322}"
BASE="http://127.0.0.1:${PORT}"
TMP_DIR="$(mktemp -d)"
PIDFILE="$TMP_DIR/server.pid"
LOGFILE="$TMP_DIR/server.log"
WORKSPACE="tests/fixtures/workspace/fnscope.workspace.json"

cd "$REPO_ROOT"

# Never delete a .fnscope directory that existed before this run.
PREEXISTING_FNSCOPE=0
if [[ -e "$REPO_ROOT/.fnscope" ]]; then PREEXISTING_FNSCOPE=1; fi

stop_server() {
  local pid=""
  if [[ -f "$PIDFILE" ]]; then pid="$(cat "$PIDFILE" 2>/dev/null || true)"; fi
  if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
    pkill -TERM -P "$pid" 2>/dev/null || true
    kill -TERM "$pid" 2>/dev/null || true
    sleep 0.5
    pkill -KILL -P "$pid" 2>/dev/null || true
    kill -KILL "$pid" 2>/dev/null || true
  fi
  if [[ "$PREEXISTING_FNSCOPE" -eq 0 ]]; then rm -rf "$REPO_ROOT/.fnscope"; fi
  rm -rf "$TMP_DIR"
}
trap stop_server EXIT

fail() {
  printf 'WORKSPACE_SIM_FAIL: %s\n' "$1" >&2
  exit 1
}

# Keep the simulation's generated manifests out of the committed fixture tree.
if [[ -e "$REPO_ROOT/.fnscope" ]]; then
  fail ".fnscope already exists; remove or move it before simulating"
fi

echo "[simulate] starting fnscope open --workspace $WORKSPACE on $BASE"
cargo run --quiet -p fnscope -- open --workspace "$WORKSPACE" \
  --port "$PORT" --host 127.0.0.1 --no-watch >"$LOGFILE" 2>&1 &
echo $! >"$PIDFILE"

ready=0
for ((attempt = 1; attempt <= 240; attempt++)); do
  if curl -fsS --max-time 2 "$BASE/api/health" >/dev/null 2>&1; then ready=1; break; fi
  pid="$(cat "$PIDFILE" 2>/dev/null || true)"
  if [[ -n "$pid" ]] && ! kill -0 "$pid" 2>/dev/null; then
    tail -n 30 "$LOGFILE" >&2 || true
    fail "server exited before becoming healthy"
  fi
  sleep 0.25
done
[[ "$ready" -eq 1 ]] || fail "server did not become healthy"

curl -fsS --max-time 5 "$BASE/api/scopes" -o "$TMP_DIR/scopes.json"
curl -fsS --max-time 5 "$BASE/api/scopes/alpha/fn-graph.json" -o "$TMP_DIR/alpha.json"
curl -fsS --max-time 5 "$BASE/api/scopes/beta/fn-graph.json" -o "$TMP_DIR/beta.json"
curl -fsS --max-time 5 "$BASE/api/fn-graph.json" -o "$TMP_DIR/compat.json"

SIM_PORT="$PORT" node --input-type=commonjs - "$TMP_DIR" <<'NODE'
const fs = require('node:fs');
const path = require('node:path');
const dir = process.argv[2];
const read = (name) => JSON.parse(fs.readFileSync(path.join(dir, name), 'utf8'));
const fail = (message) => {
  console.error(`WORKSPACE_SIM_FAIL: ${message}`);
  process.exit(1);
};

const list = read('scopes.json');
const alpha = read('alpha.json');
const beta = read('beta.json');
const compat = read('compat.json');

if (!Array.isArray(list.scopes) || list.scopes.length !== 2) fail('the workspace must list two scopes');
if (list.defaultScope !== 'alpha') fail('the default scope must be alpha');
for (const scope of list.scopes) {
  if (scope.rootDir.includes(':') || scope.rootDir.startsWith('/')) fail(`scope ${scope.id} leaks an absolute root`);
  if (scope.available !== true) fail(`scope ${scope.id} is unavailable`);
  if (scope.invokeMode !== 'simulate') fail(`scope ${scope.id} must report simulate mode`);
}
if (compat.functions.length !== alpha.functions.length) fail('the compatibility route must serve the default scope');

const nameOf = (graph, name) => graph.functions.find((fn) => fn.name === name);
for (const [label, graph, entryName] of [
  ['alpha', alpha, 'alphaEntry'],
  ['beta', beta, 'betaEntry'],
]) {
  if (graph.functions.length !== 2) fail(`${label} must contain exactly its two functions`);
  const entry = nameOf(graph, entryName);
  const shared = nameOf(graph, 'shared');
  if (!entry || !shared) fail(`${label} is missing its entry or shared helper`);
  const call = entry.steps.find((step) => step.callsFnId);
  if (!call) fail(`${label}'s entry must record a resolved call edge`);
  if (call.callsFnId !== shared.id) fail(`${label}'s edge must point at ${label}'s shared`);
  if (!shared.calledBy.some((caller) => caller.fnId === entry.id)) {
    fail(`${label}'s shared must list ${entryName} as a resolved caller`);
  }
  if (!entry.sourceCode || !entry.steps.length) fail(`${label} must serve real source and steps`);
}
if (nameOf(alpha, 'betaEntry') || nameOf(beta, 'alphaEntry')) fail('scope functions leaked across manifests');
if (JSON.stringify(alpha.functions.map((fn) => fn.sourceCode)) === JSON.stringify(beta.functions.map((fn) => fn.sourceCode))) {
  fail('alpha and beta must not share identical source bodies');
}

const invoke = async (scope, fnId) => {
  const response = await fetch(`http://127.0.0.1:${process.env.SIM_PORT}/api/scopes/${scope}/invoke`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ fnId, args: { x: 3 } }),
  });
  return { status: response.status, body: await response.json() };
};

(async () => {
  const alphaEntry = nameOf(alpha, 'alphaEntry');
  const betaEntry = nameOf(beta, 'betaEntry');

  const alphaRun = await invoke('alpha', alphaEntry.id);
  if (alphaRun.status !== 200 || alphaRun.body.ok !== true) fail('alpha invoke must succeed');
  if (!alphaRun.body.trace || typeof alphaRun.body.trace !== 'object') fail('alpha invoke must return a trace');
  if (typeof alphaRun.body.durationMs !== 'number' || alphaRun.body.durationMs < 0) {
    fail('alpha invoke must report a real duration');
  }

  const betaRun = await invoke('beta', betaEntry.id);
  if (betaRun.status !== 200 || betaRun.body.ok !== true) fail('beta invoke must succeed');

  const foreign = await invoke('alpha', betaEntry.id);
  if (foreign.status === 200 && foreign.body.ok === true) {
    fail('alpha must not execute an unknown beta id');
  }

  console.log('WORKSPACE_SIM_OK');
})().catch((error) => {
  console.error(`WORKSPACE_SIM_FAIL: ${error.message}`);
  process.exit(1);
});
NODE

node scripts/check-ui-contract.mjs "$TMP_DIR/alpha.json"

echo "[simulate] server and manifests cleaned up by the EXIT trap"
