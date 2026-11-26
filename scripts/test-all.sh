#!/usr/bin/env bash
# Orchestrator gate: format, workspace tests, release build, fixture scan, server smoke.
# Usage: bash scripts/test-all.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=repo-root.sh
source "$SCRIPT_DIR/repo-root.sh"

cd "$REPO_ROOT"

echo "[test-all] cargo fmt --all -- --check"
cargo fmt --all -- --check

echo "[test-all] cargo test --workspace"
cargo test --workspace

echo "[test-all] ui/build.sh"
bash "$REPO_ROOT/ui/build.sh"

echo "[test-all] UI checks"
node "$REPO_ROOT/ui/scripts/check-ui.mjs"
node "$REPO_ROOT/ui/scripts/check-ui.mjs" --self-test
(cd "$REPO_ROOT/ui" && npm test)

echo "[test-all] cargo build --release -p fnscope"
cargo build --release -p fnscope

echo "[test-all] scan-fixture.sh"
bash "$REPO_ROOT/scripts/scan-fixture.sh"

echo "[test-all] smoke-server.sh"
bash "$REPO_ROOT/scripts/smoke-server.sh"

echo "ALL_TESTS_OK"
