#!/usr/bin/env bash
# POST-IMPL / CANONICAL (reconstructed 2026-11-26)
#
# Canonical CLI simulation for the fnscope-rust-backend pack. Reconstructed
# from the committed origin record; it is not a backdated artifact and it does
# not replace the real gates quoted in .reports/goal-build-fnscope.md
# iterations 15-20.
#
# Scenarios (see PRD-TRD-fnscope-rust-backend.md "Simulation contract"):
#   S1 scan tests/fixtures/rust_app -> manifest with at least 7 functions
#   S2 dotted method id: name=scale, id=calculator-scale, call -> add
#   S3 classify errors contain EMPTY_TEXT
#   S4 report purity=side-effect and permission console:out
#   S5 no dangling call edges (scripts/check-manifest.mjs)
#   one PASS/FAIL line per scenario, then a final verdict marker
#
# It uses the real release/debug binary when one is present and otherwise runs
# a documented placeholder mode that prints SKIP, never a fabricated PASS.
# It never runs cargo and starts no server. Scratch files live outside the
# repo and are removed by a trap. Usage:
#   bash docs/features/fnscope-rust-backend/simulate.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
FIXTURE_DIR="$REPO_ROOT/tests/fixtures/rust_app"
CHECKER="$REPO_ROOT/scripts/check-manifest.mjs"

SIM_TMP=""
MANIFEST=""
RESULT=0

pass() { printf '[PASS] %s\n' "$1"; }
fail() { printf '[FAIL] %s : %s\n' "$1" "$2"; RESULT=1; }
skip() { printf '[SKIP-PLACEHOLDER] %s : %s\n' "$1" "$2"; }

placeholder() {
  local reason="$1"
  echo "[canonical-sim] placeholder mode: $reason"
  echo "[canonical-sim] no binary was executed; these lines are not verification."
  echo "[canonical-sim] recorded real results live in the goal ledger and IMPL.md."
  skip "S1 scan -> at least 7 functions" "$reason"
  skip "S2 dotted method id and resolved call" "$reason"
  skip "S3 EMPTY_TEXT error" "$reason"
  skip "S4 side-effect permission" "$reason"
  skip "S5 no dangling call edges" "$reason"
  echo "CANONICAL_SIM_PLACEHOLDER"
}

cleanup() {
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

# Run a node assertion over the manifest. The script prints "OK ..." on
# success and a diagnostic on failure; both are echoed as local evidence.
check_node() {
  local label="$1" script="$2" out
  if ! out="$(node -e "$script" "$MANIFEST" 2>&1)"; then
    fail "$label" "$out"
  elif [[ "$out" == OK* ]]; then
    printf '[canonical-sim] %s\n' "$out"
    pass "$label"
  else
    fail "$label" "$out"
  fi
}

# ---------------------------------------------------------------- preflight
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

SIM_TMP="$(mktemp -d)"
MANIFEST="$SIM_TMP/fn-graph.json"

echo "[canonical-sim] using binary: $BIN"
echo "[canonical-sim] fixture: $FIXTURE_DIR"
echo "[canonical-sim] scratch dir: $SIM_TMP (outside the repo)"

# ------------------------------------- S1: scan fixture, >=7 functions
echo "[canonical-sim] S1 scan tests/fixtures/rust_app -> at least 7 functions"
SCAN_OK=0
if ! "$BIN" scan "$FIXTURE_DIR" --out "$SIM_TMP" >"$SIM_TMP/scan.log" 2>&1; then
  fail "S1 scan -> at least 7 functions" "fnscope scan exited non-zero; see $SIM_TMP/scan.log"
elif [[ ! -f "$MANIFEST" ]]; then
  fail "S1 scan -> at least 7 functions" "expected manifest was not written"
else
  SCAN_OK=1
  S1_SCRIPT='
const fs = require("node:fs");
const g = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
const fns = Array.isArray(g.functions) ? g.functions : [];
if (fns.length < 7) {
  console.error("expected at least 7 functions, got " + fns.length);
  process.exit(1);
}
console.log("OK " + fns.length + " functions");
'
  check_node "S1 scan -> at least 7 functions" "$S1_SCRIPT"
fi

# ------------------------ S2: dotted method id and resolved call to add
echo "[canonical-sim] S2 dotted method id and resolved call"
if [[ "$SCAN_OK" -ne 1 ]]; then
  fail "S2 dotted method id and resolved call" "no manifest from S1"
else
  S2_SCRIPT='
const fs = require("node:fs");
const g = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
const fns = Array.isArray(g.functions) ? g.functions : [];
const scale = fns.find(f => f && f.name === "scale");
if (!scale) { console.error("no function named scale"); process.exit(1); }
if (scale.id !== "calculator-scale") {
  console.error("scale id = " + JSON.stringify(scale.id));
  process.exit(1);
}
const add = fns.find(f => f && f.name === "add");
if (!add) { console.error("no function named add"); process.exit(1); }
const steps = Array.isArray(scale.steps) ? scale.steps : [];
const call = steps.find(s => s && s.kind === "call" && s.callsFnId === add.id);
if (!call) {
  console.error("no call step from " + scale.id + " to " + add.id);
  process.exit(1);
}
console.log("OK " + scale.name + " -> " + scale.id + " calls " + add.id);
'
  check_node "S2 dotted method id and resolved call" "$S2_SCRIPT"
fi

# ----------------------------------------- S3: EMPTY_TEXT error fact
echo "[canonical-sim] S3 EMPTY_TEXT error"
if [[ "$SCAN_OK" -ne 1 ]]; then
  fail "S3 EMPTY_TEXT error" "no manifest from S1"
else
  S3_SCRIPT='
const fs = require("node:fs");
const g = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
const fns = Array.isArray(g.functions) ? g.functions : [];
const classify = fns.find(f => f && f.name === "classify");
if (!classify) { console.error("no function named classify"); process.exit(1); }
const errors = Array.isArray(classify.errors) ? classify.errors : [];
if (!errors.some(e => e && e.code === "EMPTY_TEXT")) {
  console.error("classify.errors = " + JSON.stringify(errors));
  process.exit(1);
}
console.log("OK classify.errors contains EMPTY_TEXT");
'
  check_node "S3 EMPTY_TEXT error" "$S3_SCRIPT"
fi

# --------------------------------- S4: side-effect permission console:out
echo "[canonical-sim] S4 side-effect permission console:out"
if [[ "$SCAN_OK" -ne 1 ]]; then
  fail "S4 side-effect permission console:out" "no manifest from S1"
else
  S4_SCRIPT='
const fs = require("node:fs");
const g = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
const fns = Array.isArray(g.functions) ? g.functions : [];
const report = fns.find(f => f && f.name === "report");
if (!report) { console.error("no function named report"); process.exit(1); }
if (report.purity !== "side-effect") {
  console.error("report.purity = " + JSON.stringify(report.purity));
  process.exit(1);
}
const perms = Array.isArray(report.permissions) ? report.permissions : [];
if (!perms.includes("console:out")) {
  console.error("report.permissions = " + JSON.stringify(perms));
  process.exit(1);
}
console.log("OK report.purity=side-effect and console:out");
'
  check_node "S4 side-effect permission console:out" "$S4_SCRIPT"
fi

# --------------------------------------- S5: no dangling call edges
echo "[canonical-sim] S5 no dangling call edges"
if [[ "$SCAN_OK" -ne 1 ]]; then
  fail "S5 no dangling call edges" "no manifest from S1"
elif ! POLICY_OUT="$(node "$CHECKER" "$MANIFEST" 2>&1)"; then
  fail "S5 no dangling call edges" "$POLICY_OUT"
elif [[ "$POLICY_OUT" == MANIFEST_OK* ]]; then
  printf '[canonical-sim] %s\n' "$POLICY_OUT"
  pass "S5 no dangling call edges"
else
  fail "S5 no dangling call edges" "$POLICY_OUT"
fi

# ------------------------------------------------------------- verdict
trap - EXIT
cleanup
if [[ "$RESULT" -eq 0 ]]; then
  echo "CANONICAL_SIM_OK"
else
  echo "CANONICAL_SIM_FAIL"
  exit 1
fi
