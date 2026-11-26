#!/usr/bin/env bash
# PRE-IMPL / DESIGN-TIME SIMULATION - RECONSTRUCTED 2026-11-26 (not backdated)
#
# This file is a design-time mirror of the flow later implemented by
# docs/features/fnscope-base/simulate.sh. It was WRITTEN AFTER the build, from
# the committed origin record, to show what the planned scenarios looked like
# before a binary existed. It is explicitly reconstructed: it is not a
# contemporary artifact, it is not evidence that any gate passed, and it must
# not be quoted as a recorded verdict.
#
# All data below is placeholder data. No fnscope binary, HTTP server, cargo
# command or real fixture is used. Usage:
#   bash docs/features/fnscope-base/archive/simulate-pre.reconstructed.sh
set -euo pipefail

SIM_TMP="$(mktemp -d)"
trap 'rm -rf "$SIM_TMP"' EXIT
MANIFEST="$SIM_TMP/fn-graph.json"

# Intended manifest shape from PRD-TRD.md section 5; ids and hashes are fake.
cat >"$MANIFEST" <<'PLACEHOLDER'
{
  "tool": "fnscope",
  "schemaVersion": "1.0.0",
  "generatedAt": "PLACEHOLDER-RFC3339",
  "rootDir": "PLACEHOLDER-FIXTURE",
  "functions": [
    {
      "id": "fn-alpha",
      "name": "alpha",
      "astHash": "sha256:PLACEHOLDER-64-HEX",
      "steps": [{ "id": "step-1", "callsFnId": "fn-beta" }],
      "calledBy": []
    },
    {
      "id": "fn-beta",
      "name": "beta",
      "astHash": "sha256:PLACEHOLDER-64-HEX",
      "steps": [],
      "calledBy": [{ "fnId": "fn-alpha" }]
    }
  ]
}
PLACEHOLDER

echo "[design-sim] RECONSTRUCTED - placeholder data only, no binary executed"
echo "[design-sim] S1 intended: fnscope scan <fixture> --out <dir> writes fn-graph.json"
printf '[design-sim] S1 placeholder manifest written: %s\n' "$MANIFEST"

if grep -q '"tool": "fnscope"' "$MANIFEST" &&
   grep -q '"schemaVersion": "1.0.0"' "$MANIFEST" &&
   grep -q '"id": "fn-alpha"' "$MANIFEST"; then
  echo "[design-sim] S1 intended observation: tool, schemaVersion and functions present"
else
  echo "[design-sim] S1 placeholder data is malformed - fix this file"
  exit 1
fi

echo "[design-sim] S2 intended: fnscope open --port 4310 --no-watch --invoke simulate"
echo '[design-sim] S2 placeholder health: {"ok":true,"tool":"fnscope"}'
echo '[design-sim] S2 placeholder manifest: GET /api/fn-graph.json -> fn-graph.json'
echo '[design-sim] S2 placeholder invoke: POST {"fnId":"fn-alpha","args":{}} -> ok:true, trace:{}'

echo "[design-sim] S3 intended: every non-null call reference resolves to a known id"
if grep -q '"callsFnId": "fn-beta"' "$MANIFEST" &&
   grep -q '"id": "fn-beta"' "$MANIFEST" &&
   grep -q '"fnId": "fn-alpha"' "$MANIFEST"; then
  echo "[design-sim] S3 intended observation: placeholder edge fn-alpha -> fn-beta resolves"
else
  echo "[design-sim] S3 placeholder edge did not resolve - fix this file"
  exit 1
fi

echo "[design-sim] S4 intended: one PASS/FAIL line per scenario in the canonical sim"
echo "[design-sim] S4 placeholder: no verdict emitted here (design-time only)"

echo "DESIGN_SIM_COMPLETE"
echo "[design-sim] NOT A VERDICT: see the canonical simulate.sh and VERIFY.md for real results"
