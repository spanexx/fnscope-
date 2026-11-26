# C# adapter contract (J36)

This document records the mapping implemented by the C# adapter in
`crates/fnscope-parser/src/csharp/`. It adds no field to the locked schema in
`docs/contract/rust-api.md`: every fact is returned as an unchanged
`DiscoveredFn`, `BodyAnalysis`, `Step`, `ErrorSpec` or `ReturnShape`.

The adapter is **not registered globally** by this job. Registration is a
separate, reviewed change; until then a caller must build a `Registry`, call
`register(Box::new(CSharpAdapter::new()))` and pass it to `scan_with_registry`.

## Identity and extensions

| Item | Value | Reason |
| --- | --- | --- |
| `CSharpAdapter::id()` | `"csharp"` | Stable diagnostics name. |
| `CSharpAdapter::extensions()` | `&["cs"]` | `LanguageAdapter::extensions` documents suffixes **without** a leading dot; `Registry::for_extension` trims a caller's dot and lowercases, so `.cs`, `cs` and `CS` all resolve. |
| `CSharpAdapter::new()` / `Default` | stateless value | Registration needs no configuration or global state. |

## Grammar source

The adapter uses `tree_sitter_c_sharp::LANGUAGE` converted with
`let language: tree_sitter::Language = tree_sitter_c_sharp::LANGUAGE.into();`
exactly as the Python and Go adapters convert their constants. The constant was
verified from the cached crate source:

- `tree-sitter-c-sharp-0.24.0/bindings/rust/lib.rs:28` —
  `pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_c_sharp) };`
- `LanguageFn` comes from `tree-sitter-language 0.1`
  (`tree-sitter-c-sharp-0.24.0/Cargo.toml`), so it converts into the workspace's
  `tree-sitter 0.25` without a version-specific `Into` impl in this crate.
- The adapter builds a fresh `Parser` in `parse_tree` (discovery) and in
  `analyze` (body); a parse failure yields an empty `Vec`/map or
  `BodyAnalysis::default()`. No partial manifest is invented.

## Wrapper nodes handled (read from `node-types.json`)

The C# grammar reports nodes that carry no fact themselves but wrap the node
that does. Each wrapper below is named and handled explicitly. Supertypes that
tree-sitter never materialises (`declaration`, `statement`, `expression`,
`lvalue_expression`, `type`) are listed too: the code matches the concrete
children, not the abstract name.

| Wrapper | Where it appears | How the adapter handles it |
| --- | --- | --- |
| `compilation_unit` → `namespace_declaration`, `type_declaration`, `global_statement` | File root | Only namespaces, the concrete `type_declaration` members (`class_declaration`, `struct_declaration`, `record_declaration`) and top-level statements are walked. `type_declaration` is an abstract supertype in `node-types.json`; its concrete children are matched. |
| `file_scoped_namespace_declaration` | File root | It has a `name` field but **no** `body` field; its members are siblings in `compilation_unit`, so they are reached by the root loop and no descent is attempted. |
| `namespace_declaration` → `declaration_list` | `body` field | The `declaration_list` wrapper is unwrapped and its concrete members visited; nested namespaces recurse. |
| `class_declaration` / `struct_declaration` / `record_declaration` → `declaration_list` | `body` field | Unwrapped before looking for `method_declaration` and `constructor_declaration`; `field_declaration` and `property_declaration` members feed the type index. |
| `declaration_list` → `declaration` | Body children | `declaration` is an abstract supertype; the code matches concrete `method_declaration`, `constructor_declaration`, nested type and property/field kinds directly. |
| `class_declaration` / `struct_declaration` / `record_declaration` → `parameter_list` | Unnamed child | A C# 12 primary constructor is discovered from this wrapper; its parameters come from the `parameter` children. |
| `method_declaration` → `block` / `arrow_expression_clause` | `body` field | `block` yields ordered top-level statements; `arrow_expression_clause` yields one implicit `Return` step built from its `expression` child. |
| `constructor_declaration` → `block` / `arrow_expression_clause` | `body` field | Same body handling; constructors have no `returns` field, so their result is `void`. |
| `local_function_statement` → `block` / `arrow_expression_clause` | `body` field | First-level local functions are discovered with an owner-qualified name. `record_local_function` deliberately does **not** recurse, so a nested local function is skipped; body analysis filters `local_function_statement` out of steps and call/input walks. |
| `global_statement` → `statement` | File root | Unwrapped to find top-level local functions; `statement` is an abstract supertype, so the concrete child kind is matched. |
| `parameter_list` → `parameter` | `parameters` field / primary-constructor child | `attribute_list` and `preproc_if_in_attribute_list` children are ignored; only `parameter` nodes become `Parameter`s. |
| `parameter` → `type` field and `expression` child | Parameter node | `type` is an abstract hidden (`named: false`) supertype, so its concrete node text is copied verbatim. The optional default is an `expression` child after `=`, **not** a field, and is found by child-kind scan. |
| `block` → `statement`, `comment` | Body field | `statement` is an abstract supertype: concrete statements are direct children and are visited in source order. `comment` extras are filtered from steps. |
| `local_declaration_statement` → `variable_declaration` → `variable_declarator` | Body statement | `variable_declaration` has a `type` field; each `variable_declarator` has a `name` field and an optional `expression` initializer. A call in that initializer makes the step a `Call` (spec §4 step 3), otherwise `Transform`. |
| `field_declaration` → `variable_declaration` → `variable_declarator` | Type member | Same unwrapping feeds one `ReturnField` per declarator (`int Count, Total;` gives two). |
| `property_declaration` → `name`, `type` fields | Type member | One `ReturnField` per property. |
| `expression_statement` → `assignment_expression` / `invocation_expression` / `object_creation_expression` | Body statement | The single expression child is classified: invocation or construction is `Call`, pure assignment/arithmetic is `Transform`. |
| `assignment_expression` → `left`, `operator`, `right` | Expression child | `left` supplies the assigned-name display output; `right` is visited for inputs. |
| `invocation_expression` → `function`, `argument_list` → `argument` | Call sites | `function` supplies the last identifier as the bare callee; `argument` children (with an optional `name` field) supply inputs. The callee itself is not counted as an input. |
| `object_creation_expression` → `type`, `arguments`, `initializer` | `new X(...)` | `type` supplies the bare constructed name (a callee and a throw-code fallback); arguments and initializer supply inputs. |
| `if_statement` → `condition`, `consequence`, `alternative` | Body statement | `condition` is the `Branch` guard and the `when` of throws below it. |
| `switch_statement` → `value`, `switch_body` → `switch_section` | Body statement | `value` is the `Branch` guard; `switch_body` is not descended into as a separate step. |
| `try_statement` → `body` (`block`), `catch_clause`, `finally_clause` | Body statement | One `Branch` step whose guard is the literal `try`. |
| `throw_statement` → `expression` | Error site | The expression is searched for `string_literal`, `verbatim_string_literal` or `raw_string_literal`; otherwise an `object_creation_expression` type supplies the code. |
| `return_statement` → `expression` | Body statement | One `Return` step; output is the declared return type or `void`. |
| `qualified_name` / `member_access_expression` → `qualifier`/`expression`, `name` | Dotted names | Copied as a single whitespace-free dotted input path; `bare_name` reduces them to the last segment for callees and error types. |
| `modifier` | Declaration children | An `async` modifier sets `is_async = true`; modifiers are not parameters. |
| `comment` | Anywhere | Extras are filtered from discovery descriptions and body steps; `//` / `///` text is read from source lines above the node instead. |

## Node kinds consumed

| Node kind | Role |
| --- | --- |
| `namespace_declaration`, `file_scoped_namespace_declaration`, `class_declaration`, `struct_declaration`, `record_declaration` | Namespace-level containers and types. |
| `method_declaration`, `constructor_declaration`, `local_function_statement` | Callables. |
| `parameter_list`, `parameter` | Parameters, defaults and primary constructors. |
| `field_declaration`, `property_declaration`, `variable_declaration`, `variable_declarator` | Type-index fields. |
| `block`, `arrow_expression_clause`, `expression_statement`, `local_declaration_statement` | Ordered body statements. |
| `if_statement`, `switch_statement`, `try_statement` | Branch steps and error guards. |
| `return_statement`, `throw_statement` | Return step and error sites. |
| `invocation_expression`, `object_creation_expression`, `argument_list`, `argument` | Calls, callee names and inputs. |
| `string_literal`, `verbatim_string_literal`, `raw_string_literal` | Thrown error codes and string inputs. |
| `comment` | Leading `//` / `///` descriptions and step titles/details. |

## Discovery mapping

- **Names.** A method is `ClassName.method` with `class_name = Some("ClassName")`
  and `FnKind::Method`. A regular or primary constructor is
  `ClassName.ClassName` with the same kind and `void` result. A first-level
  local function is `Owner.localName` with `FnKind::Declaration`; owner is the
  enclosing qualified callable name, or empty for top-level statements.
- **Lines and source.** `line`/`end_line` are 1-based from the node positions.
  `source_code` is the exact node byte slice; for a primary constructor it is
  the type header from its first byte through the `parameter_list` end.
- **Parameters.** `parameter` nodes supply `name`, exact declared `type` text
  (`unknown` when absent) and an optional default expression stored as
  `default_value = Some(Value::String(expr_text))` with `required = false`.
- **Results.** The `returns` field of a method or the `type` field of a local
  function is preserved verbatim, including generics and dotted names. Fields
  are resolved through `crate::resolve::resolve_fields_from_map` during
  discovery, then through `resolve_fields` for other files.
- **Docs.** Contiguous `//` / `///` lines directly above a declaration become a
  single-space-joined `doc_description`; XML tags are stripped. `tags` stays
  empty because no `@tags` convention is defined for C# in this contract.
- **Ordering.** Discoveries are sorted by `(line, qualified_name)` and deduped
  by that pair, matching the other adapters.

## Type-index mapping

`collect_types` returns a `BTreeMap<String, ReturnShape>` keyed by simple type
name. `class_declaration`, `struct_declaration` and `record_declaration`
contribute named properties, named fields (one per `variable_declarator`) and
primary-constructor parameters. Empty entries are omitted. Nested types are
walked too. `resolve_return_fields` delegates to the shared grammar-free
resolver in `crate::resolve`; this adapter never duplicates that logic.

## Body-analysis mapping

- **Steps.** Top-level statements of the function body are visited in source
  order and get ids `step-1`, `step-2`, ... . `code` is the statement collapsed
  to one line; `inputs` are referenced identifiers in first-seen source order;
  `output` is the return type, a guard result, or the assigned name and type.
  Titles and details come from the two `//` / `///` lines above the statement,
  with deterministic fallbacks (`Call name()`, `Guard: …`, `Transform name`,
  `Return <type>`). Nested local-function declarations emit no step.
- **Calls.** Each `invocation_expression` contributes the last identifier of its
  callee path and each `object_creation_expression` its bare constructed type as
  a raw callee. `callee_names` collects them in step order. A `StepKind::Call`
  step also carries the first callee present in `known_names` as `calls_fn_id`,
  which core maps to a graph id.
- **Step kinds.** `if` / `switch` / `try` are `Branch`; `return` is `Return`; a
  `local_declaration_statement` whose initializer invokes a function is `Call`
  (spec §4 step 3 and the locked schema example `const tokens = tokenize(...)`),
  otherwise `Transform`; a standalone call expression is `Call`; pure
  assignment/arithmetic is `Transform`. A top-level `throw` is a `Transform`
  step because the contract reserves `Branch` for `if`/`switch`/`try`; its
  failure is reported through `ErrorSpec`.
- **Errors.** `throw new FunctionError("CODE")` and `throw new X("CODE")` produce
  `ErrorSpec { code, status, when }`. `when` is the nearest enclosing `if` or
  `switch` guard, otherwise `explicit throw`. Sites are de-duplicated by code,
  first occurrence wins. A branch containing an error site reports
  `throws CODE | continues` as its output.
- **Status mapping.** `422` for `TOKEN_NOT_FOUND`, `VALIDATION_ERROR`,
  `INVALID_INPUT`; `400` for `EMPTY_TEXT`, `EMPTY_SENTENCE`,
  `INSECURE_PROTOCOL`, `BAD_REQUEST`; `404` for `NOT_FOUND`; `401` for
  `UNAUTHORIZED`; `403` for `FORBIDDEN`; `500` otherwise. This matches the
  Python, Go and Java adapters so the enricher needs no language branch.
- **Purity and permissions.** `Console.Write*` gives a side effect and
  `console:out` (`Console.Read*` gives `console:in`); `System.IO` / `File.` gives
  `fs:read`, plus `fs:write` for `File.Write*`/`Create`/`Delete`/`Copy`/`Move`/
  `Append`; `HttpClient` / `WebRequest` gives `net:outbound`;
  `Environment.GetEnvironmentVariable("KEY")` gives `secrets:read:KEY`. No
  marker means `Pure` with no permissions. This is a conservative text
  classification of the exact function slice, matching the sibling adapters.
- **Budgets.** Pure functions get `timeout_ms = 150` and `runtime =
  "universal"`; side-effecting functions get `1000` and `"node20"`; network
  functions get `5000` and `"node20"`. The runtime string is an fnscope
  execution label, not a claim that C# runs on Node.

## Fixture

`tests/fixtures/csharp_app/App.cs` is a coherent namespace with a result class,
a primary-constructor record, a constructor, a call edge (`Analyze` →
`Normalize`/`Add`), two guarded throws, a `Console.WriteLine` side effect, a
file read, an environment read, an `HttpClient` network call and a first-level
local function whose nested sibling is omitted. `README.md` lists the expected
functions and evidence. Glob exclusion stays core's job.

## Verification performed by this job

- `rustfmt --edition 2021 --check <the three owned .rs files>` ->
  `RUSTFMT_STATUS=0`. This is format-only evidence, **not** a compile check.
  Rustfmt parsed the files, but only the orchestrator's cargo gate proves that
  identifiers resolve.
- CR check via `tr -cd '\r' < file | wc -c` -> `0` for `mod.rs`, `discover.rs`,
  `body.rs`, `App.cs` and `README.md`. No `sed 's/\r$//'` was used at any point
  (L11); CR normalization would use `tr -d '\r'` only.
- Grammar constant verified from the cached crate source at
  `tree-sitter-c-sharp-0.24.0/bindings/rust/lib.rs:28`; wrapper nodes verified
  from `tree-sitter-c-sharp-0.24.0/src/node-types.json` and the relevant rules
  in `grammar.js` (`compilation_unit`, `record_declaration`, `parameter`,
  `variable_declarator`, `local_declaration_statement`,
  `local_function_statement`, `throw_statement`).

## Findings for files outside this job's ownership

These are reported, not edited:

1. `Cargo.toml` `[workspace.dependencies]` (lines 30–38, after line 32
   `tree-sitter-cpp = "0.23.4"`) needs `tree-sitter-c-sharp = "0.24"`. That is
   the version cached in the WSL registry; it depends on
   `tree-sitter-language 0.1`, so `LANGUAGE.into()` is compatible with the
   workspace's `tree-sitter 0.25`. Do not assume the `0.25` series used by
   Go/Python/JavaScript.
2. `crates/fnscope-parser/Cargo.toml` `[dependencies]` (lines 12–17, after line
   13 `tree-sitter-go.workspace = true`) needs
   `tree-sitter-c-sharp.workspace = true`. Without it the three owned `.rs`
   files cannot resolve `tree_sitter_c_sharp::LANGUAGE`.
3. `crates/fnscope-parser/src/lib.rs` (module list at lines 4–11, e.g. after
   line 5 `pub mod body;`) needs `pub mod csharp;`. Without it the module and
   its `#[cfg(test)]` tests are not compiled.
4. `crates/fnscope-parser/src/adapter.rs` needs
   `use crate::csharp::CSharpAdapter;` near lines 13–17 and a
   `register(Box::new(CSharpAdapter::new()))` line in
   `Registry::with_builtin_adapters` (lines 67–76). This job deliberately adds
   **neither**: global registration is a separate, reviewed change.
5. `crates/fnscope-core/src/scan.rs` `ScanOptions::default()` includes list
   (lines 29–41, after line 37 `**/*.java`) needs `**/*.cs`, and the matching
   expectation in `default_options_match_the_contract` (lines 529–541, after
   line 537 `**/*.java`) needs the same entry, otherwise a fixture scan finds
   no C# files. No graph-builder, schema, enricher or UI change is required;
   the plugin invariant holds.
