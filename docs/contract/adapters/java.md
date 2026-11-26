# Java adapter contract (J30)

This document records the mapping implemented by the Java adapter in
`crates/fnscope-parser/src/java/`. It adds no field to the locked schema in
`docs/contract/rust-api.md`: every fact is returned as an unchanged
`DiscoveredFn`, `BodyAnalysis`, `Step`, `ErrorSpec` or `ReturnShape`.

The adapter is **not registered globally** by this job. Registration is a
separate, reviewed change; until then a caller must build a `Registry`, call
`register(Box::new(JavaAdapter::new()))` and pass it to `scan_with_registry`.

## Identity and extensions

| Item | Value | Reason |
|---|---|---|
| `JavaAdapter::id()` | `"java"` | Stable diagnostics name. |
| `JavaAdapter::extensions()` | `&["java"]` | `LanguageAdapter::extensions` documents suffixes **without** a leading dot; `Registry::for_extension` trims a caller's dot and lowercases before comparing, so `.java`, `java` and `JAVA` all resolve. |
| `JavaAdapter::new()` / `Default` | stateless value | Registration needs no configuration or global state. |

## Grammar source

The adapter uses `tree_sitter_java::LANGUAGE` (verified in the cached crate at
`bindings/rust/lib.rs`: `pub const LANGUAGE: LanguageFn`, where `LanguageFn`
comes from `tree-sitter-language 0.1`) converted into a `tree_sitter::Language`
exactly as the Go adapter converts `tree_sitter_go::LANGUAGE`. The dependency is
not added by this job (see Findings). A parse failure yields an empty `Vec`/map
or `BodyAnalysis::default()`; no partial manifest is invented.

## Wrapper nodes handled (read from `node-types.json`)

The Java grammar reports several nodes that do not themselves carry a fact but
wrap the nodes that do. A previous batch failed by skipping these, so each one
is named here and handled explicitly.

| Wrapper | Where it appears | How the adapter handles it |
|---|---|---|
| `program` → `class_declaration` / `interface_declaration` / `record_declaration` | File root | Only these three kinds are walked at the root. Java has no module-level functions, so a method is never recorded without a type. |
| `class_declaration` → `class_body` | `body` field | `class_body` is unwrapped before looking for `method_declaration` and `constructor_declaration`; `field_declaration` members feed the type index. |
| `interface_declaration` → `interface_body` | `body` field | `interface_body` is unwrapped; a body-less `method_declaration` signature is still a `FnKind::Method` callable. |
| `record_declaration` → `class_body` and `formal_parameters` | `body` and `parameters` fields | Record methods come from the `class_body` wrapper; record components come from the `formal_parameters` wrapper. |
| `formal_parameters` → `formal_parameter` / `spread_parameter` / `receiver_parameter` | Method and constructor `parameters` field | `formal_parameter` supplies `name`/`type`; `spread_parameter` has no fields, so its `variable_declarator` child supplies the name and its other named child the type; `receiver_parameter` is not a caller argument and is skipped. |
| `variable_declarator` | `field_declaration`, `local_variable_declaration`, `spread_parameter` | Named through `children_by_field_name("declarator", ...)` / `child_by_field_name("name")`; multiple declarators (`int a, b;`) become multiple fields. |
| `block` → `statement` | Method/constructor `body` field, `static_initializer`, `catch_clause`, `finally_clause` | `statement` is an **abstract supertype** (listed in `supertypes` of `grammar.js`), so it is not a real node: concrete statements are direct children of `block`. `block_statements` unwraps defensively and filters comments. |
| `switch_block` → `switch_block_statement_group` / `switch_rule` | `switch_expression` `body` field | These concrete group nodes are unwrapped defensively so a grouped statement cannot be skipped; `switch_label` is filtered out. |
| `static_initializer` → `block` | `class_body` member | Deliberately ignored as a callable; its calls are not attributed to a method step. |
| `object_creation_expression` → anonymous `class_body` | `throw`/initializer expressions | The call is reported as a constructor call, but its anonymous class body is never descended during discovery. `OPAQUE_KINDS` also prevents a nested class's calls from being attributed to the enclosing step. |
| `lambda_expression` | Method bodies | Treated as opaque: its calls belong to the lambda, not the enclosing step. |
| `field_access` / `method_invocation` / `assignment_expression` | Statement expressions | `object`, `field`, `name` and `arguments` fields are read separately so referenced values become inputs without counting the callee as an input. |
| `argument_list` | `object_creation_expression` `arguments` field | First `string_literal` argument becomes the thrown error code. |

## Node kinds consumed

| Node kind | Role |
|---|---|
| `class_declaration`, `interface_declaration`, `record_declaration` | Top-level types; their direct members become callables. |
| `method_declaration`, `constructor_declaration` | Callables; `name`, `parameters`, `type` and `body` fields. |
| `formal_parameter`, `spread_parameter`, `formal_parameters` | Parameters. |
| `field_declaration`, `variable_declarator` | Class fields in the type index. |
| `line_comment` | Contiguous leading `//` block for `doc_description`, `@param` texts and `@tags`. |
| `method_invocation`, `object_creation_expression` | Call steps, `callee_names` and constructor calls. |
| `if_statement`, `switch_expression`, `try_statement`, `try_with_resources_statement` | Branch steps; `condition` is the guard text. |
| `local_variable_declaration`, `expression_statement`, `assignment_expression` | `call` when the initializer/right side invokes a function, `transform` otherwise. |
| `return_statement` | Return step. |
| `throw_statement`, `string_literal` | Error codes inside `throw new X("CODE")`. |

## Discovery mapping

- **Names.** Every callable is `ClassName.method` with
  `class_name = Some("ClassName")` and `FnKind::Method`; a constructor is
  `ClassName.ClassName`. There is no bare, module-level function in Java.
- **Lines and source.** `line`/`end_line` are 1-based from the node positions;
  `source_code` is the exact node byte slice.
- **Parameters.** `formal_parameter` and `spread_parameter` become required
  `Parameter`s with exact declared type text. Varargs keep their component type
  (for example `String... args` reports `String`). Java has no optional
  parameters in this grammar, so `default_value` is always `None`.
- **Results.** A method's declared `type` text is preserved (for example
  `String`, `java.util.List<String>`); constructors return `void`. Same-file
  return fields resolve during discovery; other files resolve through the
  shared type index.
- **Docs.** Contiguous `//` lines directly above the declaration become a
  single-space-joined `doc_description`; `@param name text` lines annotate
  parameters and `@tags a, b` populate `tags`.
- **Ordering.** Discoveries are sorted by `(line, qualified_name)` and deduped
  by that pair, matching the other adapters.

## Type-index mapping

`collect_types` returns a `BTreeMap<String, ReturnShape>` keyed by simple type
name, the same shape used by the built-in adapters. `class_declaration` fields
contribute their named fields (including `int a, b;` declarators);
`record_declaration` components contribute one field each. Empty entries are
omitted. `resolve_return_fields` delegates to the shared grammar-free resolver
in `crate::resolve`; the Java adapter does not duplicate that logic.

## Body-analysis mapping

- **Steps.** Top-level statements of the function body are visited in source
  order and get ids `step-1`, `step-2`, ... . `code` is the statement normalized
  to a single line; `inputs` are referenced identifiers in first-seen source
  order; `output` is the return type, a guard result, or the assigned name and
  type. Titles and details come from the two `//` lines above the statement,
  with deterministic fallbacks.
- **Calls.** Each `method_invocation` contributes its method name and each
  `object_creation_expression` its bare type name as a raw callee (so
  `System.out.println` yields `println` and `new RoleResult()` yields
  `RoleResult`). `callee_names` collects all of them in step order. A
  `StepKind::Call` step also carries the first callee that appears in
  `known_names` as its raw `calls_fn_id`, which core resolves to a graph id.
- **Step kinds.** `if`/`switch`/`try` are `Branch`; `return` is `Return`; a
  `local_variable_declaration` whose initializer invokes a function is `Call`
  (spec section 4 step 3 and the locked schema example `const tokens =
  tokenize(...)`), otherwise `Transform`; a standalone call expression is
  `Call`; pure assignment/arithmetic is `Transform`. A top-level `throw` is a
  `Transform` step because the contract reserves `branch` for `if`/`switch`/
  `try`; its failure is reported through `ErrorSpec`.
- **Errors.** `throw new FunctionError("CODE")` and `throw new X("CODE")` produce
  `ErrorSpec { code, status, when }`. `when` is the nearest enclosing
  `if`/`switch` condition, otherwise `explicit throw`. Sites are de-duplicated
  by code, first occurrence wins. A branch containing an error site reports
  `throws CODE | continues` as its output.
- **Status mapping.** `422` for `TOKEN_NOT_FOUND`, `VALIDATION_ERROR`,
  `INVALID_INPUT`; `400` for `EMPTY_TEXT`, `EMPTY_SENTENCE`,
  `INSECURE_PROTOCOL`, `BAD_REQUEST`; `404` for `NOT_FOUND`; `401` for
  `UNAUTHORIZED`; `403` for `FORBIDDEN`; `500` otherwise. `TOKEN_NOT_FOUND` →
  `422` follows the spec's worked example.
- **Purity and permissions.** `System.out.print*`/`System.err` give a side
  effect and `console:out`; `java.nio.file`/`Files.` give `fs:read` (plus
  `fs:write` for `Files.write`/`create`/`delete`/`copy`/`move`/`newOutputStream`);
  `java.net`/`HttpClient`/`HttpURLConnection`/`java.net.Socket` give
  `net:outbound`; `System.getenv("KEY")` gives `secrets:read:KEY`. No marker
  means `Pure` with no permissions. This is a conservative text classification
  of the function slice, matching the sibling adapters.
- **Budgets.** The contract labels are reused unchanged: pure functions get
  `timeout_ms = 150` and `runtime = "universal"`; side-effecting functions get
  `1000` and `"node20"`; network functions get `5000` and `"node20"`. The
  runtime string is an fnscope execution label, not a claim that Java runs on
  Node.

## Verification performed by this job

- `rustfmt --edition 2021 --check <the three owned .rs files>` ->
  `RUSTFMT_CHECK_STATUS=0`. This is format-only evidence, **not** a compile
  check. The orchestrator's cargo gate is the only compile evidence.
- CR check via `tr -cd '\r' < <file> | wc -c` -> `0` for each of `mod.rs`,
  `discover.rs`, `body.rs`, `java.md`, `Main.java` and `README.md`. No
  `sed 's/\r$//'` was used at any point (L11).
- Contract constant verified from the cached crate source:
  `pub const LANGUAGE: LanguageFn` at
  `tree-sitter-java-0.23.5/bindings/rust/lib.rs:37`; wrapper nodes verified from
  `tree-sitter-java-0.23.5/src/node-types.json` and the `supertypes` list in
  `grammar.js:56`.

## Findings for files outside this job's ownership

These are reported, not edited:

1. `Cargo.toml` `[workspace.dependencies]` (near line 30, after
   `tree-sitter = "0.25"`) needs
   `tree-sitter-java = "0.23.5"`. That is the version cached in the WSL
   registry and it exposes `LANGUAGE: LanguageFn` from
   `tree-sitter-language 0.1`, which converts into `tree_sitter 0.25` with
   `&LANGUAGE.into()`, exactly as the `0.23`-series TypeScript grammar does.
   Do not assume the `0.25` series used by Go/Python/JavaScript.
2. `crates/fnscope-parser/Cargo.toml` `[dependencies]` (after line 13,
   `tree-sitter-go.workspace = true`) needs `tree-sitter-java.workspace = true`.
3. `crates/fnscope-parser/src/lib.rs` needs `pub mod java;`; without it the
   adapter module and its unit tests are not compiled.
4. `crates/fnscope-parser/src/adapter.rs` `Registry::with_builtin_adapters`
   needs a `register(Box::new(JavaAdapter::new()))` line plus the matching
   `use crate::java::JavaAdapter;` import. This job deliberately adds neither.
5. `crates/fnscope-core/src/scan.rs` `ScanOptions::default()` needs `**/*.java`
   in `includes`, and its default-includes test expectation needs the same
   entry, otherwise a fixture scan finds no Java files.
