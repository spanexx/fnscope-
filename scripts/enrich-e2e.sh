#!/usr/bin/env bash
# End-to-end proof that `fnscope scan --enrich` uses a content-addressed cache.
# The first scan is all cache misses, the second makes ZERO API calls, formatting-only
# edits stay at zero, and a changed string literal forces a new API call.
#
# NOTE: ureq follows FNSCOPE_LLM_BASE_URL for the completion endpoint. Pointing it
# at an http://127.0.0.1 URL is intentional here: the mock LLM is a local HTTP
# server, not a real TLS provider, and no real model is contacted.
# Usage: bash scripts/enrich-e2e.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=repo-root.sh
source "$SCRIPT_DIR/repo-root.sh"

TMP_DIR="/tmp/fnscope-enrich-$$"
WORK_SRC="$TMP_DIR/src"
OUT_DIR="$TMP_DIR/out"
GRAPH="$OUT_DIR/fn-graph.json"
CACHE="$OUT_DIR/enrich-cache.json"
MOCK_SCRIPT="$REPO_ROOT/tests/e2e/mock-llm.mjs"
BIN="$REPO_ROOT/target/release/fnscope"

PORT_FILE="$TMP_DIR/mock.port"
COUNT_FILE="$TMP_DIR/mock.count"
MOCK_LOG="$TMP_DIR/mock.log"
MOCK_STDOUT="$TMP_DIR/mock.stdout.log"
PIDFILE="$TMP_DIR/mock.pid"

VERDICT=0
PORT=""

cd "$REPO_ROOT"

# Never delete a .fnscope directory that existed before this script ran.
FNSCOPE_AT_ROOT=0
FNSCOPE_AT_FIXTURE=0
if [[ -e "$REPO_ROOT/.fnscope" ]]; then FNSCOPE_AT_ROOT=1; fi
if [[ -e "$REPO_ROOT/tests/fixtures/sample_app/.fnscope" ]]; then FNSCOPE_AT_FIXTURE=1; fi

# Stop descendants before the tracked parent, mirroring scripts/smoke-server.sh.
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

stop_mock() {
  local pid="" i=0
  if [[ -f "$PIDFILE" ]]; then
    pid="$(cat "$PIDFILE" 2>/dev/null || true)"
  fi
  if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
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

dump_logs() {
  echo "----- mock request log (tail) -----" >&2
  tail -n 40 "$MOCK_LOG" 2>/dev/null || true
  echo "----- mock stdout (tail) -----" >&2
  tail -n 40 "$MOCK_STDOUT" 2>/dev/null || true
  local log
  for log in "$TMP_DIR"/cli-*.log; do
    [[ -f "$log" ]] || continue
    echo "----- fnscope log $(basename "$log") (tail) -----" >&2
    tail -n 20 "$log" 2>/dev/null || true
  done
}

on_exit() {
  local rc=$?
  stop_mock || true
  if [[ "$VERDICT" -eq 0 ]]; then
    echo "ENRICH_E2E_FAIL: unexpected error (exit code $rc)" >&2
    dump_logs
  fi
}
trap on_exit EXIT

fail() {
  VERDICT=2
  echo "ENRICH_E2E_FAIL: $1" >&2
  dump_logs
  exit 1
}

# GET /count is the authoritative call counter: no competing file reader.
get_count() {
  local value
  if ! value="$(curl -fsS --max-time 5 "http://127.0.0.1:${PORT}/count" 2>/dev/null)"; then
    return 1
  fi
  if [[ ! "$value" =~ ^[0-9]+$ ]]; then
    return 1
  fi
  printf '%s\n' "$value"
}

# One CLI invocation; each run keeps its own stdout for report assertions.
scan_once() {
  local log="$1"
  env \
    FNSCOPE_LLM_BASE_URL="http://127.0.0.1:${PORT}/v1/chat/completions" \
    OPENAI_API_KEY="e2e-fake-key" \
    NO_PROXY="127.0.0.1,localhost" \
    no_proxy="127.0.0.1,localhost" \
    "$BIN" scan "$WORK_SRC" --out "$OUT_DIR" --enrich --model e2e-test-model \
    >"$log" 2>&1
}

mkdir -p "$TMP_DIR"

echo "[enrich-e2e] preparing isolated source copy in $TMP_DIR"
cp -R "$REPO_ROOT/tests/fixtures/sample_app/src" "$WORK_SRC"
printf '%s\n' '{"name":"fnscope-enrich-e2e","private":true,"type":"module"}' \
  >"$TMP_DIR/package.json"

if [[ ! -x "$BIN" ]]; then
  echo "[enrich-e2e] release binary missing; building it"
  if command -v timeout >/dev/null 2>&1; then
    if ! timeout 900 cargo build --release -p fnscope >"$TMP_DIR/build.log" 2>&1; then
      fail "cargo build --release -p fnscope failed"
    fi
  elif ! cargo build --release -p fnscope >"$TMP_DIR/build.log" 2>&1; then
    fail "cargo build --release -p fnscope failed"
  fi
fi
[[ -x "$BIN" ]] || fail "fnscope binary not found at $BIN"

echo "[enrich-e2e] starting mock LLM on an OS-assigned localhost port"
# Port 0 asks the OS for a free port; the mock publishes the real port to PORT_FILE.
node "$MOCK_SCRIPT" 0 "$COUNT_FILE" "$MOCK_LOG" "$PORT_FILE" \
  >"$MOCK_STDOUT" 2>&1 &
echo $! >"$PIDFILE"

ready=0
for ((attempt = 1; attempt <= 40; attempt++)); do
  if [[ -s "$PORT_FILE" ]]; then
    ready=1
    break
  fi
  sleep 0.25
done
if [[ "$ready" -ne 1 ]]; then
  fail "mock LLM did not publish a port after 40 attempts"
fi
PORT="$(cat "$PORT_FILE")"
if [[ ! "$PORT" =~ ^[0-9]+$ ]]; then
  fail "mock LLM published an invalid port: $PORT"
fi

ready=0
for ((attempt = 1; attempt <= 40; attempt++)); do
  if get_count >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 0.25
done
if [[ "$ready" -ne 1 ]]; then
  fail "mock LLM did not answer GET /count after 40 attempts"
fi

# ---------------------------------------------------------------- first scan
echo "[enrich-e2e] first scan: every function is a cache miss"
if ! scan_once "$TMP_DIR/cli-1.log"; then
  fail "first scan failed"
fi
if ! FIRST_COUNT="$(get_count)"; then
  fail "could not read mock call count after first scan"
fi

FN_COUNT="$(node --input-type=commonjs -e '
const fs = require("node:fs");
const graph = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
process.stdout.write(String(Array.isArray(graph.functions) ? graph.functions.length : 0));
' "$GRAPH")"
if [[ ! "$FN_COUNT" =~ ^[0-9]+$ ]] || [[ "$FN_COUNT" -lt 1 ]]; then
  fail "first scan produced no functions"
fi

if ! node --input-type=commonjs -e '
const fs = require("node:fs");
const graphPath = process.argv[1];
const cachePath = process.argv[2];
const expectedCalls = Number(process.argv[3]);
const problems = [];
const fail = (message) => problems.push(message);

let graph;
let cache;
try {
  graph = JSON.parse(fs.readFileSync(graphPath, "utf8"));
} catch (error) {
  fail("fn-graph.json is not valid JSON: " + error.message);
}
try {
  cache = JSON.parse(fs.readFileSync(cachePath, "utf8"));
} catch (error) {
  fail("enrich-cache.json is not valid JSON: " + error.message);
}

const functions = graph && Array.isArray(graph.functions) ? graph.functions : [];
const entries = cache && cache.entries && typeof cache.entries === "object" &&
  !Array.isArray(cache.entries) ? Object.keys(cache.entries) : [];
if (functions.length === 0) fail("graph has no functions");
if (entries.length === 0) fail("enrich cache has no entries");
if (entries.length !== functions.length) {
  fail("cache entries " + entries.length + " != functions " + functions.length);
}
if (expectedCalls !== functions.length) {
  fail("mock calls " + expectedCalls + " != functions " + functions.length);
}
for (const fn of functions) {
  if (!fn || typeof fn.description !== "string" ||
      !fn.description.includes("Mock-enriched")) {
    fail("function " + String(fn && fn.id) + " was not enriched by the mock");
  }
  const steps = Array.isArray(fn.steps) ? fn.steps : [];
  for (const step of steps) {
    if (!step || typeof step.title !== "string" || !step.title.includes("Mock title")) {
      fail("step title missing on " + String(fn && fn.id));
    }
    if (!step || typeof step.detail !== "string" || !step.detail.includes("Mock detail")) {
      fail("step detail missing on " + String(fn && fn.id));
    }
  }
}

if (problems.length > 0) {
  for (const problem of problems) console.error("E2E_ASSERT_FAIL: " + problem);
  process.exit(1);
}
console.log("E2E_ASSERT_OK: " + functions.length + " functions, " + expectedCalls + " mock calls");
' "$GRAPH" "$CACHE" "$FIRST_COUNT"; then
  fail "first-run graph/cache/enrichment assertions failed"
fi
if ! grep -Fq "enrich: 0 cache hits, ${FN_COUNT} misses, ${FN_COUNT} API calls" \
  "$TMP_DIR/cli-1.log"; then
  fail "first scan did not report all ${FN_COUNT} calls"
fi
cp "$CACHE" "$TMP_DIR/cache-baseline.json"

# ------------------------------------------------------- unchanged second scan
echo "[enrich-e2e] second scan: all cache hits, zero API calls"
if ! scan_once "$TMP_DIR/cli-2.log"; then
  fail "second scan failed"
fi
if ! SECOND_COUNT="$(get_count)"; then
  fail "could not read mock call count after second scan"
fi
if [[ "$SECOND_COUNT" -ne "$FIRST_COUNT" ]]; then
  fail "second scan made $((SECOND_COUNT - FIRST_COUNT)) extra API calls"
fi
if ! cmp -s "$CACHE" "$TMP_DIR/cache-baseline.json"; then
  fail "an all-hit run rewrote the cache file"
fi
if ! grep -Fq "enrich: ${FN_COUNT} cache hits, 0 misses, 0 API calls" \
  "$TMP_DIR/cli-2.log"; then
  fail "second scan did not report ${FN_COUNT} cache hits and 0 API calls"
fi

# -------------------------------------------------------- formatting-only edit
echo "[enrich-e2e] formatting-only edit: hashes must stay identical"
if ! node --input-type=commonjs -e '
const fs = require("node:fs");
const path = process.argv[1];
let source = fs.readFileSync(path, "utf8");
const marker = "  const tokens: Token[] = tokenize(input.sentence);";
if (!source.includes(marker)) {
  console.error("E2E_FORMAT_FAIL: expected source line is missing");
  process.exit(1);
}
source = source.replace(
  marker,
  "\n\n    const tokens: Token[] = tokenize(input.sentence);   "
);
fs.writeFileSync(path, source, "utf8");
' "$WORK_SRC/nlp/roles.ts"; then
  fail "could not apply the formatting-only edit"
fi

if ! scan_once "$TMP_DIR/cli-3.log"; then
  fail "third scan (formatting only) failed"
fi
if ! THIRD_COUNT="$(get_count)"; then
  fail "could not read mock call count after formatting-only edit"
fi
if [[ "$THIRD_COUNT" -ne "$FIRST_COUNT" ]]; then
  fail "formatting-only edit caused $((THIRD_COUNT - FIRST_COUNT)) extra API calls"
fi
if ! cmp -s "$CACHE" "$TMP_DIR/cache-baseline.json"; then
  fail "formatting-only edit rewrote the cache file"
fi
if ! grep -Fq "enrich: ${FN_COUNT} cache hits, 0 misses, 0 API calls" \
  "$TMP_DIR/cli-3.log"; then
  fail "formatting-only scan did not report zero API calls"
fi

# ------------------------------------------------------------ semantic edit
echo "[enrich-e2e] changed string literal: a new hash must cause a new call"
if ! node --input-type=commonjs -e '
const fs = require("node:fs");
const path = process.argv[1];
let source = fs.readFileSync(path, "utf8");
if (!source.includes("EMPTY_SENTENCE")) {
  console.error("E2E_SEMANTIC_FAIL: expected string literal is missing");
  process.exit(1);
}
source = source.replace("EMPTY_SENTENCE", "EMPTY_SENTENCE_V2");
fs.writeFileSync(path, source, "utf8");
' "$WORK_SRC/nlp/roles.ts"; then
  fail "could not apply the semantic edit"
fi

if ! scan_once "$TMP_DIR/cli-4.log"; then
  fail "fourth scan (semantic edit) failed"
fi
if ! FOURTH_COUNT="$(get_count)"; then
  fail "could not read mock call count after semantic edit"
fi
if [[ "$FOURTH_COUNT" -le "$FIRST_COUNT" ]]; then
  fail "semantic edit did not increase the mock call count"
fi
if cmp -s "$CACHE" "$TMP_DIR/cache-baseline.json"; then
  fail "semantic edit did not add a new cache entry"
fi

# Clean up before the verdict so success always means no mock server is left behind.
stop_mock
trap - EXIT
VERDICT=1
echo "ENRICH_E2E_OK"
