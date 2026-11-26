#!/usr/bin/env bash
# Validate the two-scope workspace fixture without a server, CLI build or third-party package.
# The fixture is pure TypeScript plus a JSON config; a broken shape would silently weaken the
# scope-isolation tests that consume it. Run: bash scripts/check-workspace-fixtures.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=repo-root.sh
source "$SCRIPT_DIR/repo-root.sh"
cd "$REPO_ROOT"

FIXTURE_DIR="tests/fixtures/workspace"
CONFIG="$FIXTURE_DIR/fnscope.workspace.json"
ALPHA_SRC="$FIXTURE_DIR/alpha/src/main.ts"
BETA_SRC="$FIXTURE_DIR/beta/src/main.ts"
README="$FIXTURE_DIR/README.md"

fail() {
  printf 'WORKSPACE_FIXTURE_FAIL: %s\n' "$1" >&2
  exit 1
}

# Existence first, so later greps fail with a useful message rather than a bare exit code.
for file in "$CONFIG" "$ALPHA_SRC" "$BETA_SRC" "$README"; do
  [[ -f "$file" ]] || fail "missing $file"
done

# JSON syntax and the structural invariants the server/CLI will rely on. Node's parser is
# standard library only; no server code, no crates, no npm packages run here.
node --input-type=module -e '
import { readFileSync } from "node:fs";

const configPath = process.argv[1];
const fail = (message) => {
  console.error("WORKSPACE_FIXTURE_FAIL: " + message);
  process.exit(1);
};

let config;
try {
  config = JSON.parse(readFileSync(configPath, "utf8"));
} catch (error) {
  fail("cannot read or parse " + configPath + ": " + error.message);
}

if (config === null || typeof config !== "object" || Array.isArray(config)) {
  fail("workspace config must be a JSON object");
}
if (config.version !== 1) {
  fail("version must be 1, got " + JSON.stringify(config.version));
}
if (config.defaultScope !== "alpha") {
  fail("defaultScope must be alpha, got " + JSON.stringify(config.defaultScope));
}
if (!Array.isArray(config.scopes)) {
  fail("scopes must be an array");
}
if (config.scopes.length !== 2) {
  fail("expected exactly 2 scopes, got " + config.scopes.length);
}

const ids = new Set();
for (const scope of config.scopes) {
  if (scope === null || typeof scope !== "object" || Array.isArray(scope)) {
    fail("each scope must be an object");
  }
  if (typeof scope.id !== "string" || !/^[a-z0-9][a-z0-9-]*$/.test(scope.id)) {
    fail("invalid scope id: " + JSON.stringify(scope.id));
  }
  if (ids.has(scope.id)) {
    fail("duplicate scope id: " + scope.id);
  }
  ids.add(scope.id);

  if (typeof scope.path !== "string" || scope.path.trim() === "") {
    fail("scope " + scope.id + " path must be a non-empty string");
  }
  const segments = scope.path.split(/[\\/]/);
  if (
    scope.path.startsWith("/") ||
    scope.path.startsWith("\\") ||
    /^[A-Za-z]:/.test(scope.path) ||
    segments.includes("..")
  ) {
    fail("scope " + scope.id + " path must be relative: " + JSON.stringify(scope.path));
  }
  if (!Array.isArray(scope.include) || !scope.include.includes("**/*.ts")) {
    fail("scope " + scope.id + " include must contain **/*.ts");
  }
}

if (!ids.has("alpha") || !ids.has("beta")) {
  fail("scopes must include alpha and beta");
}
' "$CONFIG"

require_pattern() {
  local file="$1" pattern="$2" label="$3"
  grep -Eq -- "$pattern" "$file" || fail "$label missing in $file"
}

# Each scope owns one entry point and one helper; the helper name is shared on purpose.
require_pattern "$ALPHA_SRC" 'export function alphaEntry\(x: number\): number' 'alphaEntry'
require_pattern "$ALPHA_SRC" 'export function shared\(x: number\): number' 'alpha shared'
require_pattern "$ALPHA_SRC" 'shared\(x\)' 'alphaEntry -> shared call'
require_pattern "$BETA_SRC" 'export function betaEntry\(x: number\): number' 'betaEntry'
require_pattern "$BETA_SRC" 'export function shared\(x: number\): number' 'beta shared'
require_pattern "$BETA_SRC" 'shared\(x\)' 'betaEntry -> shared call'

# An entry point leaking into the other scope would make the isolation test vacuous.
if grep -Eq 'betaEntry' "$ALPHA_SRC"; then
  fail "alpha source contains betaEntry"
fi
if grep -Eq 'alphaEntry' "$BETA_SRC"; then
  fail "beta source contains alphaEntry"
fi

# Same shared name, different behavior: the isolation test must be able to tell them apart.
require_pattern "$ALPHA_SRC" 'return x \* 2;' 'alpha shared behavior'
require_pattern "$BETA_SRC" 'return x \* 10;' 'beta shared behavior'

printf 'WORKSPACE_FIXTURE_OK\n'
