# Go adapter contract (J24)

This document records the mapping implemented by the Go adapter in
`crates/fnscope-parser/src/golang/`. It adds no field to the locked schema in
`docs/contract/rust-api.md`: every fact is returned as an unchanged
`DiscoveredFn`, `BodyAnalysis`, `Step`, `ErrorSpec` or `ReturnShape`.

The adapter is **not registered globally** by this job. Registration is a
separate, reviewed change; until then a caller must build a `Registry`, call
`register(Box::new(GoAdapter::new()))` and pass it to `scan_with_registry`.

## Module name

The directory is `golang/`, not `go/`: `go` is a Rust keyword and cannot be used
as a module identifier. The public type is `GoAdapter` and its id is `"go"`.

## Identity and extensions

| Item | Value | Reason |
|---|---|---|
| `GoAdapter::id()` | `"go"` | Stable diagnostics name. |
| `GoAdapter::extensions()` | `&["go"]` | `LanguageAdapter::extensions` documents suffixes **without** a leading dot; `Registry::for_extension` trims a caller's dot and lowercases before comparing, so `.go`, `go` and `GO` all resolve. |
| `GoAdapter::new()` / `Default` | stateless value | Registration needs no configuration or global state. |

## Grammar source

The adapter uses `tree_sitter_go::LANGUAGE`, converted into a
`tree_sitter::Language` exactly as the Rust adapter converts
`tree_sitter_rust::LANGUAGE`. The dependency is not added by this job (see
Findings). A parse failure yields an empty `Vec`/map or
`BodyAnalysis::default()`; no partial manifest is invented.

## Node kinds consumed

| Node kind | Role |
|---|---|
| `function_declaration` | Free function; direct children of the source file only. |
| `method_declaration` | Method, with `receiver` used for the class name. |
| `parameter_list`, `parameter_declaration` | Parameters and parenthesized results; `name` may repeat for `a, b int`. |
| `func_literal` | Ignored: a closure inside a body is not a separate callable and its calls are not attributed to the enclosing step. |
| `type_spec`, `struct_type`, `field_declaration` | Struct fields in the type index. |
| `interface_type`, `method_elem` | Interface methods in the type index, typed as compact `func(...) ...` signatures. |
| `comment` | Contiguous leading `//` block for `doc_description`, `@param` texts and `@tags`. |
| `call_expression`, `go_statement`, `defer_statement` | Call steps and `callee_names`. |
| `if_statement`, `expression_switch_statement`, `type_switch_statement`, `select_statement` | Branch steps; `condition` or `value` is the guard text. |
| `short_var_declaration`, `var_declaration`, `assignment_statement` | `call` when the initializer invokes a function, `transform` otherwise; inferred output labels. |
| `return_statement` | Return step and propagated `err` detection. |
| `interpreted_string_literal`, `raw_string_literal` | Error codes inside `panic`/`errors.New`/`fmt.Errorf`. |

## Discovery mapping

- **Names.** A free function keeps its bare name; a method is recorded as
  `ReceiverType.Method` with `class_name = Some("ReceiverType")` and
  `FnKind::Method`. Pointer (`*Service`), qualified (`pkg.Service`) and generic
  (`Service[T]`) receivers reduce to the bare type name.
- **Lines and source.** `line`/`end_line` are 1-based from the node positions;
  `source_code` is the exact node byte slice.
- **Parameters.** Every `name` of a `parameter_declaration` becomes a required
  `Parameter` with its declared type text. Grouped names (`a, b int`) become two
  parameters sharing the type. Unnamed parameters are skipped because there is
  no contract name to report. Go has no optional parameters, so
  `default_value` is always `None`.
- **Results.** A single result keeps its declared text (for example `Item` or
  `*Item`) and resolves fields through the shared type index. A parenthesized
  result becomes `(T1, T2, ...)`; named results additionally populate
  `ReturnShape.fields` in source order. An absent result is `void`.
- **Docs.** Contiguous `//` lines directly above the declaration become a
  single-space-joined `doc_description`; `@param name text` lines annotate
  parameters and `@tags a, b` populate `tags`.
- **Ordering.** Discoveries are sorted by `(line, qualified_name)` and deduped
  by that pair, matching the other adapters.

## Type-index mapping

`collect_types` returns a `BTreeMap<String, ReturnShape>` keyed by bare type
name, the same shape used by the built-in adapters. Structs contribute their
named fields (including grouped declarations); interfaces contribute method
entries; a simple alias copies the fields of its target after all declarations
in the file are indexed. Empty entries are omitted. `resolve_return_fields`
delegates to the shared grammar-free resolver in `crate::resolve` — the Go
adapter does not duplicate that logic, it only strips Go-specific `*`, `[]`,
package and generic syntax before lookup.

## Body-analysis mapping

- **Steps.** Top-level statements of the function body are visited in source
  order and get ids `step-1`, `step-2`, ... . `code` is the statement normalized
  to a single line; `inputs` are referenced identifiers and selector paths in
  first-seen source order; `output` is the return type, a guard result, or the
  assigned name and type. Titles and details come from the two `//` lines above
  the statement, with deterministic fallbacks.
- **Calls.** Each `call_expression` contributes a bare callee name (the last
  identifier of `a.b.c(...)`, so `fmt.Println` yields `Println` and a local
  function `normalize` yields `normalize`). `callee_names` collects all of them
  in step order. A `StepKind::Call` step also carries the first callee that
  appears in `known_names` as its raw `calls_fn_id`, which core resolves to a
  graph id. `go fn()` and `defer fn()` are Call steps.
- **Errors.** `panic("CODE")`, `errors.New("CODE")` and `fmt.Errorf("CODE")`
  produce `ErrorSpec { code, status, when }`. `return value, err` (or
  `return err`) produces the stable fallback code `ERR`. `when` is the nearest
  enclosing `if` condition, otherwise `explicit panic`, `explicit error` or
  `propagated error`. Sites are de-duplicated by code, first occurrence wins.
- **Status mapping.** `422` for `TOKEN_NOT_FOUND`, `VALIDATION_ERROR`,
  `INVALID_INPUT`; `400` for `EMPTY_TEXT`, `EMPTY_SENTENCE`,
  `INSECURE_PROTOCOL`, `BAD_REQUEST`; `404` for `NOT_FOUND`; `401` for
  `UNAUTHORIZED`; `403` for `FORBIDDEN`; `500` otherwise.
- **Purity and permissions.** `fmt.Print*`/`log.` give a side effect and
  `console:out`; `os.Open`/`os.ReadFile`/`ioutil` reads give `fs:read`;
  `os.WriteFile`/`os.Create`/`ioutil.WriteFile` give `fs:write`;
  `net/http`, `http.Get`/`Post`/`Head`/`Client`/`NewRequest` give
  `net:outbound`; `os.Getenv("KEY")` gives `secrets:read:KEY`. No marker means
  `Pure` with no permissions. This is a conservative text classification of
  the function slice, matching the sibling adapters.
- **Budgets.** The contract labels are reused unchanged: pure functions get
  `timeout_ms = 150` and `runtime = "universal"`; side-effecting functions get
  `1000` and `"node20"`; network functions get `5000` and `"node20"`. The
  runtime string is an fnscope execution label, not a claim that Go runs on
  Node.

## Findings for files outside this job's ownership

These are reported, not edited:

1. `Cargo.toml` `[workspace.dependencies]` needs `tree-sitter-go = "0.25"` to
   match the cached crate and the workspace `tree-sitter = "0.25"`.
2. `crates/fnscope-parser/Cargo.toml` `[dependencies]` needs
   `tree-sitter-go.workspace = true`.
3. `crates/fnscope-parser/src/lib.rs` needs `pub mod golang;`; without it the
   adapter module and its unit tests are not compiled.
4. `crates/fnscope-parser/src/adapter.rs` `Registry::with_builtin_adapters`
   needs a registration line. This job deliberately does not add one.
5. `crates/fnscope-core/src/scan.rs` `ScanOptions::default()` needs `**/*.go`
   in `includes`, and its default-includes test expectation needs the same
   entry, otherwise a fixture scan finds no Go files.
