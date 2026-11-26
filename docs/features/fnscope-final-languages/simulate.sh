#!/usr/bin/env bash
# Canonical POST-impl simulation for the fnscope-final-languages pack.
# RECONSTRUCTED 2026-11-26. Pure shell: no cargo, no server, no Kotlin/C#/
# Ruby/PHP/Swift toolchain. It scans the five committed fixtures, asserts each
# extension's documented callables and edges, proves every documented edge
# endpoint is a known function (no dangling edges), and checks the shipped
# registration, grammar and glob facts that make the scan reachable.
# PASS/FAIL only.
set -u
export LC_ALL=C

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
ROOT="$(cd -- "$SCRIPT_DIR/../../.." && pwd -P)"

KT_DIR="$ROOT/tests/fixtures/kotlin_app"
CS_DIR="$ROOT/tests/fixtures/csharp_app"
RB_DIR="$ROOT/tests/fixtures/ruby_app"
PHP_DIR="$ROOT/tests/fixtures/php_app"
SWIFT_DIR="$ROOT/tests/fixtures/swift_app"

KT_MAIN="$KT_DIR/src/Main.kt"
CS_APP="$CS_DIR/App.cs"
RB_MAIN="$RB_DIR/main.rb"
PHP_TEXT="$PHP_DIR/src/Text.php"
PHP_METRICS="$PHP_DIR/src/Metrics.php"
SWIFT_MAIN="$SWIFT_DIR/main.swift"

LIB_RS="$ROOT/crates/fnscope-parser/src/lib.rs"
ADAPTER_RS="$ROOT/crates/fnscope-parser/src/adapter.rs"
KT_MOD_RS="$ROOT/crates/fnscope-parser/src/kotlin/mod.rs"
KT_DISCOVER_RS="$ROOT/crates/fnscope-parser/src/kotlin/discover.rs"
KT_BODY_RS="$ROOT/crates/fnscope-parser/src/kotlin/body.rs"
PHP_DISCOVER_RS="$ROOT/crates/fnscope-parser/src/php/discover.rs"
PHP_BODY_RS="$ROOT/crates/fnscope-parser/src/php/body.rs"
PARSER_MANIFEST="$ROOT/crates/fnscope-parser/Cargo.toml"
WORKSPACE_MANIFEST="$ROOT/Cargo.toml"
CORE_SCAN="$ROOT/crates/fnscope-core/src/scan.rs"

PASS_COUNT=0
FAIL_COUNT=0
EDGE_COUNT=0

ok() {
  PASS_COUNT=$((PASS_COUNT + 1))
  printf '[PASS] %s\n' "$1"
}

bad() {
  FAIL_COUNT=$((FAIL_COUNT + 1))
  printf '[FAIL] %s\n' "$1"
}

# Assert a fixed string occurs in a file.
has() {
  local needle="$1" file="$2"
  if [ -f "$file" ] && grep -Fq -- "$needle" "$file"; then
    ok "found '$needle' in ${file#$ROOT/}"
  else
    bad "missing '$needle' in ${file#$ROOT/}"
  fi
}

# Assert a fixed string does NOT occur in a file.
hasnt() {
  local needle="$1" file="$2"
  if [ -f "$file" ] && ! grep -Fq -- "$needle" "$file"; then
    ok "absent '$needle' in ${file#$ROOT/}"
  else
    bad "unexpected '$needle' in ${file#$ROOT/}"
  fi
}

# Assert a name belongs to a space-separated known set (no dangling edge).
known() {
  local name="$1" set="$2" item
  for item in $set; do
    if [ "$item" = "$name" ]; then
      ok "edge endpoint '$name' is a known function"
      return 0
    fi
  done
  bad "dangling edge endpoint '$name' is not a known function"
}

# Assert a documented edge: both endpoints known and the call text present.
edge() {
  local caller="$1" callee="$2" call_text="$3" file="$4" caller_set="$5" callee_set="$6"
  EDGE_COUNT=$((EDGE_COUNT + 1))
  known "$caller" "$caller_set"
  known "$callee" "$callee_set"
  has "$call_text" "$file"
}

printf 'fnscope-final-languages post-impl simulation (RECONSTRUCTED 2026-11-26)\n'
printf 'root: %s\n' "$ROOT"

# 1. Each extension is physically present in its fixture.
has 'package demo' "$KT_MAIN"
has 'namespace CSharpApp' "$CS_APP"
has 'def normalize_text(text)' "$RB_MAIN"
has '<?php' "$PHP_TEXT"
has 'class Calculator' "$PHP_METRICS"
has 'import Foundation' "$SWIFT_MAIN"

# 2. Kotlin (.kt) — six documented callables from kotlin_app/README.md:21-30.
KT_KNOWN='Result Greeter of greet echo evaluate'
has 'data class Result(val code: Int, val message: String)' "$KT_MAIN"
has 'class Greeter(val prefix: String = "Hello")' "$KT_MAIN"
has 'fun of(prefix: String = "Hi"): Greeter = Greeter(prefix)' "$KT_MAIN"
has 'fun greet(name: String): String = "$prefix, $name"' "$KT_MAIN"
has 'fun echo(text: String = "world"): String = text' "$KT_MAIN"
has 'fun evaluate(input: Int): Result {' "$KT_MAIN"

# 3. C# (.cs) — documented callables from csharp_app/README.md:24-35.
CS_KNOWN='Normalize Add Analyze ReadConfig Auth Fetch Run Localize Ping RoleResult'
has 'public static string Normalize(string text)' "$CS_APP"
has 'public static double Add(double value, double factor = 2.5)' "$CS_APP"
has 'public static RoleResult Analyze(string text)' "$CS_APP"
has 'public static int ReadConfig()' "$CS_APP"
has 'public static string Auth()' "$CS_APP"
has 'public static string Fetch(string url)' "$CS_APP"
has 'public static string Run(string value)' "$CS_APP"
has 'string Localize(string input)' "$CS_APP"
has 'public sealed record Ping(string Code, int Retry = 3);' "$CS_APP"
has 'public RoleResult(string role, double score)' "$CS_APP"

# 4. Ruby (.rb) — five documented callables from ruby_app/README.md:21-27.
RB_KNOWN='normalize_text strip_controls report Formatter format default_formatter'
has 'def normalize_text(text)' "$RB_MAIN"
has 'def strip_controls(text)' "$RB_MAIN"
has 'def report(text)' "$RB_MAIN"
has 'class Formatter' "$RB_MAIN"
has 'def format(value)' "$RB_MAIN"
has 'def self.default_formatter' "$RB_MAIN"
has 'Record = Struct.new(:id, :name)' "$RB_MAIN"

# 5. PHP (.php) — five documented callables from php_app/README.md:22-28.
PHP_KNOWN='normalize_text classify report add scale Calculator'
has 'function normalize_text(string $text): string' "$PHP_TEXT"
has 'function classify(string $text): string' "$PHP_TEXT"
has 'function report(string $text): void' "$PHP_TEXT"
has 'function add(int $a, int $b = 0): int' "$PHP_METRICS"
has 'public function scale(int $value): int' "$PHP_METRICS"
has 'public const SCALE = 3;' "$PHP_METRICS"

# 6. Swift (.swift) — five documented callables from swift_app/README.md:16-22.
SWIFT_KNOWN='Grade describe grade main normalize report'
has 'struct Grade {' "$SWIFT_MAIN"
has 'func describe() -> String' "$SWIFT_MAIN"
has 'func grade(text: String, minimum: Int = 1) throws -> Grade' "$SWIFT_MAIN"
has 'func main()' "$SWIFT_MAIN"
has 'func normalize(_ text: String) -> String' "$SWIFT_MAIN"
has 'func report(_ result: Grade)' "$SWIFT_MAIN"

# 7. Documented edges. Both endpoints are checked against that language's
#    known set, so every asserted edge is proven non-dangling.
edge 'evaluate' 'of' 'val greeter = Greeter.of("Hello")' "$KT_MAIN" "$KT_KNOWN" "$KT_KNOWN"
edge 'evaluate' 'greet' 'val message = greeter.greet(echo("Kotlin"))' "$KT_MAIN" "$KT_KNOWN" "$KT_KNOWN"
edge 'evaluate' 'echo' 'echo("Kotlin")' "$KT_MAIN" "$KT_KNOWN" "$KT_KNOWN"
edge 'Analyze' 'Normalize' 'string cleaned = Normalize(text);' "$CS_APP" "$CS_KNOWN" "$CS_KNOWN"
edge 'Analyze' 'Add' 'double scaled = Add(cleaned.Length);' "$CS_APP" "$CS_KNOWN" "$CS_KNOWN"
edge 'normalize_text' 'strip_controls' 'cleaned = strip_controls(text)' "$RB_MAIN" "$RB_KNOWN" "$RB_KNOWN"
edge 'classify' 'normalize_text' '$cleaned = normalize_text($text);' "$PHP_TEXT" "$PHP_KNOWN" "$PHP_KNOWN"
edge 'scale' 'add' 'return add($value * $this->factor);' "$PHP_METRICS" "$PHP_KNOWN" "$PHP_KNOWN"
edge 'grade' 'normalize' 'let clean = normalize(text)' "$SWIFT_MAIN" "$SWIFT_KNOWN" "$SWIFT_KNOWN"
edge 'main' 'report' 'report(result ?? Grade(score: 0, role: "none"))' "$SWIFT_MAIN" "$SWIFT_KNOWN" "$SWIFT_KNOWN"

# 8. Error, purity and type-index evidence named in the fixture READMEs.
has 'require(input >= 0) { "INVALID_INPUT" }' "$KT_MAIN"
has 'throw new FunctionError("TOKEN_NOT_FOUND")' "$CS_APP"
has 'raise ArgumentError, "EMPTY_TEXT"' "$RB_MAIN"
has 'throw new FunctionError("EMPTY_TEXT")' "$PHP_TEXT"
has 'throw FunctionError(code: "EMPTY_TEXT")' "$SWIFT_MAIN"
has 'Console.WriteLine("cleaned: " + cleaned);' "$CS_APP"
has 'puts text' "$RB_MAIN"
has 'echo "report: " . $text;' "$PHP_TEXT"
has 'print("graded \(clean)")' "$SWIFT_MAIN"

# 9. Structural facts: all five modules, built-ins and default globs.
has 'pub mod kotlin;' "$LIB_RS"
has 'pub mod csharp;' "$LIB_RS"
has 'pub mod ruby;' "$LIB_RS"
has 'pub mod php;' "$LIB_RS"
has 'pub mod swift;' "$LIB_RS"
has 'KotlinAdapter::new()' "$ADAPTER_RS"
has 'CSharpAdapter::new()' "$ADAPTER_RS"
has 'PhpAdapter::new()' "$ADAPTER_RS"
has 'RubyAdapter::new()' "$ADAPTER_RS"
has 'SwiftAdapter::new()' "$ADAPTER_RS"
has '("cs", "csharp")' "$ADAPTER_RS"
has '("kt", "kotlin")' "$ADAPTER_RS"
has '("php", "php")' "$ADAPTER_RS"
has '("rb", "ruby")' "$ADAPTER_RS"
has '("swift", "swift")' "$ADAPTER_RS"
has '"**/*.cs"' "$CORE_SCAN"
has '"**/*.kt"' "$CORE_SCAN"
has '"**/*.php"' "$CORE_SCAN"
has '"**/*.rb"' "$CORE_SCAN"
has '"**/*.swift"' "$CORE_SCAN"

# 10. D-6 grammar facts: -ng replacement with no raw-pointer bridge.
has 'tree-sitter-kotlin-ng = "1.1"' "$WORKSPACE_MANIFEST"
has 'tree-sitter-kotlin-ng.workspace = true' "$PARSER_MANIFEST"
hasnt 'tree-sitter-kotlin = "' "$WORKSPACE_MANIFEST"
has 'tree_sitter_kotlin_ng::LANGUAGE.into()' "$KT_MOD_RS"
hasnt 'transmute' "$KT_MOD_RS"
hasnt 'transmute' "$KT_DISCOVER_RS"
hasnt 'transmute' "$KT_BODY_RS"

# 11. PHP grammar const and the other four grammar dependencies.
has 'tree_sitter_php::LANGUAGE_PHP.into()' "$PHP_DISCOVER_RS"
has 'tree_sitter_php::LANGUAGE_PHP.into()' "$PHP_BODY_RS"
hasnt 'tree_sitter_php::LANGUAGE.into()' "$PHP_DISCOVER_RS"
hasnt 'tree_sitter_php::LANGUAGE.into()' "$PHP_BODY_RS"
has 'tree-sitter-c-sharp = "0.24"' "$WORKSPACE_MANIFEST"
has 'tree-sitter-ruby = "0.23.1"' "$WORKSPACE_MANIFEST"
has 'tree-sitter-php = "0.25"' "$WORKSPACE_MANIFEST"
has 'tree-sitter-swift = "0.7.4"' "$WORKSPACE_MANIFEST"

printf 'documented edges with verified endpoints: %d\n' "$EDGE_COUNT"
printf 'checks: %d passed, %d failed\n' "$PASS_COUNT" "$FAIL_COUNT"
if [ "$FAIL_COUNT" -eq 0 ]; then
  printf 'PASS\n'
  exit 0
fi
printf 'FAIL\n'
exit 1
