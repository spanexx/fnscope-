#!/usr/bin/env bash
# Culture gate for the mechanically checkable rules in AGENTS.md and docs/ethics/*.
# Prints one PASS/FAIL line per check, then CULTURE_OK or CULTURE_FAIL: <reason>.
# No cargo, no network, no service starts, no long waits. Run: bash scripts/check-culture.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=repo-root.sh
source "$SCRIPT_DIR/repo-root.sh"
cd "$REPO_ROOT"

FAILURES=()
WARNINGS=()
# Kept at top level so the EXIT cleanup trap can see it after self_test returns.
SELFTEST_TMPDIR=''

record_pass() { printf '[PASS] %s\n' "$1"; }
record_fail() { FAILURES+=("$1"); printf '[FAIL] %s\n' "$1"; }
record_warn() { WARNINGS+=("$1"); printf '[WARN] %s\n' "$1"; }

# Forbidden absolute-path shapes. Each literal below is the pattern being
# detected, so each line carries a waiver explaining why it contains one.
PATH_RE='(^|[^[:alnum:]])[A-Za-z]:\\' # culture-allow: absolute path: this regex detects drive-letter paths
PATH_RE+='|/mnt/c/Users/'            # culture-allow: absolute path: this literal is a forbidden WSL user path
PATH_RE+='|/home/'                   # culture-allow: absolute path: this literal is a forbidden Linux home path
PATH_RE+='|[A-Za-z]:/Users/'         # culture-allow: absolute path: this regex detects C:/Users style paths

# Forbidden process patterns. The literal is here so the gate can reject it.
PROCESS_RE='pkill -f|nohup|setsid' # culture-allow: process pattern: this regex names the forbidden patterns so the gate can detect them

PATH_WAIVER='culture-allow: absolute path'

# Documented exceptions: files that QUOTE real command output or describe the
# build lane itself, where an absolute path is evidence, not a configuration.
#   AGENTS.md                              - names the WSL repo path (the lane).
#   .reports/goal-build-fnscope.md         - verbatim orchestrator ledger.
#   .reports/ui-browser.md                 - verbatim browser audit.
#   docs/ethics/lessons.md                 - verbatim incident quotes.
#   docs/workflows/cross-environment-runner.md - verbatim cross-OS error text.
PATH_EXCEPTIONS=(
  'AGENTS.md'
  '.reports/goal-build-fnscope.md'
  '.reports/ui-browser.md'
  'docs/ethics/lessons.md'
  'docs/workflows/cross-environment-runner.md'
)

# The single CRLF detector shared by the gate and its self-test. -U stops grep
# stripping CR before the match; iteration 17's green came from $"\r" instead.
detect_cr_files() {
  grep -rlU $'\r' -- "$@" 2>/dev/null || true
}

# All regular files under the repo, excluding dependency and build trees.
list_repo_files() {
  find "$REPO_ROOT" \
    -path "$REPO_ROOT/.git" -prune -o \
    -name node_modules -prune -o \
    -name target -prune -o \
    -type f -print0
}

# Tracked files plus staged/untracked-but-not-ignored source, text extensions only.
TEXT_FILES=()
while IFS= read -r -d '' f; do
  case "$f" in
    *.rs|*.md|*.sh|*.toml|*.mjs|*.json|*.tsx|*.ts) TEXT_FILES+=("${f#./}") ;;
  esac
done < <(
  { git ls-files -z; git ls-files --others --exclude-standard -z; } \
    | grep -zE '\.(rs|md|sh|toml|mjs|json|tsx|ts)$' \
    | grep -zvE '(^|/)(node_modules|target|\.git)/' \
    || true
)

check_crlf() {
  local -a hits=()
  if (( ${#TEXT_FILES[@]} > 0 )); then
    mapfile -t hits < <(detect_cr_files "${TEXT_FILES[@]}")
  fi
  if (( ${#hits[@]} > 0 )); then
    record_fail "CRLF: CR bytes in ${hits[*]}"
  else
    record_pass "CRLF: no CR bytes in ${#TEXT_FILES[@]} text files"
  fi
}

check_absolute_paths() {
  local -a offenders=()
  local f exception skip bad_lines
  for f in "${TEXT_FILES[@]}"; do
    skip=0
    for exception in "${PATH_EXCEPTIONS[@]}"; do
      if [[ "$f" == "$exception" ]]; then skip=1; break; fi
    done
    (( skip )) && continue
    bad_lines="$(grep -nE "$PATH_RE" "$f" 2>/dev/null | grep -vF "$PATH_WAIVER" || true)"
    if [[ -n "$bad_lines" ]]; then offenders+=("$f"); fi
  done
  if (( ${#offenders[@]} > 0 )); then
    record_fail "ABSOLUTE_PATH: hardcoded path in ${offenders[*]}"
  else
    record_pass "ABSOLUTE_PATH: no unwaived drive/user/home paths"
  fi
}

# A process-pattern waiver counts only with a written explanation after the marker.
waived_process_line() {
  local line="$1" tail
  [[ "$line" == *'culture-allow: process pattern'* ]] || return 1
  tail="${line#*'culture-allow: process pattern'}"
  [[ "$tail" =~ [:-][[:space:]]*[^[:space:]] ]]
}

check_process_patterns() {
  local -a offenders=()
  local f line bad=0
  while IFS= read -r -d '' f; do
    case "$f" in *.sh) ;; *) continue ;; esac
    bad=0
    while IFS= read -r line; do
      if ! waived_process_line "$line"; then bad=1; break; fi
    done < <(grep -nE "$PROCESS_RE" "$f" 2>/dev/null || true)
    (( bad )) && offenders+=("${f#"$REPO_ROOT"/}")
  done < <(list_repo_files)
  if (( ${#offenders[@]} > 0 )); then
    record_fail "PROCESS_PATTERN: unwaived forbidden process pattern in ${offenders[*]}"
  else
    record_pass "PROCESS_PATTERN: no unwaived forbidden process pattern in scripts"
  fi
}

check_drift() {
  local drift='docs/drift.md'
  if [[ ! -f "$drift" ]]; then
    record_fail "DRIFT: $drift is missing"
    return
  fi

  declare -A ROW_STATUS=()
  local -a bad_status=()
  local id status
  while IFS=$'\t' read -r id status; do
    ROW_STATUS["$id"]="$status"
    case "$status" in
      Open|Awaiting\ decision|Resolved) ;;
      *) bad_status+=("$id=$status") ;;
    esac
  done < <(awk -F'|' '
    /^[[:space:]]*D-[0-9]+[[:space:]]*\|/ {
      id = $1; gsub(/^[[:space:]]+|[[:space:]]+$/, "", id);
      st = $(NF-1); gsub(/^[[:space:]]+|[[:space:]]+$/, "", st);
      print id "\t" st;
    }' "$drift")

  local -a bad_notes=() missing_rows=() resolved_rows=()
  for id in "${!ROW_STATUS[@]}"; do
    [[ "${ROW_STATUS[$id]}" == Resolved ]] && resolved_rows+=("$id")
  done
  while IFS= read -r id; do
    [[ -z "$id" ]] && continue
    if [[ -z "${ROW_STATUS[$id]+present}" ]]; then
      missing_rows+=("$id")
    elif [[ "${ROW_STATUS[$id]}" != Resolved ]]; then
      bad_notes+=("$id note says Resolved but row says ${ROW_STATUS[$id]}")
    fi
  done < <(grep -oE 'D-[0-9]+[[:space:]]*->[[:space:]]*Resolved' "$drift" | grep -oE 'D-[0-9]+' || true)

  if (( ${#bad_status[@]} > 0 )); then
    record_fail "DRIFT_STATUS: invalid status cell ${bad_status[*]}"
  else
    record_pass "DRIFT_STATUS: all ${#ROW_STATUS[@]} rows have a valid status cell"
  fi
  if (( ${#bad_notes[@]} > 0 )); then
    record_fail "DRIFT_NOTE: ${bad_notes[*]}"
  else
    record_pass "DRIFT_NOTE: every '-> Resolved' note matches a Resolved row"
  fi
  # Warnings, not failures: a note may quote another file or a superseded
  # revision, and a Resolved row may carry its proof in a 'Verified by:' note
  # rather than the literal '-> Resolved' form. A human decides those cases.
  if (( ${#missing_rows[@]} > 0 )); then
    record_warn "DRIFT_NOTE: '-> Resolved' for ${missing_rows[*]} with no row in this file (quoted or superseded?)"
  fi
  if (( ${#resolved_rows[@]} > 0 )); then
    local id2 seen=0
    for id2 in "${resolved_rows[@]}"; do
      grep -qE "$id2[[:space:]]*->[[:space:]]*Resolved" "$drift" && seen=1
      (( seen )) || record_warn "DRIFT_ROW: $id2 is Resolved with no '-> Resolved' note (check for 'Verified by:')"
      seen=0
    done
  fi
}

check_doc_headings() {
  local -a offenders=()
  local f
  while IFS= read -r -d '' f; do
    if [[ ! -s "$f" ]]; then
      offenders+=("${f#"$REPO_ROOT"/} (empty)")
    elif ! grep -q '^# ' "$f"; then
      offenders+=("${f#"$REPO_ROOT"/} (no top-level heading)")
    fi
  done < <(find "$REPO_ROOT/docs" -type f \
    \( -path '*/ethics/*.md' -o -path '*/workflows/*.md' -o -path '*/concepts/*.md' \) -print0)
  if (( ${#offenders[@]} > 0 )); then
    record_fail "DOC_HEADING: ${offenders[*]}"
  else
    record_pass "DOC_HEADING: every ethics/workflows/concepts doc is non-empty with a '# ' heading"
  fi
}

check_contract_symbols() {
  local -a symbols=(FnGraph DiscoveredFn BodyAnalysis ScanOptions InvokeResponse) missing=()
  local s
  for s in "${symbols[@]}"; do
    if ! grep -rlF "$s" "$REPO_ROOT/crates" --include='*.rs' >/dev/null 2>&1; then
      missing+=("$s")
    fi
  done
  if (( ${#missing[@]} > 0 )); then
    record_fail "CONTRACT_SYMBOL: missing from crate sources: ${missing[*]}"
  else
    record_pass "CONTRACT_SYMBOL: all ${#symbols[@]} key symbols present in crate sources"
  fi
}

check_scratch_files() {
  local -a offenders=()
  local f rel
  while IFS= read -r -d '' f; do
    rel="${f#"$REPO_ROOT"/}"
    case "$rel" in
      *fnscope-node-*|*fnscope-invoke-*|*.log) offenders+=("$rel") ;;
    esac
  done < <(list_repo_files)
  if (( ${#offenders[@]} > 0 )); then
    record_fail "SCRATCH: untracked scratch/log files: ${offenders[*]}"
  else
    record_pass "SCRATCH: no fnscope-node-*, fnscope-invoke-* or *.log files in repo"
  fi
}

# Prove the byte-level detector against a known positive and negative before
# anyone trusts CULTURE_OK. Uses the same detect_cr_files as the gate above.
self_test() {
  local crlf_sample lf_sample crlf_count lf_count
  SELFTEST_TMPDIR="$(mktemp -d)"
  trap 'if [[ -n "$SELFTEST_TMPDIR" ]]; then rm -rf "$SELFTEST_TMPDIR"; fi' EXIT
  crlf_sample="$SELFTEST_TMPDIR/crlf-sample.txt"
  lf_sample="$SELFTEST_TMPDIR/lf-sample.txt"

  printf 'alpha\r\nbeta\r\n' > "$crlf_sample"
  printf 'alpha\nbeta\n' > "$lf_sample"

  crlf_count="$(tr -cd '\r' < "$crlf_sample" | wc -c)"
  lf_count="$(tr -cd '\r' < "$lf_sample" | wc -c)"
  if (( crlf_count == 0 || lf_count != 0 )); then
    printf 'CULTURE_SELFTEST_FAIL: samples wrong (CRLF CR=%s, LF CR=%s)\n' \
      "$crlf_count" "$lf_count" >&2
    exit 1
  fi

  if [[ -z "$(detect_cr_files "$crlf_sample")" ]]; then
    printf 'CULTURE_SELFTEST_FAIL: known CRLF sample was not detected\n' >&2
    exit 1
  fi
  if [[ -n "$(detect_cr_files "$lf_sample")" ]]; then
    printf 'CULTURE_SELFTEST_FAIL: known LF sample was reported as CRLF\n' >&2
    exit 1
  fi
  # Clean up before the marker so no post-marker error can follow a green.
  rm -rf "$SELFTEST_TMPDIR"
  SELFTEST_TMPDIR=''
  trap - EXIT
  printf 'CULTURE_SELFTEST_OK\n'
}

if [[ "${1:-}" == '--self-test' ]]; then
  self_test
  exit 0
fi
if (( $# > 0 )); then
  printf 'Usage: %s [--self-test]\n' "$0" >&2
  exit 2
fi

check_crlf
check_absolute_paths
check_process_patterns
check_drift
check_doc_headings
check_contract_symbols
check_scratch_files

if (( ${#FAILURES[@]} > 0 )); then
  reason="$(printf '%s; ' "${FAILURES[@]}")"
  printf 'CULTURE_FAIL: %s\n' "${reason%; }"
  exit 1
fi
printf 'CULTURE_OK\n'
