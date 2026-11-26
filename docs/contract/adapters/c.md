# C adapter contract (J31)

This document records the mapping implemented by the C adapter in
`crates/fnscope-parser/src/c_lang/`. It adds no field to the locked schema in
`docs/contract/rust-api.md`: every fact is returned as an unchanged
`DiscoveredFn`, `BodyAnalysis`, `Step`, `ErrorSpec` or `ReturnShape`.

The adapter is **not registered globally** by this job. Registration is a
separate, reviewed change; until then a caller must build a `Registry`, call
`register(Box::new(CAdapter::new()))` and pass it to `scan_with_registry`.

## Module name and extensions

The directory is `c_lang/`: `c` alone would be easy to confuse with the crate
name and with the C toolchain. The public type is `CAdapter` and its id is `"c"`.

| Item | Value | Reason |
|---|---|---|
| `CAdapter::id()` | `"c"` | Stable diagnostics name. |
| `CAdapter::extensions()` | `&["c"]` | `LanguageAdapter::extensions` documents suffixes **without** a leading dot; `Registry::for_extension` trims a caller's dot and lowercases before comparing, so `.c`, `c` and `C` all resolve. |
| `CAdapter::new()` / `Default` | stateless value | Registration needs no configuration or global state. |

**Header files are not claimed.** `.h` files repeat declarations that already
have an implementation in a `.c` file; discovering both would add duplicate
functions with different ids and pollute the call graph. Prototypes inside `.c`
files are likewise skipped. Only `function_definition` nodes directly under the
translation unit become callables.

## Grammar source and wrapper nodes

The adapter uses `tree_sitter_c::LANGUAGE`, converted into a
`tree_sitter::Language` exactly as the sibling adapters convert their language
constants. The constant exists at `bindings/rust/lib.rs:35` of the cached
`tree-sitter-c-0.24.2` crate. The dependency is not added by this job (see
Findings). A parse failure yields an empty `Vec`/map or
`BodyAnalysis::default()`; no partial manifest is invented.

Wrapper nodes handled, verified in that crate's `src/node-types.json`:

| Wrapper | Where | Handling |
|---|---|---|
| `field_declaration_list` | `struct_specifier.body` / `union_specifier.body` | The type index unwraps this node before reading `field_declaration` members; skipping it would index zero fields. |
| `pointer_declarator` / `parenthesized_declarator` / `attributed_declarator` / `array_declarator` | `function_definition.declarator`, parameter declarators | Descended to find the inner `function_declarator` and the bare `identifier`; pointer depth is appended as `*` to the declared type. |
| `statement_list` | not present in `tree-sitter-c-0.24.2` | `body.rs` unwraps it defensively if a later grammar inserts it; in 0.24.2 `compound_statement` holds statements directly. |

## Node kinds consumed

| Node kind | Role |
|---|---|
| `function_definition` | Top-level function; direct children of the translation unit only. `static` is a `storage_class_specifier` and is included. |
| `function_declarator`, `parameter_list`, `parameter_declaration` | Function name and ordered parameters; bare `void` and `...` have no name and are skipped. |
| `declaration` | A prototype is not a callable; in a body, a declaration with a call initializer is a `Call` step. |
| `struct_specifier`, `union_specifier`, `type_definition`, `field_declaration_list`, `field_declaration` | Named record fields in the type index. |
| `comment` | Contiguous leading `//` or `/* */` block for `doc_description`, `@param` texts and `@tags`. |
| `call_expression`, `argument_list` | Call steps, `callee_names` and effect markers. |
| `if_statement`, `switch_statement` | Branch steps; `condition` is the guard text. |
| `return_statement` | Return step and guarded `return -1` / `return NULL` detection. |
| `string_literal` | Error code inside `fprintf(stderr, "CODE")` and secret key inside `getenv("KEY")`. |

## Discovery mapping

- **Names.** A definition keeps its bare identifier as both `name` and
  `qualified_name`, with `FnKind::Declaration`, `class_name = None` and
  `is_async = false`. C has no classes or async keyword.
- **Lines and source.** `line`/`end_line` are 1-based from the node positions;
  `source_code` is the exact node byte slice.
- **Parameters.** Every named `parameter_declaration` becomes a required
  `Parameter`. The declared type keeps qualifiers (`const char *`) and pointer
  depth; unnamed parameters and `void` lists are skipped, so `default_value` is
  always `None`.
- **Return type.** The text before the declarator (minus storage-class words)
  plus any pointer depth is reported, for example `int`, `char *` or
  `struct Point`. Fields resolve through the shared resolver after stripping
  `struct`/`union`/`enum` and trailing `*`. An absent type is `void`.
- **Docs.** Contiguous `//` or `/* */` lines directly above the definition
  become a single-space-joined `doc_description`; `@param name text` annotates
  parameters and `@tags a, b` populates `tags`.
- **Ordering.** Discoveries are sorted by `(line, qualified_name)` and deduped
  by that pair, matching the sibling adapters.

## Type-index mapping

`collect_types` returns a `BTreeMap<String, ReturnShape>` keyed by bare type
name. Structs and unions contribute their named fields; a typedef with an
inline record body indexes those fields under the typedef name; a typedef of a
named record is an alias resolved after all declarations in the file are
indexed, so forward references work. Empty entries are omitted.
`resolve_return_fields` delegates to the shared grammar-free resolver in
`crate::resolve`; the C adapter only strips C-specific `struct`/`union`/`enum`
and pointer syntax before lookup.

## Body-analysis mapping

- **Steps.** Top-level statements of the `compound_statement` are visited in
  source order and get ids `step-1`, `step-2`, ... . `code` is the statement
  normalized to a single line; `inputs` are referenced identifiers in first-seen
  source order; `output` is the return type, a guard result, or the assigned
  name and type. Titles and details come from the two `//` lines above the
  statement, with deterministic fallbacks.
- **Kinds (spec §4 step 3).** `return` → `Return`; `if`/`switch` → `Branch`; a
  declaration with a call initializer → `Call`; any other statement containing
  a call → `Call`; arithmetic, assignment without a call and other data mapping
  → `Transform`.
- **Calls.** Each `call_expression` contributes a bare callee name (the last
  identifier of `a.b(...)`). `callee_names` collects all of them in step order.
  A `StepKind::Call` step also carries the first callee that appears in
  `known_names` as its raw `calls_fn_id`, which core resolves to a graph id.
- **Errors.** C has no `throw`, so three idioms are mapped: `fprintf(stderr,
  "CODE")` yields the literal as the code; `return -1` and `return NULL` yield
  `ERR` and `NULL` but only inside an `if` guard; `assert(expr)` yields `ASSERT`.
  `when` is the nearest enclosing `if` condition, otherwise `explicit stderr
  report`, `error return` or `assertion failed`. Sites are de-duplicated by code
  in source order, first occurrence wins.
- **Status mapping.** `422` for `TOKEN_NOT_FOUND`, `VALIDATION_ERROR`,
  `INVALID_INPUT`; `400` for `EMPTY_TEXT`, `EMPTY_SENTENCE`,
  `INSECURE_PROTOCOL`, `BAD_REQUEST`, `ERR`, `NULL`, `ASSERT`; `404` for
  `NOT_FOUND`; `401` for `UNAUTHORIZED`; `403` for `FORBIDDEN`; `500` otherwise.
- **Purity and permissions.** `printf`, `puts` and `fprintf` give a side effect
  and `console:out`; `fopen`/`fread` give `fs:read`; `fwrite`/`remove` give
  `fs:write`; `socket`/`connect`/`curl_*` give `net:outbound`;
  `getenv("KEY")` gives `secrets:read:KEY`. No marker means `Pure` with no
  permissions. Markers are read from call expressions in the function's AST,
  not from comments or string contents. Purity is not propagated through callee
  names, matching the sibling adapters.
- **Budgets.** Pure functions get `timeout_ms = 150` and `runtime =
  "universal"`; side-effecting functions get `1000` and `"node20"`; network
  functions get `5000` and `"node20"`. The runtime string is an fnscope
  execution label, not a claim that C runs on Node.

## Findings for files outside this job's ownership

These are reported, not edited:

1. `Cargo.toml` `[workspace.dependencies]` (near lines 17–18, after
   `tree-sitter-typescript`) needs `tree-sitter-c = "0.24.2"`. The crate is
   cached in the WSL Debian registry; it depends on `tree-sitter-language 0.1`
   and dev-depends on `tree-sitter 0.25.4`, matching this workspace's
   `tree-sitter = "0.25"` conversion.
2. `crates/fnscope-parser/Cargo.toml` `[dependencies]` needs
   `tree-sitter-c.workspace = true` after line 13
   (`tree-sitter-go.workspace = true`).
3. `crates/fnscope-parser/src/lib.rs` needs `pub mod c_lang;` (after line 7,
   `pub mod golang;`). Without it the adapter module and its unit tests are not
   compiled. No change is needed in `resolve.rs`: `resolve_fields` and
   `resolve_fields_from_map` are already `pub(crate)`.
4. `crates/fnscope-parser/src/adapter.rs` needs `use crate::c_lang::CAdapter;`
   and a `registry.register(Box::new(CAdapter::new()));` line inside
   `Registry::with_builtin_adapters` (lines 67–77). This job deliberately does
   not add either. If registration lands, extend the
   `builtin_registry_resolves_every_extension` test (lines 108–124) with
   `("c", "c")`.
5. `crates/fnscope-core/src/scan.rs` `ScanOptions::default()` (line 30) needs
   `"**/*.c"` in `includes`, and the `default_options_match_the_contract`
   expectation (line 519) needs the same entry, otherwise a fixture scan finds
   no C files.
