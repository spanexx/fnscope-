#!/usr/bin/env bash
# Resolve the repository root from this file's own location so no caller hardcodes a path.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export REPO_ROOT

# Executed directly it prints the root; sourced it only publishes REPO_ROOT.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  printf '%s\n' "$REPO_ROOT"
fi
