# Swift adapter contract (J39)

This document records the mapping implemented by the Swift adapter in
`crates/fnscope-parser/src/swift/`. It adds no field to the locked schema in
`docs/contract/rust-api.md`: every fact is returned as an unchanged
`DiscoveredFn`, `BodyAnalysis`, `Step`, `ErrorSpec` or `ReturnShape`.

The adapter is **not registered globally** by this job. Registration is a
separate, reviewed change; until then a caller must build a `Registry`, call
`register(Box::new(SwiftAdapter::new()))` and pass it to `scan_with_registry`.

## Identity and extensions

| Item | Value | Reason |
|---|---|---|
| `SwiftAdapter::id()` | `"swift"` | Stable diagnostics name. |
| `SwiftAdapter::extensions()` | `&["swift"]` | `LanguageAdapter::extensions` documents suffixes **without** a leading dot; `Registry::for_extension` trims a caller's dot and lowercases before comparing, so `.swift`, `swift` and `SWIFT` all resolve. |
| `SwiftAdapter::new()` / `Default` | stateless value | Registration needs no configuration or global state. |

## Grammar source and compatibility

The cached crate is `tree-sitter-swift-0.7.4` in the Cargo registry source
cache; the adapter reads it through the ordinary crate dependency, never a
hardcoded path.

- **Exported constant verified from source** — `bindings/rust/lib.rs` declares
  `pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_swift) };`.
  There is **no** `LANGUAGE_SWIFT`. The crate's own test calls
  `parser.set_language(&super::LANGUAGE.into())`.
- The adapter uses the same conversion,
  `let language: tree_sitter::Language = tree_sitter_swift::LANGUAGE.into();`,
  that the existing Rust adapter uses for `tree_sitter_rust::LANGUAGE`.
- **Compatibility finding (no workaround invented).** `tree-sitter-swift` 0.7.4
  depends on `tree-sitter-language = "0.1.0"`; the workspace `tree-sitter`
  0.25.10 depends on `tree-sitter-language = "0.1"` (resolved 0.1.9), and the
  already-integrated `tree-sitter-rust` 0.24.2 also uses `0.1` with the identical
  `.into()` pattern. The conversion therefore type-checks against the workspace
  grammar runtime. The grammar's own dev-dependency on `tree-sitter 0.23` only
  applies to that crate's internal tests and is not a constraint here.
- A parse failure yields an empty `Vec`/map or `BodyAnalysis::default()`; no
  partial manifest is invented.

## Node kinds consumed

| Node kind | Role |
|---|---|
| `function_declaration` | Free function at file scope; method when a direct child of a type body. |
| `class_declaration` | **One node for `class`, `struct`, `actor`, `extension` and `enum`**; the `declaration_kind` field selects which. Only the first four are scanned for callables. |
| `init_declaration` | Initializer, recorded as `TypeName.init` with `FnKind::Method`. |
| `parameter` | External label (`external_name`), internal name (`name`) and type (`type`); there is no `parameter_list` node in this grammar. |
| `default_value` | Function-node field holding a default expression; mapped to the preceding parameter by source byte order. |
| `return_type` | Function-node field for the text after `->`. |
| `type_annotation`, `user_type`, `type_identifier` | Declared types of stored properties, parameters and returns. |
| `typealias_declaration` | `name` plus aliased `value`; fields are copied from the target after all types are indexed. |
| `property_declaration` | Stored property in the type index; computed values are skipped. |
| `comment`, `multiline_comment` | Contiguous leading `//`/`///` block for `doc_description`, `- Parameter`/`@param` texts and `@tags`. |
| `call_expression`, `constructor_expression` | Call steps and `callee_names`; the latter covers `Data(...)`-style initializers. |
| `if_statement`, `guard_statement`, `switch_statement`, `do_statement` | Branch steps; `condition`/`expr` is the guard text, `catch` for do-catch. |
| `assignment`, `property_declaration`, expression statements | `Call` when the initializer/statement invokes a function, `Transform` otherwise. |
| `control_transfer_statement`, `throw_keyword` | `Return` steps and throw sites. |
| `value_argument`, `value_argument_label` | `code:` argument of a thrown error object; external/internal parameter labels. |
| `line_string_literal`, `multi_line_string_literal`, `raw_string_literal` | Error-code literals and string default values. |
| `function_body`, `statements` | Ordered top-level statements of the body. |
| `function_declaration` (nested), `lambda_literal` | Ignored: a local function or closure is not a separate callable and its calls are not attributed to the enclosing step. |

## Wrapper nodes handled

The cached `src/node-types.json` was inspected before any child filter was
written. `tree-sitter-swift` 0.7.4 is older than the workspace `tree-sitter
0.25`, so child filters are written against the node kinds actually present in
that file, not against a newer grammar's shape. Every wrapper this adapter
handles, explicitly or by visiting through it, is named here:

| Wrapper (from `node-types.json`) | Handling |
|---|---|
| `function_body -> statements` | **Explicitly unwrapped** in `body::analyze` to reach the ordered top-level statements. |
| `statements` (70 child expression/declaration kinds) | Its named children are the step list; comments are filtered out. |
| `class_declaration` (`declaration_kind` field) | Grammar-level wrapper for class/struct/actor/extension/enum; filtered by field text. There are no `struct_declaration`, `extension_declaration`, `struct_body` or `extension_body` nodes. |
| `parameter` (`external_name`, `name`, `type` fields) | Read directly; no `parameter_list` wrapper exists in this grammar. |
| `type_annotation -> type` | Field lookup for property and local-binding types. |
| `user_type -> type_identifier + type_arguments` | Visited through for return types and constructed types; `type_arguments` are excluded from step inputs. |
| `optional_type -> wrapped`, `array_type`, `dictionary_type`, `metatype`, `function_type` | Visited through as declared-type text; excluded from step inputs. |
| `directly_assignable_expression` | Assignment target wrapper; `assigned_name` reads its text. |
| `assignment` (`target`, `operator`, `result`) | Only the `result` side contributes step inputs. |
| `property_declaration` (`name`, `value`, `computed_value`) | Only the `value` side contributes inputs; `computed_value` marks a computed property, skipped in the type index. |
| `computed_property -> statements` | Skipped when indexing stored properties. |
| `call_expression -> _expression + call_suffix` | First named child is the callee target; the last identifier of that subtree is the bare callee name. |
| `call_suffix -> value_arguments` | Traversed when collecting calls and inputs, but never used as the callee. |
| `value_arguments -> value_argument` | Walked to find the `code:` argument. |
| `value_argument_label -> simple_identifier` | Used as the label text for external/internal names; excluded from step inputs. |
| `identifier -> simple_identifier` | Visited through when gathering identifiers. |
| `type_modifiers -> attribute`, `parameter_modifiers -> parameter_modifier` | Skipped as inputs: they are modifiers, not referenced values. |
| `lambda_function_type_parameters -> lambda_parameter` | Function-type text only; never a value input. |
| `lambda_literal -> statements` | Skipped by call collection so a closure's calls are not attributed to the enclosing step. |
| `navigation_expression -> target + navigation_suffix` | Member-access wrapper: reported as one input path, and its last identifier supplies a callee or error code. |
| `constructor_expression -> constructed_type + constructor_suffix` | Treated as a call; `constructed_type` supplies the callee (`Data`, `URL`, `Grade`, ...). |
| `try_expression`, `await_expression` | Transparent wrappers around calls; visited by `has_call`/`visit_calls`. |
| `control_transfer_statement -> throw_keyword` | Throw detection ignores the keyword child and reads the remaining expression. |
| `else` (inside `guard_statement`/`if_statement`) | Parent chain used to derive a throw's `when` condition. |
| `switch_pattern -> pattern`, `capture_list -> capture_list_item`, `discard_statement -> self_expression`, `modify_specifier/setter_specifier -> mutation_modifier`, `precedence_group_attributes -> precedence_group_attribute`, `type_parameter_modifiers -> attribute`, `external_macro_definition -> value_arguments` | Not specially unwrapped: they never occur as a top-level body statement or as a declared type in the shapes this adapter reads. They are traversed generically when present. |

## Discovery mapping

- **Names.** A free function keeps its bare name; a method or initializer is
  `TypeName.name` with `class_name = Some("TypeName")` and `FnKind::Method`.
- **Lines and source.** `line`/`end_line` are 1-based from the node positions;
  `source_code` is the exact node byte slice.
- **Parameters.** `name` is the *internal* name used in the body. When an
  external label differs (`func f(label name: Int)`), the label is retained in
  `description` as `external label: label`; the contract has no dedicated field
  for it. A `= expression` default sets `required = false` and a JSON value
  (`3`, `true`, or the expression text for non-literals). Unnamed parameters are
  skipped because there is no contract name to report.
- **Results.** The text after `->` is kept verbatim (for example `Grade`,
  `[String]`, `Int?`) and its fields are resolved through the shared type index.
  An absent return type is an empty type string, matching the Rust adapter.
- **Docs.** Contiguous `//`/`///` lines above the declaration become a
  single-space-joined `doc_description`; `- Parameter name: text` and `@param`
  annotate parameters and `@tags a, b` populate `tags`.
- **Ordering.** Discoveries are sorted by `(line, qualified_name)` and deduped by
  that pair, matching the other adapters.

## Type-index mapping

`collect_types` returns a `BTreeMap<String, ReturnShape>` keyed by bare type
name. `class`/`struct`/`actor` declarations contribute their stored properties
(computed properties are ignored), in source order. Type aliases are resolved
in a second pass after all named types are indexed, so an alias can point to a
type declared later in the file. Empty entries are omitted.
`resolve_return_fields` delegates to the shared grammar-free resolver in
`crate::resolve`; the adapter does not duplicate that logic.

## Body-analysis mapping

- **Steps.** Top-level statements of the unwrapped `statements` node are visited
  in source order and get ids `step-1`, `step-2`, ... . `code` is the statement
  normalized to a single line; `inputs` are referenced identifiers and selector
  paths in first-seen source order; `output` is the return type, a guard result,
  or the assigned name and type. Titles and details come from the two `//` lines
  above the statement, with deterministic fallbacks.
- **Step kinds.** A top-level `if`/`guard`/`switch`/`do-catch` is `Branch`; a
  `return` control-transfer statement is `Return`; any statement containing a
  `call_expression` or `constructor_expression` (including an assignment with a
  call initializer, spec §4 step 3) is `Call`; everything else is `Transform`.
- **Calls.** Each call contributes a bare callee name (the last identifier of
  `a.b.c(...)`, so `Grade.describe` yields `describe` and a local `normalize`
  yields `normalize`). `callee_names` collects all of them in step order. A
  `Call` step also carries the first callee appearing in `known_names` as its
  raw `calls_fn_id`, which core resolves to a graph id.
- **Errors.** `throw X.code`, `throw Error(code: "CODE")`, string literals and
  bare identifiers produce `ErrorSpec { code, status, when }`. `when` is the
  nearest enclosing `guard`/`if` condition, otherwise `explicit throw`. Sites
  are de-duplicated by code, first occurrence wins.
- **Status mapping.** `422` for `TOKEN_NOT_FOUND`, `VALIDATION_ERROR`,
  `INVALID_INPUT`; `400` for `EMPTY_TEXT`, `EMPTY_SENTENCE`, `INSECURE_PROTOCOL`,
  `BAD_REQUEST`, `invalid`; `429` for `RATE_LIMITED`; `404` for `NOT_FOUND`;
  `401` for `UNAUTHORIZED`; `403` for `FORBIDDEN`; `500` otherwise.
- **Purity and permissions.** `print`/`debugPrint`/`os_log` give `console:out`;
  `FileManager`/`Data(contentsOf:)` give `fs:read`; `URLSession`/`URLRequest`
  give `net:outbound`; `ProcessInfo.processInfo.environment["KEY"]` gives
  `secrets:read:KEY`. No marker means `Pure` with no permissions. This is a
  conservative text classification of the declaration slice, matching the
  sibling adapters.
- **Budgets.** The contract labels are reused unchanged: pure functions get
  `timeout_ms = 150` and `runtime = "universal"`; console/filesystem/secrets
  effects get `1000` and `"node20"`; network effects get `5000` and `"node20"`.
  The runtime string is an fnscope execution label, not a claim that Swift runs
  on Node.

## Findings for files outside this job's ownership

These are reported, not edited:

1. `Cargo.toml` `[workspace.dependencies]` (lines 18–36) needs
   `tree-sitter-swift = "0.7.4"` to match the cached crate.
2. `crates/fnscope-parser/Cargo.toml` `[dependencies]` (lines 8–17) needs
   `tree-sitter-swift.workspace = true`; without it the new module cannot
   resolve the grammar crate.
3. `crates/fnscope-parser/src/lib.rs` (lines 4–11) needs `pub mod swift;`;
   without it the adapter module and its unit tests are not compiled.
4. `crates/fnscope-parser/src/adapter.rs:67-77`
   `Registry::with_builtin_adapters` needs
   `registry.register(Box::new(SwiftAdapter::new()));` plus the matching import.
   This job deliberately does not add one.
5. `crates/fnscope-core/src/scan.rs:30` and `:519` need `**/*.swift` in the
   default `includes`, and the default-includes assertion, otherwise a fixture
   scan finds no Swift files.
6. The mixed polyglot fixture under `tests/fixtures/` needs a Swift file added
   by the registration job so one manifest contains Swift alongside the other
   languages.
