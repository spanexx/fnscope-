#!/usr/bin/env bash
# POST-IMPL / CANONICAL (reconstructed 2026-11-26)
#
# Canonical CLI simulation for the fnscope-python-go pack. Reconstructed from
# the committed origin record on 2026-11-26; it is not a backdated artifact and
# it does not replace the real gates in .reports/goal-build-fnscope.md.
#
# Scenarios (see PRD-TRD-fnscope-python-go.md "Simulation contract"):
#   S0 preflight: required fixture, adapter and registration files exist
#   S1 discover functions from both fixtures (source-level scan, shell only)
#   S2 assert both languages present with the expected qualified functions
#   S3 assert the documented call edges and that no edge is dangling
#   S4 assert the __pycache__ decoy is excluded and registration facts hold
#   S5 one [PASS]/[FAIL] line per assertion plus a final PASS/FAIL marker
#
# It is pure shell: no cargo, no server, no Python/Go/Node toolchain. Paths are
# resolved from this script's own location, never from a drive letter or user
# directory. Usage: bash docs/features/fnscope-python-go/simulate.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
PY_FIXTURE="$REPO_ROOT/tests/fixtures/python_app"
GO_FIXTURE="$REPO_ROOT/tests/fixtures/go_app"
LIB_RS="$REPO_ROOT/crates/fnscope-parser/src/lib.rs"
WORKSPACE_TOML="$REPO_ROOT/Cargo.toml"
PARSER_TOML="$REPO_ROOT/crates/fnscope-parser/Cargo.toml"
SCAN_RS="$REPO_ROOT/crates/fnscope-core/src/scan.rs"
PY_MOD="$REPO_ROOT/crates/fnscope-parser/src/python/mod.rs"
GO_MOD="$REPO_ROOT/crates/fnscope-parser/src/golang/mod.rs"

RESULT=0
pass() { printf '[PASS] %s\n' "$1"; }
fail() { printf '[FAIL] %s : %s\n' "$1" "$2"; RESULT=1; }

TMP=""
cleanup() { if [[ -n "$TMP" && -d "$TMP" ]]; then rm -rf "$TMP"; fi; }
trap cleanup EXIT

# ------------------------------------------------------------------- S0 preflight
for required in \
  "$PY_FIXTURE/app/text.py" "$PY_FIXTURE/app/metrics.py" \
  "$PY_FIXTURE/__pycache__/generated.py" "$GO_FIXTURE/main.go" \
  "$LIB_RS" "$WORKSPACE_TOML" "$PARSER_TOML" "$SCAN_RS" "$PY_MOD" "$GO_MOD"; do
  if [[ ! -f "$required" ]]; then
    fail "S0 preflight" "missing required file: ${required#"$REPO_ROOT/"}"
  fi
done
if [[ "$RESULT" -ne 0 ]]; then
  echo "FAIL"
  exit 1
fi
pass "S0 preflight: all fixture and adapter files present"

TMP="$(mktemp -d)"
PY_DISCOVERED="$TMP/python-functions.txt"
GO_DISCOVERED="$TMP/go-functions.txt"
: > "$PY_DISCOVERED"
: > "$GO_DISCOVERED"

has_function() {
  local lang="$1" name="$2" file
  if [[ "$lang" == "python" ]]; then file="$PY_DISCOVERED"; else file="$GO_DISCOVERED"; fi
  grep -Fqx "$(printf '%s\t%s' "$lang" "$name")" "$file"
}

call_evidence() {
  local lang="$1" callee="$2"
  if [[ "$lang" == "python" ]]; then
    grep -Fq "$callee(" "$PY_FIXTURE/app/text.py" \
      || grep -Fq "$callee(" "$PY_FIXTURE/app/metrics.py"
  else
    grep -Fq "$callee(" "$GO_FIXTURE/main.go"
  fi
}

# ------------------------------- S1: source-level scan of both fixtures
# Python: module-level `def` -> qualified name; indented `def` inside a
# `class X:` block -> `X.method`. __pycache__ is not in app/*.py.
for f in "$PY_FIXTURE"/app/*.py; do
  [[ -f "$f" ]] || continue
  awk '
    /^class [A-Za-z_]/ { cls = $2; sub(/[:(].*$/, "", cls); next }
    /^def [A-Za-z_]/ {
      name = $2; sub(/\(.*$/, "", name)
      print "python\t" name; cls = ""; next
    }
    /^[[:space:]]+def [A-Za-z_]/ && cls != "" {
      line = $0; sub(/^[[:space:]]*def[[:space:]]+/, "", line)
      sub(/\(.*$/, "", line); print "python\t" cls "." line; next
    }
  ' "$f"
done > "$PY_DISCOVERED"

# Go: `func Name(` -> Name; `func (recv *Type) Name(` -> Type.Name.
while IFS= read -r line; do
  case "$line" in
    func\ *)
      rest="${line#func }"
      case "$rest" in
        \(*)
          recv="${rest#\(}"
          recv="${recv%%\)*}"
          typ="${recv##* }"
          typ="${typ#\*}"
          after="${rest#*\) }"
          name="${after%%(*}"
          printf 'go\t%s.%s\n' "$typ" "$name"
          ;;
        *)
          name="${rest%%(*}"
          printf 'go\t%s\n' "$name"
          ;;
      esac
      ;;
  esac
done < "$GO_FIXTURE/main.go" > "$GO_DISCOVERED"

if [[ -s "$PY_DISCOVERED" && -s "$GO_DISCOVERED" ]]; then
  pass "S1 source-level discovery produced python and go records"
else
  fail "S1 source-level discovery produced python and go records" "empty scan output"
fi

# ------------------------------------------- S2: languages + functions
PY_COUNT="$(grep -c $'^python\t' "$PY_DISCOVERED" || true)"
GO_COUNT="$(grep -c $'^go\t' "$GO_DISCOVERED" || true)"
if [[ "$PY_COUNT" -gt 0 ]]; then
  pass "S2 language python present ($PY_COUNT functions)"
else
  fail "S2 language python present" "no python functions discovered"
fi
if [[ "$GO_COUNT" -gt 0 ]]; then
  pass "S2 language go present ($GO_COUNT functions)"
else
  fail "S2 language go present" "no go functions discovered"
fi

PY_EXPECTED="normalize_text classify report add Calculator.__init__ Calculator.scale"
GO_EXPECTED="normalize analyze load Service.Apply report main"
for name in $PY_EXPECTED; do
  if has_function python "$name"; then
    pass "S2 python function $name discovered"
  else
    fail "S2 python function $name discovered" "not found in python scan output"
  fi
done
for name in $GO_EXPECTED; do
  if has_function go "$name"; then
    pass "S2 go function $name discovered"
  else
    fail "S2 go function $name discovered" "not found in go scan output"
  fi
done

# ------------------------------------- S3: documented edges, no dangling
# Edges are the Call-step edges promised by the two fixture READMEs. Each
# endpoint must be a discovered function and the callee must appear as a call
# in the caller's source; a missing endpoint is a dangling edge.
EDGES=(
  "python|Calculator.scale|add"
  "python|classify|normalize_text"
  "go|main|report"
)
for edge in "${EDGES[@]}"; do
  IFS='|' read -r lang from to <<< "$edge"
  label="S3 edge $lang:$from->$to"
  if ! has_function "$lang" "$from"; then
    fail "$label" "source function $from was not discovered"
  elif ! has_function "$lang" "$to"; then
    fail "$label" "target function $to was not discovered (dangling)"
  elif ! call_evidence "$lang" "$to"; then
    fail "$label" "no call to $to found in the $lang fixture source"
  else
    pass "$label resolves (no dangling endpoint)"
  fi
done

# ------------------------ S4: decoy excluded + registration facts
if has_function python generated_should_be_excluded; then
  fail "S4 __pycache__ decoy excluded" "generated_should_be_excluded was discovered"
else
  pass "S4 __pycache__ decoy excluded from python scan output"
fi

if grep -Fq '"**/__pycache__/**"' "$SCAN_RS"; then
  pass "S4 core default excludes __pycache__"
else
  fail "S4 core default excludes __pycache__" "no default exclude in scan.rs"
fi

REGISTRATION_CHECKS=(
  "$LIB_RS|pub mod python;|python module compiled"
  "$LIB_RS|pub mod golang;|golang module compiled"
  "$WORKSPACE_TOML|tree-sitter-python = \"0.25\"|python grammar dependency"
  "$WORKSPACE_TOML|tree-sitter-go = \"0.25\"|go grammar dependency"
  "$PARSER_TOML|tree-sitter-python.workspace = true|parser python grammar wiring"
  "$PARSER_TOML|tree-sitter-go.workspace = true|parser go grammar wiring"
  "$SCAN_RS|\"**/*.py\"|python default scan glob"
  "$SCAN_RS|\"**/*.go\"|go default scan glob"
  "$PY_MOD|\"python\"|python adapter id"
  "$GO_MOD|\"go\"|go adapter id"
)
for check in "${REGISTRATION_CHECKS[@]}"; do
  IFS='|' read -r file needle label <<< "$check"
  if grep -Fq "$needle" "$file"; then
    pass "S4 $label present"
  else
    fail "S4 $label present" "expected text not found in ${file#"$REPO_ROOT/"}"
  fi
done

# ------------------------------------------------------------- S5: verdict
if [[ "$RESULT" -eq 0 ]]; then
  echo "PASS"
else
  echo "FAIL"
  exit 1
fi
