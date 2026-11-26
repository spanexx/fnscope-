#!/usr/bin/env bash
# Canonical POST-IMPL simulation for the fnscope culture / reviewer pack.
# Runs the real culture checker twice and asserts the verdict markers it prints.
# No cargo, no network, no service, no repository writes, no hardcoded paths.
set -uo pipefail

PACK_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$PACK_DIR/../../.." && pwd)"
CHECKER="$REPO_ROOT/scripts/check-culture.sh"

fail() {
  printf 'FAIL: %s\n' "$1"
  exit 1
}

[[ -f "$CHECKER" ]] || fail "culture checker not found: $CHECKER"

SIM_TMP="$(mktemp -d)"
trap 'rm -rf "$SIM_TMP"' EXIT

GATE_OUT="$SIM_TMP/gate.out"
SELFTEST_OUT="$SIM_TMP/selftest.out"

bash "$CHECKER" > "$GATE_OUT" 2>&1
gate_status=$?
bash "$CHECKER" --self-test > "$SELFTEST_OUT" 2>&1
selftest_status=$?

printf '%s\n' '--- bash scripts/check-culture.sh ---'
cat "$GATE_OUT"
printf '%s\n' '--- bash scripts/check-culture.sh --self-test ---'
cat "$SELFTEST_OUT"
printf '%s\n' '--- assertions ---'

problems=0

if (( gate_status != 0 )); then
  printf 'FAIL: culture checker exit status %s (expected 0)\n' "$gate_status"
  problems=$((problems + 1))
fi
if ! grep -qxF 'CULTURE_OK' "$GATE_OUT"; then
  printf 'FAIL: no exact CULTURE_OK line in culture checker output\n'
  problems=$((problems + 1))
fi
if grep -q '^CULTURE_FAIL:' "$GATE_OUT"; then
  printf 'FAIL: culture checker printed CULTURE_FAIL\n'
  problems=$((problems + 1))
fi

if (( selftest_status != 0 )); then
  printf 'FAIL: self-test exit status %s (expected 0)\n' "$selftest_status"
  problems=$((problems + 1))
fi
if ! grep -qxF 'CULTURE_SELFTEST_OK' "$SELFTEST_OUT"; then
  printf 'FAIL: no exact CULTURE_SELFTEST_OK line in self-test output\n'
  problems=$((problems + 1))
fi
if grep -q '^CULTURE_SELFTEST_FAIL:' "$SELFTEST_OUT"; then
  printf 'FAIL: checker self-test printed CULTURE_SELFTEST_FAIL\n'
  problems=$((problems + 1))
fi

if (( problems > 0 )); then
  printf 'FAIL: %s assertion(s) did not hold\n' "$problems"
  exit 1
fi

printf 'PASS: CULTURE_OK and CULTURE_SELFTEST_OK verified; no cargo was run\n'
