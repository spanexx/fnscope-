#!/usr/bin/env bash
# Prove a real scan of the fixture yields a valid, bidirectional, correctly filtered graph.
# Usage: bash scripts/scan-fixture.sh [output-dir]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=repo-root.sh
source "$SCRIPT_DIR/repo-root.sh"

OUT_DIR="${1:-/tmp/fnscope-e2e-$$}"
MANIFEST="$OUT_DIR/fn-graph.json"

cd "$REPO_ROOT"
trap 'rm -rf "$OUT_DIR"' EXIT
mkdir -p "$OUT_DIR"

echo "[scan-fixture] scanning fixture into $OUT_DIR"
cargo run --quiet -p fnscope -- scan "$REPO_ROOT/tests/fixtures/sample_app" \
  --out "$OUT_DIR" \
  --include '**/*.ts' --include '**/*.tsx' --include '**/*.js'

if [[ ! -f "$MANIFEST" ]]; then
  echo "scan-fixture: expected manifest at $MANIFEST was not written" >&2
  exit 1
fi

node "$REPO_ROOT/scripts/check-manifest.mjs" "$MANIFEST"

# Fixture-specific evidence: count, filtering, hub edges, error spec and side effect.
node --input-type=commonjs -e '
const fs = require("node:fs");
const graph = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
const fns = Array.isArray(graph.functions) ? graph.functions : [];
const problems = [];
const fail = (m) => problems.push(m);

if (fns.length < 8) fail("expected at least 8 functions, got " + fns.length);

const ids = new Set(fns.map((f) => String(f && f.id)));
for (const f of fns) {
  const haystack = String(f && f.id || "") + " " + String(f && f.filePath || "");
  if (/roles[.\-]test|ignored[.\-]generated/i.test(haystack)) {
    fail("excluded or generated source leaked into graph: " + haystack.trim());
  }
}

const byName = (name) => fns.find((f) => f && f.name === name);
const hub = byName("evaluateGrammarRole");
const grading = byName("gradeSentenceExercise");
if (!hub) fail("missing function evaluateGrammarRole");
if (!grading) fail("missing function gradeSentenceExercise");

if (hub && grading) {
  const callers = Array.isArray(hub.calledBy) ? hub.calledBy : [];
  const linked = callers.some((c) => c && c.fnId != null &&
    (c.fnId === grading.id || c.name === "gradeSentenceExercise"));
  if (!linked) fail("evaluateGrammarRole has no calledBy entry pointing at gradeSentenceExercise");

  const steps = Array.isArray(hub.steps) ? hub.steps : [];
  const calls = steps.filter((s) => s && s.callsFnId != null);
  if (calls.length === 0) fail("evaluateGrammarRole has no callsFnId values");
  for (const step of calls) {
    if (!ids.has(step.callsFnId)) fail("unresolved callsFnId on evaluateGrammarRole: " + step.callsFnId);
  }
}

const hasTokenError = fns.some((f) => Array.isArray(f.errors) && f.errors.some(
  (e) => e && e.code === "TOKEN_NOT_FOUND" && e.status === 422));
if (!hasTokenError) fail("no function declares TOKEN_NOT_FOUND with status 422");

const hook = byName("dispatchWebhookEvent");
if (!hook) {
  fail("missing function dispatchWebhookEvent");
} else {
  if (hook.purity !== "side-effect") fail("dispatchWebhookEvent purity must be side-effect");
  const permissions = Array.isArray(hook.permissions) ? hook.permissions : [];
  if (!permissions.includes("net:outbound")) fail("dispatchWebhookEvent missing net:outbound permission");
  if (!permissions.some((p) => typeof p === "string" && p.startsWith("secrets:read:"))) {
    fail("dispatchWebhookEvent missing a secrets:read:<NAME> permission");
  }
}

if (problems.length > 0) {
  for (const problem of problems) console.error("FIXTURE_SCAN_FAIL: " + problem);
  process.exit(1);
}
' "$MANIFEST"

rm -rf "$OUT_DIR"
trap - EXIT
echo "FIXTURE_SCAN_OK"
