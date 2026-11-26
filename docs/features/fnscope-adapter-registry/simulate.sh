#!/usr/bin/env bash
# POST-IMPL / CANONICAL (reconstructed 2026-11-26)
#
# Canonical source-level simulation for the fnscope-adapter-registry pack.
# Reconstructed from the shipped seam (J21/J22) on 2026-11-26; it is not a
# backdated artifact and it does not replace the recorded gate in the ledger.
#
# Scenarios (see PRD-TRD-fnscope-adapter-registry.md "Simulation contract"):
#   S0 preflight: the two Rust source files are present
#   S1 registry resolves every documented built-in suffix to its adapter id
#   S2 an unregistered suffix resolves to None
#   S3 a fake adapter is injected into core through scan_with_registry
#   S4 core has no language branch and exactly one lookup per file
#   S5 all five deliverables are LF-only and the pre-sim is labelled
#
# It reads source only: no cargo, no server, no file writes, no hardcoded drive
# or user directory. Usage: bash docs/features/fnscope-adapter-registry/simulate.sh
set -uo pipefail
export LC_ALL=C

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
PARSER_DIR="$REPO_ROOT/crates/fnscope-parser/src"
ADAPTER_RS="$PARSER_DIR/adapter.rs"
SCAN_RS="$REPO_ROOT/crates/fnscope-core/src/scan.rs"

if [[ ! -f "$ADAPTER_RS" || ! -f "$SCAN_RS" ]]; then
  printf '[FAIL] S0 preflight : missing %s or %s\n' "$ADAPTER_RS" "$SCAN_RS"
  printf 'ADAPTER_SIM_FAIL\n'
  exit 1
fi
printf '[PASS] S0 preflight : parser adapter.rs and core scan.rs are present\n'

python3 - "$PARSER_DIR" "$ADAPTER_RS" "$SCAN_RS" "$SCRIPT_DIR" <<'PY'
import os
import re
import sys

parser_dir, adapter_path, scan_path, pack_dir = sys.argv[1:5]

failures = []


def check(name, ok, detail=""):
    if ok:
        print("[PASS] " + name)
    else:
        print("[FAIL] " + name + " : " + detail)
        failures.append(name)


def read_text(path):
    with open(path, "rb") as fh:
        return fh.read().decode("utf-8")


def fn_body(text, signature):
    """Return the brace-balanced body of the first fn with `signature`."""
    pos = text.find(signature)
    if pos < 0:
        return ""
    brace = text.find("{", pos)
    if brace < 0:
        return ""
    depth = 0
    for i in range(brace, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return text[brace + 1:i]
    return ""


def quoted(text):
    return re.findall(r'"([^"]+)"', text)


adapter = read_text(adapter_path)
scan = read_text(scan_path)
parts = scan.split("#[cfg(test)]", 1)
prod_scan = parts[0]
test_scan = parts[1] if len(parts) > 1 else ""

# ---------------------------------------------------------------- S1: builtins
# (adapter type, module file, claimed suffix, expected id)
expected = [
    ("TypeScriptAdapter", "typescript/mod.rs", "ts", "typescript"),
    ("TypeScriptAdapter", "typescript/mod.rs", "tsx", "tsx"),
    ("TypeScriptAdapter", "typescript/mod.rs", "js", "javascript"),
    ("TypeScriptAdapter", "typescript/mod.rs", "jsx", "jsx"),
    ("RustAdapter", "rust/mod.rs", "rs", "rust"),
    ("PythonAdapter", "python/mod.rs", "py", "python"),
    ("GoAdapter", "golang/mod.rs", "go", "go"),
    ("JavaAdapter", "java/mod.rs", "java", "java"),
    ("CAdapter", "c_lang/mod.rs", "c", "c"),
    ("CppAdapter", "cpp/mod.rs", "cpp", "cpp"),
    ("CppAdapter", "cpp/mod.rs", "cc", "cpp"),
    ("CSharpAdapter", "csharp/mod.rs", "cs", "csharp"),
    ("KotlinAdapter", "kotlin/mod.rs", "kt", "kotlin"),
    ("PhpAdapter", "php/mod.rs", "php", "php"),
    ("RubyAdapter", "ruby/mod.rs", "rb", "ruby"),
    ("SwiftAdapter", "swift/mod.rs", "swift", "swift"),
]

registry_body = fn_body(adapter, "pub fn with_builtin_adapters()")
registered = re.findall(r"Box::new\((\w+)", registry_body)
module_cache = {}
missing = []
for adapter_type, module_file, suffix, adapter_id in expected:
    path = os.path.join(parser_dir, *module_file.split("/"))
    if module_file not in module_cache:
        module_cache[module_file] = read_text(path) if os.path.isfile(path) else ""
    module = module_cache[module_file]
    ext_claims = quoted(fn_body(module, "fn extensions"))
    ids = quoted(fn_body(module, "fn id"))
    if adapter_type not in registered:
        missing.append("%s not registered" % adapter_type)
    elif not module:
        missing.append("missing module %s" % module_file)
    elif suffix not in ext_claims:
        missing.append("%s does not claim %s" % (module_file, suffix))
    elif adapter_id not in ids:
        missing.append("%s has no id %s" % (module_file, adapter_id))

distinct_types = set(registered)
wanted_types = {row[0] for row in expected}
if not distinct_types.issuperset(wanted_types):
    missing.append("registered types %s do not cover %s" % (sorted(distinct_types), sorted(wanted_types)))
if registered.count("TypeScriptAdapter") < 4:
    missing.append("fewer than four TypeScriptAdapter registrations")

check(
    "S1 registry resolves built-in suffixes",
    not missing,
    "; ".join(missing[:6]),
)

# --------------------------------------------------------- S2: unclaimed suffix
claimed = set()
for module in module_cache.values():
    claimed.update(quoted(fn_body(module, "fn extensions")))
s2_detail = []
if "zig" in claimed:
    s2_detail.append("a production adapter claims zig")
if "pub fn for_extension(&self, ext: &str) -> Option<&dyn LanguageAdapter>" not in adapter:
    s2_detail.append("for_extension does not return Option<&dyn LanguageAdapter>")
if "find_map" not in fn_body(adapter, "pub fn for_extension"):
    s2_detail.append("for_extension does not use find_map (first claim wins)")
if 'for_extension("zig").is_none()' not in adapter or "unregistered_extension_returns_none" not in adapter:
    s2_detail.append("no unit test asserting an unknown suffix returns None")
check("S2 unregistered suffix resolves to None", not s2_detail, "; ".join(s2_detail))

# ------------------------------------------------------- S3: injected adapter
s3_tokens = [
    "struct FakeAdapter",
    "impl LanguageAdapter for FakeAdapter",
    '"zig"',
    "Registry::new()",
    "register(Box::new(FakeAdapter))",
    "scan_with_registry(",
    "builtin.functions.is_empty()",
    "report.function_count, 1",
    'graph.functions[0].name, "zeta"',
    "main.zig",
]
s3_missing = [token for token in s3_tokens if token not in test_scan]
if "FakeAdapter" in prod_scan:
    s3_missing.append("FakeAdapter appears in production code, not only the test")
check("S3 fake adapter injected into core", not s3_missing, "; ".join(s3_missing))

# --------------------------------------------------------- S4: core is neutral
s4_detail = []
if re.search(r"\bLang\b", prod_scan):
    s4_detail.append("standalone Lang token in production scan.rs")
if "ends_with(" in prod_scan or "match lang" in prod_scan:
    s4_detail.append("per-language branch in production scan.rs")
if prod_scan.count("for_extension(") != 1:
    s4_detail.append("expected one production for_extension call, found %d" % prod_scan.count("for_extension("))
if "pub fn scan_with_registry(" not in prod_scan:
    s4_detail.append("no scan_with_registry injection seam")
if "scan_with_registry(options, out_dir, generated_at, &registry)" not in prod_scan:
    s4_detail.append("scan does not delegate to scan_with_registry")
check("S4 core has no language branch", not s4_detail, "; ".join(s4_detail))

# ---------------------------------------------------------- S5: LF and labels
pack_files = [
    "GRILL-fnscope-adapter-registry.txt",
    "PRD-TRD-fnscope-adapter-registry.md",
    "IMPL-fnscope-adapter-registry.md",
    "simulate.sh",
    os.path.join("archive", "simulate-pre.reconstructed.sh"),
]
cr_files = []
pre_bytes = b""
for name in pack_files:
    path = os.path.join(pack_dir, *name.split("/"))
    if not os.path.isfile(path):
        cr_files.append(name + " missing")
        continue
    with open(path, "rb") as fh:
        data = fh.read()
    # Equivalent to `tr -cd '\r' < file | wc -c` == 0 (L10/L11).
    if b"\r" in data:
        cr_files.append(name + " has CR bytes")
    if name.endswith("simulate-pre.reconstructed.sh"):
        pre_bytes = data
if b"PRE_SIM_PLACEHOLDER" not in pre_bytes:
    cr_files.append("pre-sim is not labelled PRE_SIM_PLACEHOLDER")
check("S5 LF-only deliverables and labelled pre-sim", not cr_files, "; ".join(cr_files))

if failures:
    print("ADAPTER_SIM_FAIL")
    sys.exit(1)
print("ADAPTER_SIM_OK")
PY
rc=$?
exit "$rc"
