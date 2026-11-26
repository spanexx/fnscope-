#!/usr/bin/env bash
# POST-IMPL / CANONICAL (reconstructed 2026-11-26)
#
# Canonical CLI simulation for the fnscope-base pack. Reconstructed from the
# committed origin record on 2026-11-26; it is not a backdated artifact and it
# does not replace the recorded gates in VERIFY.md.
#
# Scenarios (see PRD-TRD.md "Simulation contract"):
#   S1 scan fixture -> manifest valid
#   S2 serve: health + index + manifest + invoke
#   S3 policy: no dangling call edges
#   S4 one PASS/FAIL line per scenario and a final verdict marker
#
# It uses the real release/debug binary when one is present and otherwise runs
# a documented placeholder mode that prints SKIP, never a fabricated PASS.
# It never runs cargo. Every wait is bounded; any server it starts is stopped
# through a tracked PID file. Usage: bash docs/features/fnscope-base/simulate.sh [base-port]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
FIXTURE_DIR="$REPO_ROOT/tests/fixtures/sample_app/src"
CHECKER="$REPO_ROOT/scripts/check-manifest.mjs"
BASE_PORT="${1:-4310}"

SIM_TMP=""
PIDFILE=""
SERVER_LOG=""
RESULT=0

pass() { printf '[PASS] %s\n' "$1"; }
fail() { printf '[FAIL] %s : %s\n' "$1" "$2"; RESULT=1; }
skip() { printf '[SKIP-PLACEHOLDER] %s : %s\n' "$1" "$2"; }

placeholder() {
  local reason="$1"
  echo "[canonical-sim] placeholder mode: $reason"
  echo "[canonical-sim] no binary was executed; these lines are not verification."
  echo "[canonical-sim] recorded real results live in VERIFY.md and the ledger."
  skip "S1 scan fixture -> manifest valid" "$reason"
  skip "S2 serve: health + manifest + invoke" "$reason"
  skip "S3 policy: no dangling call edges" "$reason"
  skip "S4 verdicts" "$reason"
  echo "CANONICAL_SIM_PLACEHOLDER"
}

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
  if [[ -n "$PIDFILE" && -f "$PIDFILE" ]]; then
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
}

cleanup() {
  stop_server || true
  if [[ -n "$SIM_TMP" && -d "$SIM_TMP" ]]; then
    rm -rf "$SIM_TMP"
  fi
}
trap cleanup EXIT

pick_binary() {
  local candidate
  for candidate in \
    "$REPO_ROOT/target/release/fnscope" \
    "$REPO_ROOT/target/debug/fnscope" \
    "$REPO_ROOT/target/release/fnscope.exe" \
    "$REPO_ROOT/target/debug/fnscope.exe"; do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

pick_port() {
  local base="$1" port i
  for ((i = 0; i < 16; i++)); do
    port=$((base + i))
    if ! curl -fsS --max-time 1 "http://127.0.0.1:${port}/api/health" >/dev/null 2>&1; then
      printf '%s\n' "$port"
      return 0
    fi
  done
  return 1
}

# ---------------------------------------------------------------- preflight
if ! command -v curl >/dev/null 2>&1; then
  placeholder "curl is not available"
  exit 0
fi
if ! command -v node >/dev/null 2>&1; then
  placeholder "node is not available for manifest parsing"
  exit 0
fi
if [[ ! -f "$CHECKER" ]]; then
  placeholder "scripts/check-manifest.mjs is missing"
  exit 0
fi
if [[ ! -d "$FIXTURE_DIR" ]]; then
  placeholder "fixture directory is missing: $FIXTURE_DIR"
  exit 0
fi
if ! BIN="$(pick_binary)"; then
  placeholder "no built fnscope binary in target/release or target/debug"
  exit 0
fi
if ! PORT="$(pick_port "$BASE_PORT")"; then
  echo "FAIL S4 verdicts : no free port in range $BASE_PORT..$((BASE_PORT + 15))"
  echo "CANONICAL_SIM_FAIL"
  exit 1
fi
BASE="http://127.0.0.1:${PORT}"

SIM_TMP="$(mktemp -d)"
PIDFILE="$SIM_TMP/server.pid"
SERVER_LOG="$SIM_TMP/server.log"
MANIFEST="$SIM_TMP/fn-graph.json"

echo "[canonical-sim] using binary: $BIN"
echo "[canonical-sim] scratch dir: $SIM_TMP (outside the repo)"

# ------------------------------------------- S1: scan fixture -> manifest valid
echo "[canonical-sim] S1 scan fixture -> manifest valid"
if ! "$BIN" scan "$FIXTURE_DIR" --out "$SIM_TMP" >"$SIM_TMP/scan.log" 2>&1; then
  fail "S1 scan fixture -> manifest valid" "fnscope scan exited non-zero; see $SIM_TMP/scan.log"
else
  if [[ ! -f "$MANIFEST" ]]; then
    fail "S1 scan fixture -> manifest valid" "expected manifest was not written"
  elif ! CHECK_OUT="$(node "$CHECKER" "$MANIFEST" 2>&1)"; then
    fail "S1 scan fixture -> manifest valid" "$CHECK_OUT"
  else
    echo "[canonical-sim] $CHECK_OUT"
    pass "S1 scan fixture -> manifest valid"
  fi
fi

# ---------------------------------- S2: serve health + manifest + invoke
echo "[canonical-sim] S2 serve: health + index + manifest + invoke"
S2_ERRORS=""
(
  cd "$SIM_TMP" || exit 1
  "$BIN" open "$FIXTURE_DIR" --port "$PORT" --host 127.0.0.1 --no-watch --invoke simulate
) >"$SERVER_LOG" 2>&1 &
echo $! >"$PIDFILE"

ready=0
for ((attempt = 1; attempt <= 40; attempt++)); do
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
  S2_ERRORS="server did not become healthy within 40 attempts (10 s)"
else
  if ! curl -fsS --max-time 5 "$BASE/api/health" -o "$SIM_TMP/health.json"; then
    S2_ERRORS+=" GET /api/health failed;"
  elif ! grep -Eq '"tool"[[:space:]]*:[[:space:]]*"fnscope"' "$SIM_TMP/health.json"; then
    S2_ERRORS+=" health response lacks tool=fnscope;"
  fi

  if ! curl -fsS --max-time 5 "$BASE/" -o "$SIM_TMP/index.html"; then
    S2_ERRORS+=" GET / failed;"
  elif ! grep -q 'id="root"' "$SIM_TMP/index.html"; then
    S2_ERRORS+=" index HTML lacks the explorer root;"
  fi

  if ! curl -fsS --max-time 5 "$BASE/api/fn-graph.json" -o "$SIM_TMP/served-graph.json"; then
    S2_ERRORS+=" GET /api/fn-graph.json failed;"
  elif ! SERVED_OUT="$(node "$CHECKER" "$SIM_TMP/served-graph.json" 2>&1)"; then
    S2_ERRORS+=" served manifest invalid: $SERVED_OUT;"
  fi

  FIRST_ID=""
  if [[ -f "$SIM_TMP/served-graph.json" ]]; then
    if ! FIRST_ID="$(node -e '
      const fs = require("node:fs");
      const graph = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
      const fns = Array.isArray(graph.functions) ? graph.functions : [];
      process.stdout.write(fns.length > 0 ? String(fns[0].id) : "");
    ' "$SIM_TMP/served-graph.json" 2>/dev/null)"; then
      FIRST_ID=""
    fi
  fi
  if [[ -z "$FIRST_ID" ]]; then
    S2_ERRORS+=" no function id in served manifest;"
  else
    printf '{"fnId":"%s","args":{}}\n' "$FIRST_ID" >"$SIM_TMP/invoke-request.json"
    if ! curl -fsS --max-time 5 -H "Content-Type: application/json" \
      --data-binary @"$SIM_TMP/invoke-request.json" \
      "$BASE/api/invoke" -o "$SIM_TMP/invoke.json"; then
      S2_ERRORS+=" POST /api/invoke failed;"
    elif ! grep -Eq '"ok"[[:space:]]*:[[:space:]]*true' "$SIM_TMP/invoke.json"; then
      S2_ERRORS+=" invoke response ok is not true;"
    elif ! grep -q '"trace"' "$SIM_TMP/invoke.json"; then
      S2_ERRORS+=" invoke response has no trace;"
    fi
  fi
fi

stop_server

if [[ -n "$S2_ERRORS" ]]; then
  fail "S2 serve: health + manifest + invoke" "$S2_ERRORS"
else
  pass "S2 serve: health + manifest + invoke"
fi

# ----------------------------------------- S3: policy, no dangling edges
echo "[canonical-sim] S3 policy: no dangling call edges"
if [[ ! -f "$MANIFEST" ]]; then
  fail "S3 policy: no dangling call edges" "scan manifest is absent"
elif ! POLICY_OUT="$(node "$CHECKER" "$MANIFEST" 2>&1)"; then
  fail "S3 policy: no dangling call edges" "$POLICY_OUT"
else
  echo "[canonical-sim] $POLICY_OUT"
  pass "S3 policy: no dangling call edges"
fi

# ------------------------------------------------------------- S4: verdict
cleanup
trap - EXIT
if [[ "$RESULT" -eq 0 ]]; then
  echo "CANONICAL_SIM_OK"
else
  echo "CANONICAL_SIM_FAIL"
  exit 1
fi
