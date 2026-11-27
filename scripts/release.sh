#!/usr/bin/env bash
# Bump the workspace version, commit it, and create the annotated release tag.
#
# This script never pushes. The release workflow runs only after the tag is
# pushed, so the human stays in control of publication.
#
# Usage: bash scripts/release.sh 0.1.0
set -euo pipefail

usage() {
  printf 'usage: %s X.Y.Z[-prerelease]\n' "$(basename "$0")" >&2
  exit 2
}

[[ $# -eq 1 ]] || usage
version="$1"

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  printf 'not a supported version: %s (expected X.Y.Z or X.Y.Z-prerelease)\n' "$version" >&2
  exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
cd "$repo_root"

if [[ -n "$(git status --porcelain)" ]]; then
  printf 'working tree is not clean; commit or stash first\n' >&2
  exit 1
fi

if git rev-parse -q --verify "refs/tags/v$version" >/dev/null; then
  printf 'tag v%s already exists\n' "$version" >&2
  exit 1
fi

current="$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)"
if [[ -z "$current" ]]; then
  printf 'could not read the workspace version from Cargo.toml\n' >&2
  exit 1
fi

if [[ "$version" == "$current" ]]; then
  printf 'workspace is already at %s; tagging without a version bump\n' "$version"
else
  command -v python3 >/dev/null 2>&1 || {
    printf 'python3 is required to update Cargo.toml\n' >&2
    exit 1
  }
  python3 - "$version" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path("Cargo.toml")
text = path.read_text(encoding="utf-8")
updated, count = re.subn(
    r'(?m)^version = "[^"]+"',
    f'version = "{sys.argv[1]}"',
    text,
    count=1,
)
if count != 1:
    raise SystemExit("could not find the workspace version in Cargo.toml")
path.write_text(updated, encoding="utf-8")
PY
  printf 'version %s -> %s\n' "$current" "$version"
  cargo check --workspace --quiet
  git add Cargo.toml Cargo.lock
  git commit -m "chore(release): v$version"
fi

git tag -a "v$version" -m "fnscope v$version"
branch="$(git branch --show-current)"
if [[ -z "$branch" ]]; then branch="HEAD"; fi

printf '\nCreated tag v%s on %s.\n' "$version" "$branch"
printf 'Push it to publish: git push origin %s --follow-tags\n' "$branch"
printf 'The release workflow builds Windows, macOS and Linux archives, writes SHA256SUMS, and publishes the GitHub Release.\n'
