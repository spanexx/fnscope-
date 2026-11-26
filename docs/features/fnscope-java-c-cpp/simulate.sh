#!/usr/bin/env bash
# POST-IMPL / CANONICAL (reconstructed 2026-11-26)
#
# Canonical source-level simulation for the fnscope-java-c-cpp pack. It was
# written after J30-J34 shipped in commit 5408cad and is reconstructed from
# the shipped adapters, contract docs and fixture READMEs. It is not a
# backdated artifact and it does not replace the real gate in ledger
# iterations 33-35 (`.reports/goal-build-fnscope.md:317-338`).
#
# Scenarios (see PRD-TRD-fnscope-java-c-cpp.md "Simulation contract"):
#   S0 adapter identity: extensions + registration + default globs
#   S1 scan three fixtures: each source and documented callable is present
#   S2 call edges: each documented edge has a real call site in its fixture
#   S3 no dangling: every edge endpoint is a documented callable
#   S4 one PASS/FAIL line per assertion and a final verdict marker
#
# This is pure shell: no cargo, no server, no Java/C/C++ toolchain. It reads
# the fixtures as text and asserts the documented fact set. A green here is a
# source-level simulation result, NOT a compile, test or runtime verdict.
# Usage: bash docs/features/fnscope-java-c-cpp/simulate.sh
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

PARSER="$REPO_ROOT/crates/fnscope-parser/src"
CORE="$REPO_ROOT/crates/fnscope-core/src"
JAVA_MOD="$PARSER/java/mod.rs"
C_MOD="$PARSER/c_lang/mod.rs"
CPP_MOD="$PARSER/cpp/mod.rs"
LIB_RS="$PARSER/lib.rs"
ADAPTER_RS="$PARSER/adapter.rs"
SCAN_RS="$CORE/scan.rs"
WORKSPACE_MANIFEST="$REPO_ROOT/Cargo.toml"

JAVA_DIR="$REPO_ROOT/tests/fixtures/java_app"
C_DIR="$REPO_ROOT/tests/fixtures/c_app"
CPP_DIR="$REPO_ROOT/tests/fixtures/cpp_app"

RESULT=0

pass() { printf '[PASS] %s\n' "$1"; }
fail() {
  printf '[FAIL] %s : %s\n' "$1" "$2"
  RESULT=1
}

assert_fixed() {
  local label="$1" file="$2" needle="$3"
  if [[ ! -f "$file" ]]; then
    fail "$label" "missing file: $file"
  elif grep -Fq -- "$needle" "$file"; then
    pass "$label"
  else
    fail "$label" "expected text not found: $needle ($file)"
  fi
}

assert_absent() {
  local label="$1" file="$2" needle="$3"
  if [[ ! -f "$file" ]]; then
    fail "$label" "missing file: $file"
  elif grep -Fq -- "$needle" "$file"; then
    fail "$label" "unexpected text found: $needle ($file)"
  else
    pass "$label"
  fi
}

known_contains() {
  local needle="$1"
  shift
  local item
  for item in "$@"; do
    if [[ "$item" == "$needle" ]]; then
      return 0
    fi
  done
  return 1
}

# Documented fact sets from the three fixture READMEs.
JAVA_SHORT=("normalize" "analyze" "readConfig" "auth" "run" "main" "FunctionError")
JAVA_QUALIFIED=(
  "Main.normalize" "Main.analyze" "Main.readConfig" "Main.auth"
  "Main.run" "Main.main" "FunctionError.FunctionError"
)
C_SHORT=("add" "normalize" "report" "main")
C_QUALIFIED=("add" "normalize" "report" "main")
CPP_SHORT=("add" "normalize" "Calculator" "scale" "guard" "report")
CPP_QUALIFIED=(
  "add" "normalize" "Calculator.Calculator" "Calculator.scale" "guard" "report"
)

# Each edge: caller|callee|call-site text in the fixture source|source file.
JAVA_EDGES=(
  "Main.run|Main.analyze|analyze(\"hello\")|$JAVA_DIR/Main.java"
  "Main.main|Main.run|run(args)|$JAVA_DIR/Main.java"
)
C_EDGES=(
  "main|report|report(value)|$C_DIR/main.c"
)
CPP_EDGES=(
  "normalize|add|add(n, 1)|$CPP_DIR/main.cpp"
)

echo "[canonical-sim] repo root: $REPO_ROOT"
echo "[canonical-sim] this is a source-level simulation, not a cargo gate"

# ------------------------------------------------- S0: identity + registration
echo "[canonical-sim] S0 adapter extensions and registration"
assert_fixed "S0 Java claims suffix java" "$JAVA_MOD" '&["java"]'
assert_fixed "S0 C claims suffix c" "$C_MOD" '&["c"]'
assert_fixed "S0 C++ claims suffixes cpp and cc" "$CPP_MOD" '&["cpp", "cc"]'
assert_absent "S0 C does not claim a quoted h suffix" "$C_MOD" '"h"'
assert_absent "S0 C++ does not claim cxx" "$CPP_MOD" '"cxx"'

assert_fixed "S0 java module declared" "$LIB_RS" 'pub mod java;'
assert_fixed "S0 c_lang module declared" "$LIB_RS" 'pub mod c_lang;'
assert_fixed "S0 cpp module declared" "$LIB_RS" 'pub mod cpp;'
assert_fixed "S0 Java adapter registered" "$ADAPTER_RS" 'JavaAdapter::new()'
assert_fixed "S0 C adapter registered" "$ADAPTER_RS" 'CAdapter::new()'
assert_fixed "S0 C++ adapter registered" "$ADAPTER_RS" 'CppAdapter::new()'
assert_fixed "S0 Java id row" "$ADAPTER_RS" '("java", "java")'
assert_fixed "S0 C id row" "$ADAPTER_RS" '("c", "c")'
assert_fixed "S0 C++ id row cpp" "$ADAPTER_RS" '("cpp", "cpp")'
assert_fixed "S0 C++ id row cc" "$ADAPTER_RS" '("cc", "cpp")'

assert_fixed "S0 default include java" "$SCAN_RS" '"**/*.java"'
assert_fixed "S0 default include c" "$SCAN_RS" '"**/*.c"'
assert_fixed "S0 default include cpp" "$SCAN_RS" '"**/*.cpp"'
assert_fixed "S0 default include cc" "$SCAN_RS" '"**/*.cc"'
assert_fixed "S0 Java grammar dependency" "$WORKSPACE_MANIFEST" 'tree-sitter-java'
assert_fixed "S0 C grammar dependency" "$WORKSPACE_MANIFEST" 'tree-sitter-c'
assert_fixed "S0 C++ grammar dependency" "$WORKSPACE_MANIFEST" 'tree-sitter-cpp'

# --------------------------------------------- S1: scan the three fixtures
echo "[canonical-sim] S1 scan three fixtures"
for spec in \
  "java|$JAVA_DIR/Main.java|$JAVA_DIR/README.md" \
  "c|$C_DIR/main.c|$C_DIR/README.md" \
  "cpp|$CPP_DIR/main.cpp|$CPP_DIR/README.md"; do
  lang="${spec%%|*}"
  rest="${spec#*|}"
  src="${rest%%|*}"
  readme="${rest#*|}"
  if [[ -f "$src" ]]; then
    pass "S1 $lang fixture source present"
  else
    fail "S1 $lang fixture source present" "missing: $src"
    continue
  fi
  if [[ -f "$readme" ]]; then
    pass "S1 $lang fixture README present"
  else
    fail "S1 $lang fixture README present" "missing: $readme"
  fi

  case "$lang" in
    java)
      short_set=("${JAVA_SHORT[@]}")
      qualified_set=("${JAVA_QUALIFIED[@]}")
      ;;
    c)
      short_set=("${C_SHORT[@]}")
      qualified_set=("${C_QUALIFIED[@]}")
      ;;
    cpp)
      short_set=("${CPP_SHORT[@]}")
      qualified_set=("${CPP_QUALIFIED[@]}")
      ;;
  esac

  for name in "${short_set[@]}"; do
    assert_fixed "S1 $lang callable $name in source" "$src" "$name("
  done
  for qualified in "${qualified_set[@]}"; do
    assert_fixed "S1 $lang qualified name $qualified in README" "$readme" "$qualified"
  done
done

# --------------------------- S2: documented call edges have real call sites
echo "[canonical-sim] S2 documented call edges"
for group in java c cpp; do
  case "$group" in
    java) edges=("${JAVA_EDGES[@]}") ;;
    c) edges=("${C_EDGES[@]}") ;;
    cpp) edges=("${CPP_EDGES[@]}") ;;
  esac
  for edge in "${edges[@]}"; do
    caller="${edge%%|*}"
    rest="${edge#*|}"
    callee="${rest%%|*}"
    rest="${rest#*|}"
    site="${rest%%|*}"
    src="${rest#*|}"
    assert_fixed "S2 $group edge $caller -> $callee call site" "$src" "$site"
  done
done

# ---------------------------------- S3: no dangling edge endpoint
echo "[canonical-sim] S3 no dangling edge endpoint"
for group in java c cpp; do
  case "$group" in
    java)
      edges=("${JAVA_EDGES[@]}")
      names=("${JAVA_QUALIFIED[@]}")
      ;;
    c)
      edges=("${C_EDGES[@]}")
      names=("${C_QUALIFIED[@]}")
      ;;
    cpp)
      edges=("${CPP_EDGES[@]}")
      names=("${CPP_QUALIFIED[@]}")
      ;;
  esac
  for edge in "${edges[@]}"; do
    caller="${edge%%|*}"
    rest="${edge#*|}"
    callee="${rest%%|*}"
    if known_contains "$caller" "${names[@]}"; then
      pass "S3 $group edge caller $caller is known"
    else
      fail "S3 $group edge caller $caller is known" "dangling caller"
    fi
    if known_contains "$callee" "${names[@]}"; then
      pass "S3 $group edge callee $callee is known"
    else
      fail "S3 $group edge callee $callee is known" "dangling callee"
    fi
  done
done

# ------------------------------------------------------------- S4: verdict
if [[ "$RESULT" -eq 0 ]]; then
  echo "PASS"
else
  echo "FAIL"
  exit 1
fi
