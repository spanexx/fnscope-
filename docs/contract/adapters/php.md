# Adapter contract — PHP (J38)

This page describes the PHP adapter's syntax-to-contract mapping. It is a
documentation-only addition: `docs/contract/rust-api.md` is unchanged, and no
schema field, `DiscoveredFn`, `BodyAnalysis`, `Step`, `ErrorSpec` or
`ReturnShape` was renamed or reshaped.

The adapter lives in `crates/fnscope-parser/src/php/`. It exposes exactly
`pub struct PhpAdapter` with `pub fn new() -> Self` and `impl Default`, and
implements all six `LanguageAdapter` methods from `docs/contract/adapter-api.md`.
It is **not** registered globally by J38; registration is a separate, reviewed
job.

## Grammar constant (verified before coding)

The cached crate is `tree-sitter-php-0.25.2`. Its
`bindings/rust/lib.rs` exports **two** `LanguageFn` constants:

- `pub const LANGUAGE_PHP: LanguageFn` — full PHP grammar (understands `<?php`,
  inline HTML and `?>`).
- `pub const LANGUAGE_PHP_ONLY: LanguageFn` — PHP-only fragment grammar.

There is **no** `LANGUAGE` constant in this crate. Both `discover.rs` and
`body.rs` use `tree_sitter_php::LANGUAGE_PHP.into()` assigned to an explicitly
typed `tree_sitter::Language`, matching the pattern used by the Python adapter.

## Identity

| Method | Value | Note |
| --- | --- | --- |
| `id()` | `"php"` | stable diagnostic name |
| `extensions()` | `&["php"]` | bare suffix; `Registry::for_extension` trims a leading dot |
| `resolve_return_fields` | `crate::resolve::resolve_fields` | the shared grammar-free helper, not a duplicate |

## Discovery (`src/php/discover.rs`)

Uses `tree_sitter_php::LANGUAGE_PHP` through `tree_sitter::Parser`. Discovery
walks program children and namespace bodies only, plus the direct members of a
class body. Results are sorted by `(line, qualified_name)` and deduplicated by
the same key. `relative_path` is never used to invent a module path.

| PHP node | Contract result |
| --- | --- |
| program-level `function_definition` | `FnKind::Declaration`, `name` from the `name` field, `qualified_name` = name |
| `namespace_definition` body member `function_definition` | same declaration shape |
| `class_declaration` body member `method_declaration` | `FnKind::Method`, `qualified_name` = `ClassName.method`, `class_name` = `ClassName` |
| `function_definition` nested in a body | ignored |
| `anonymous_function`, `arrow_function` | ignored |

`source_code` is the exact node slice, `line`/`end_line` are 1-based rows, and
`is_async` is always `false` because this grammar has no `async` declaration
keyword.

### Parameters and returns

| PHP syntax | Contract field |
| --- | --- |
| `simple_parameter` in `formal_parameters` | one `Parameter` |
| `variadic_parameter` (`...$rest`) | one optional `Parameter` (`required = false`) |
| `property_promotion_parameter` (constructor promotion) | one `Parameter` |
| `name` field under `variable_name` | `Parameter.name`, with leading `$` / `&` removed |
| `type` field (`int`, `?string`, `A\|B`, `App\Dto`) | `Parameter.type_`, exact source text; `"unknown"` when absent |
| `default_value` field | `Parameter.default_value = Some(Value::String(expr))`, `required = false` |
| no default | `required = true`, `default_value = None` |
| `: type` field `return_type` | `ReturnShape.type_`, exact source text; empty when absent, reported as `void` by the `Return` step |
| return fields | resolved through `crate::resolve::resolve_fields_from_map` only |

Stripping the `$` sigil is deliberate: `Parameter.name` is the identifier, not
the PHP sigil, so it stays comparable with the bare names used by `inputs` and
`callee_names`. `Parameter.description` is empty.

### Descriptions and type index

- `doc_description` prefers contiguous `//` lines directly above the
  declaration, otherwise one `/** ... */` block (including `*` continuation
  lines). Delimiters are stripped and lines are joined with single spaces.
- `collect_types` indexes each `class_declaration` under its bare class name as
  a `ReturnShape { type_: "object", fields }`. `property_declaration` →
  `property_element` contributes `name: declared type` (or an inferred literal
  type / `"unknown"`). `const_declaration` → `const_element` contributes the
  constant name with its declared type, or the value expression text when the
  constant is untyped. Functions declaring `: ClassName` then resolve those
  fields through the shared resolver.

## Body analysis (`src/php/body.rs`)

The declaration node is found by exact `source_code` match with a line+name
fallback; a miss returns `BodyAnalysis::default()`. Only the direct named
children of the declaration's `compound_statement` become steps. `comment`,
`text`, `php_tag` and `empty_statement` children are excluded.

| PHP node | `StepKind` | `code` / `inputs` / `output` |
| --- | --- | --- |
| `expression_statement` containing `function_call_expression`, `member_call_expression`, `nullsafe_member_call_expression` or `scoped_call_expression` | `Call` | single-line normalized source; identifier inputs; bare callee name; `calls_fn_id` when the callee is in `known_names` |
| `assignment_expression` / `augmented_assignment_expression` whose `right` contains a call | `Call` | spec §4 step 3: an assignment that invokes a function is a Call, not a Transform |
| `if_statement`, `else_if_clause`, `switch_statement`, `try_statement`, loops (`for`, `foreach`, `while`, `do`) | `Branch` | condition with one parenthesis layer removed; `throws <CODE> \| continues` when the branch contains a throw, else `<condition> \| continues` |
| plain assignment / arithmetic / other expressions | `Transform` | normalized source; right-hand identifiers as inputs; `$name: inferred type`, else `void` |
| `return_statement` | `Return` | `ReturnShape.type_` from discovery, or `void` |
| `echo_statement`, `print_intrinsic` | `Call` | callee `echo`; console output is a Call step, never a Return |
| `throw_expression` | `Transform` | normalized source; the failure itself is reported as an `ErrorSpec` |

`code` is collapsed to one line. `inputs` are referenced identifiers in
first-seen source order with `$` removed; literals, type nodes, keywords and
assignment targets are not inputs. `callee_names` are raw bare callee names in
step order, including calls nested in assignments, so core can resolve edges.

Titles and details come from the two contiguous `//` lines immediately above a
statement, with deterministic fallbacks (`Call name()`, `Guard: …`,
`Transform name`, `Return <type>`).

### Errors

`throw new X("CODE")`, `throw new FunctionError("CODE", ...)` and other
`throw_expression` forms yield an `ErrorSpec`. `code` is the first string
literal, falling back to the thrown class name; `when` is the nearest enclosing
`if` / `elseif` condition, or `explicit throw`. Sites are deduplicated by code
in source order. Status hints mirror the existing adapters:

| Code | Status |
| --- | --- |
| `TOKEN_NOT_FOUND`, `VALIDATION_ERROR`, `INVALID_INPUT` | 422 |
| `EMPTY_TEXT`, `EMPTY_SENTENCE`, `INSECURE_PROTOCOL`, `BAD_REQUEST` | 400 |
| `NOT_FOUND` | 404 |
| `UNAUTHORIZED` | 401 |
| `FORBIDDEN` | 403 |
| anything else | 500 |

### Purity, permissions and budgets

Derived from the whole function slice, so default expressions are considered:

| Marker | Effect |
| --- | --- |
| `echo`, `print(`, `printf(`, `print_r(`, `var_dump(` | `Purity::SideEffect`, `console:out` |
| `file_get_contents(`, `fopen(`, `readfile(`, `scandir(` | `fs:read` |
| `file_put_contents(`, `fwrite(`, `unlink(`, `mkdir(` | `fs:write` |
| `curl_*`, `stream_socket_client(`, `file_get_contents("http…")` | `net:outbound` |
| `getenv("KEY")`, `getenv('KEY')`, `$_ENV["KEY"]`, `$_ENV['KEY']` | `secrets:read:<KEY>` |

`file_get_contents` with an HTTP(S) URL contributes both `fs:read` and
`net:outbound`. The marker scan is a deterministic substring scan of the whole
slice, so a marker inside a string literal is also counted; this matches the
existing adapters' approach and does not change the schema.

Budgets and runtime labels are the contract values already used by the Rust,
TypeScript, Python and Go adapters: `150 ms` / `universal` for pure code,
`1000 ms` / `node20` for side effects, `5000 ms` / `node20` for network effects.
PHP code is not actually executed by the Node runner; these labels stay stable
so the schema and enricher need no language branch.

## Wrapper nodes handled (grammar inspection)

`tree-sitter-php-0.25.2/php/src/node-types.json` was inspected before any child
filter was written. The following wrappers are handled explicitly; every child
filter in the adapter starts from a named field or one of these unwraps:

| # | Wrapper | Where handled | Effect |
| --- | --- | --- | --- |
| 1 | `php_tag` | `visit_scope_children`, statement filter | skipped; not a statement |
| 2 | `text` (inline HTML) | `visit_scope_children`, `visit_inputs`, statement filter | skipped |
| 3 | `statement` (named hidden supertype) | `unwrap_statement`, `statement_expression` | unwrapped to its first named child |
| 4 | `expression` (named hidden supertype) | `unwrap_statement` | unwrapped to its first named child |
| 5 | `namespace_definition` | `visit_scope_children` | recurse through its `compound_statement` body field |
| 6 | `class_declaration` | `visit_class`, `class_shape` | members read from its `declaration_list` body field |
| 7 | `formal_parameters` | `extract_parameters` | children are `simple_parameter` / `variadic_parameter` / `property_promotion_parameter`; attributes ignored |
| 8 | `variable_name` | `bare_variable_name`, `extract_parameters`, `collect_callee_identifiers`, `visit_inputs` | unwrapped to the inner `name`; `$` / `&` stripped |
| 9 | `expression_statement` | `statement_expression` | unwrapped to the expression it holds |
| 10 | `parenthesized_expression` | `condition_text`, `enclosing_condition` | exactly one outer parenthesis pair removed |
| 11 | `arguments` | `collect_callee_identifiers`, `first_expression_text` | never mistaken for a callee; skipped when finding an expression |
| 12 | `property_declaration` → `property_element` | `class_shape` | property field name from `variable_name`, type from the declaration's `type` field |
| 13 | `const_declaration` → `const_element` | `class_shape`, `const_element_name`, `first_expression_text` | class constants; this grammar has **no** `class_constant` node kind, so `const_declaration` / `const_element` is the representation indexed into `ReturnShape`; `const_element`'s `name` and value are positional children, not named fields, so they are found by child kind |
| 14 | `type` supertype (`named_type`, `optional_type`, `union_type`, `intersection_type`, `primitive_type`, `bottom_type`) | `extract_parameters`, `extract_returns`, `class_shape` | read as exact source text; never descended into for identifiers |
| 15 | `primary_expression` / `literal` supertypes | `visit_inputs`, `classify_statement` | walked through named children; `print_intrinsic` classified as a Call step |
| 16 | `compound_statement` | `analyze`, `visit_scope_children` | its direct named children are the ordered steps |
| 17 | `sequence_expression` | `analyze` (`code` normalization) | `echo` with several operands stays one single-line step |
| 18 | `catch_clause` / `finally_clause` | `visit_throws` | throw sites inside them are still collected |
| 19 | `object_creation_expression` | `throw_code`, `first_expression_text` | thrown class name fallback and first string argument |
| 20 | `member_call_expression` / `nullsafe_member_call_expression` / `scoped_call_expression` | `callee_bare_name` | callee from the `name` field, never from `object` / `scope` |

Nested `function_definition`, `method_declaration`, `anonymous_function`,
`arrow_function` and `class_declaration` nodes are excluded from call recursion
so a nested body's calls are not attributed to the enclosing function.

## Tests

Sixteen `#[cfg(test)]` unit tests live in `src/php/mod.rs`: identity, free
function with parameters/defaults, variadic parameter, class method, nested
function exclusion, line/block doc comments, class type index, same-file and
cross-file return-field resolution, call/branch/return steps, pure transform
step, operator-expression inference, throw error with guard, side-effect
permissions, pure budget and deterministic repeat.

`infer_type` in `body.rs` and `infer_literal_type` in `discover.rs` share
`is_comparison_operator` from `mod.rs`. Both infer `int` for numeric arithmetic
(a float operand upgrades to `float`), `string` for `.` concatenation, `bool`
for comparisons and `!`, and the declared cast target for `cast_expression`;
`unknown` remains only where no declared or deducible type exists (for example
an untyped variable or a call result). This is display metadata in `Step.output`
and never changes the contract schema.

`discover::type_index` is the single helper that builds the class-shape map:
`discover()` passes it to `extract_returns` for same-file return fields and
`collect_types()` returns it for core's project-wide `resolve_return_fields`
call. Both paths share one map shape.

## Fixture

`tests/fixtures/php_app/` contains `src/Text.php` and `src/Metrics.php` with
five functions: three free functions, a helper call edge, a guarded
`EMPTY_TEXT` throw, an `echo`/`printf` side effect, and `Calculator.scale` plus
the `Calculator` type-index entry. `README.md` lists the expected output.

## External findings (not edited by J38)

J38 owns only the three `src/php/` files, the fixture directory and this doc.
The following edits are required before the adapter can be wired or built; they
are reported, not made:

1. `Cargo.toml` — add `tree-sitter-php = "0.25"` to `[workspace.dependencies]`,
   immediately after line 35 (`tree-sitter-typescript = "0.23"`) and before
   line 36 (`ureq = ...`). The cached crate is 0.25.2 and its build script
   compiles the grammar with `cc`, which the orchestrator gate must confirm.
2. `crates/fnscope-parser/Cargo.toml` — add `tree-sitter-php.workspace = true`
   to `[dependencies]`, immediately after line 16
   (`tree-sitter-typescript.workspace = true`).
3. `crates/fnscope-parser/src/lib.rs` — add `pub mod php;` after line 7
   (`pub mod golang;`) and before line 8 (`pub mod python;`). No `pub use` is
   required: callers reach `PhpAdapter` through `fnscope_parser::php::PhpAdapter`.
4. `crates/fnscope-parser/src/adapter.rs` — **no edit in J38**. A later
   registration job would add `use crate::php::PhpAdapter;`, a
   `registry.register(Box::new(PhpAdapter::new()));` line in
   `with_builtin_adapters`, and `("php", "php")` to the registry test. Doing
   that here would violate the "do not register globally" instruction.
5. No edit to `fnscope-core`, `fnscope-model`, `fnscope-enrich`, `ui/`,
   `docs/contract/rust-api.md` or `docs/drift.md` is needed. If a gate proves
   otherwise, stop and record it as architecture drift rather than adding a
   language branch.

## Evidence and limits

- Grammar constant was read from the cached crate's `bindings/rust/lib.rs`; the
  wrapper inventory was read from its `php/src/node-types.json` before code was
  written.
- Byte normalization was never performed with `sed`; all owned files were
  written as LF. Each owned text file was verified with
  `tr -cd '\r' < file | wc -c` returning `0`.
- `rustfmt --edition 2021 --check` is **format-only** evidence. No
  `cargo build`, `cargo test`, `cargo check` or `cargo fmt` was run by J38;
  compilation is the orchestrator's gate.
