#!/usr/bin/env bash
# Produce the static assets the Rust server embeds with include_str!.
# Everything here is build-time only; the runtime stays Rust-only.

# Fail fast: a half-written dist/ asset would silently ship a broken UI.
set -euo pipefail

# Resolve this script's own directory so the build works from WSL, from
# PowerShell via `wsl.exe -d Debian --exec bash .../ui/build.sh`, or from any
# working directory. No drive letters or user paths are hardcoded.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# dist/ holds the committed artifacts; recreate it if a checkout lacks it.
mkdir -p dist

# Dependencies are needed only to produce dist/. Install once when absent.
if [ ! -d node_modules ]; then
  npm install --no-audit --no-fund
fi

# Keep the served HTML in sync with the source template.
cp index.html dist/index.html
# Pre-paint palette bootstrap; external so the strict CSP needs no inline hash.
cp src/theme.js dist/theme.js

# Tailwind scans src/**/*.{ts,tsx} and emits a minified stylesheet containing
# only the utility classes the explorer actually uses.
./node_modules/.bin/tailwindcss -i ./src/input.css -o ./dist/app.css --minify

# esbuild bundles React, ReactDOM, lucide-react and the explorer into one ESM
# file that the browser loads directly from the Rust server.
./node_modules/.bin/esbuild src/main.tsx \
  --bundle \
  --minify \
  --format=esm \
  --jsx=automatic \
  --define:process.env.NODE_ENV='"production"' \
  --outfile=dist/app.js

# Report exact sizes so CI (and the orchestrator) can assert real bundles.
wc -c dist/index.html dist/theme.js dist/app.js dist/app.css
