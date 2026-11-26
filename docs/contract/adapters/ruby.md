# Adapter contract — Ruby (J37)

This page describes the Ruby adapter's syntax-to-contract mapping. It is a
documentation-only addition: `docs/contract/rust-api.md` is unchanged, and no
schema field, `DiscoveredFn`, `BodyAnalysis`, `Step`, `ErrorSpec` or
`ReturnShape` was renamed or reshaped.

The adapter lives in `crates/fnscope-parser/src/ruby/`. It exposes exactly
`pub struct RubyAdapter` with `pub fn new() -> Self` and `impl Default`, and
implements all six `LanguageAdapter` methods from `docs/contract/adapter-api.md`.
It is **not** registered globally by J37; registration is a separate job.

## Identity

| Method | Value | Note |
| --- | --- | --- |
| `id()` | `"ruby"` | stable diagnostic name |
| `extensions()` | `&["rb"]` | `Registry::for_extension` trims a leading dot, so this is bare `rb`, not `.rb` |
| `resolve_return_fields` | `crate::resolve::resolve_fields` | the shared grammar-free helper, not a duplicate |
| grammar | `tree_sitter_ruby::LANGUAGE` (cached crate 0.23.1) | converted with `.into()` to `tree_sitter::Language`, the same pattern as the Python adapter |

## Wrapper nodes handled

The wrapper list was derived by reading the cached
`tree-sitter-ruby-0.23.1/src/node-types.json` and `grammar.js` before any child
filter was written. Every wrapper this adapter handles is named below.

| Wrapper node | Where handled | How it is unwrapped |
| --- | --- | --- |
| `body_statement` | discovery (`class`/`module` body), type index, body analysis | `child_by_field_name("body")`; its **named children** are the top-level statements, so a nested `def` is never a second callable |
| `method_parameters` | discovery parameters | `child_by_field_name("parameters")`; named children are the formal parameters |
| `argument_list` | type index (`attr_*`, `Struct.new`), body calls | `child_by_field_name("arguments")`; named children are arguments. `command_argument_list` is aliased by the grammar to `argument_list` |
| `then` | body analysis error collection | reached by generic descendant walking; its statements are **not** separate top-level steps, but `raise` calls inside them are collected |
| `else` | body analysis error collection | same: walked for errors, not emitted as an independent step |
| `elsif` | body analysis error collection | a named branch node with `condition`/`consequence`/`alternative` fields; walked, not emitted separately |
| `begin` | body analysis classification | named node whose children are statements plus `rescue`/`else`/`ensure`; classified as one `Branch` step |
| `rescue` | body analysis classification and error `when` | named node with `exceptions`/`variable`/`body` fields; classified as one `Branch` step |
| `parenthesized_statements` | body analysis `statement_expression` | unwrapped to its first named child before deciding `Call` vs `Transform` vs `Return` |

`block` and `do_block` are named nodes with `body`/`parameters` fields; calls
carrying a block are still one `Call` step and their arguments are read through
`argument_list`. The abstract `_`-prefixed node types in `node-types.json`
(`_statement`, `_expression`, `_arg`, `_primary`, `_lhs`, `_variable`,
`_method_name`, `_call_operator`, `_body_statement`, etc.) are **field type
descriptors, not concrete tree nodes**, and are never matched directly.

## Discovery (`src/ruby/discover.rs`)

Uses `tree_sitter_ruby::LANGUAGE` through `tree_sitter::Parser`. Discovery walks
only module-level items and direct members of a class/module `body_statement`,
so a method nested in another body is never a second callable. Results are
sorted by `(line, qualified_name)` and de-duplicated by the same key.

| Ruby node | Contract result |
| --- | --- |
| module-level `method` | `FnKind::Declaration`, `name` from the `name` field, `qualified_name` = name |
| module-level `singleton_method` (`def self.x`) | same, bare name from the `name` field |
| `method` in a `class`/`module` body | `FnKind::Method`, `qualified_name` = `ClassName.method`, `class_name` = `ClassName` |
| `singleton_method` in a `class`/`module` body | same method shape |
| `attr_reader` / `attr_writer` / `attr_accessor` | ignored (they are `call` nodes, not functions) |
| nested `method` inside a method body | ignored |

`source_code` is the exact method slice, `line`/`end_line` are 1-based rows, and
`relative_path` is never used to invent a module path. `is_async` is always
false because Ruby has no `async def`.

### Parameters and returns

| Ruby syntax | Contract field |
| --- | --- |
| bare `identifier` in `method_parameters` | one required `Parameter`, type from YARD or `"unknown"` |
| `optional_parameter` (`a = 1`) | `default_value = Some(Value::String("1"))`, `required = false` |
| `keyword_parameter` (`a: 1`) | default text when present; `required = false` only when a default exists |
| `splat_parameter` (`*a`), `hash_splat_parameter` (`**a`), `block_parameter` (`&b`) | one `Parameter` each with fallback types `Array`, `Hash`, `Proc` |
| `forward_parameter` (`...`), `destructured_parameter`, `**nil` | skipped: no stable caller-supplied name |
| `# @param [Type] name Description` | `Parameter.type_` = `Type`, `Parameter.description` = `Description` |
| `# @return [Type]` | `ReturnShape.type_` = `Type`; empty declared text when absent |
| return fields | resolved through `crate::resolve::resolve_fields_from_map` only |

Contiguous leading `#` lines become `doc_description` with YARD tag lines
removed; the tags are parsed separately as shown above.

### Type index

- `class`/`module` declarations are indexed under their name. `attr_*` fields
  become `ReturnField`s; `# @!attribute [Type] name` supplies a field type,
  otherwise `"unknown"`.
- `Record = Struct.new(:id, :name)` and `Data.define(:a)` are indexed with
  `type_ = "Struct"`/`"Data"` and one field per symbol/string argument.
- Constant aliases such as `UserId = Integer` are indexed with the right-hand
  type text and no fields. Literal constants are skipped.

## Body analysis (`src/ruby/body.rs`)

The method node is found by exact `source_code` match with a line+name fallback;
a miss returns `BodyAnalysis::default()`. Only the body's direct statements are
steps; comments are excluded.

| Ruby node | `StepKind` | `code` / `inputs` / `output` |
| --- | --- | --- |
| assignment / operator assignment whose right side contains a `call` | `Call` | normalized one-line source; `calls_fn_id` set when a callee is in `known_names` |
| `if`, `unless`, `case`, `begin`, `rescue`, `else`, `ensure`, `elsif`, loops and modifiers | `Branch` | condition (or `value`/`pattern`/`exceptions`); `throws <CODE> | continues` when the branch contains a raise, else `<condition> | continues` |
| other statements containing a `call` | `Call` | raw callee names in step order |
| pure assignment and other expressions | `Transform` | inferred `name: type` output, else `void` |
| `return`, or a final expression with no call | `Return` | `ReturnShape.type_` from discovery, or `void` |

Titles and details come from the two contiguous `#` lines immediately above a
statement, with deterministic fallbacks (`Call name()`, `Guard: …`,
`Transform name`, `Return <type>`). `code` is always collapsed to one line;
`callee_names` are raw callee identifiers in step order.

### Errors

In Ruby `raise` and `fail` are `call` nodes rather than a dedicated statement.
A call named `raise`/`fail` yields an `ErrorSpec` whose code is the first string
in its arguments — including the string inside `Error.new("CODE")`. `when` is
the nearest enclosing branch condition (or `explicit raise`). Sites are
deduplicated by code in source order.

| Code | Status |
| --- | --- |
| `TOKEN_NOT_FOUND`, `VALIDATION_ERROR`, `INVALID_INPUT` | 422 |
| `EMPTY_TEXT`, `EMPTY_SENTENCE`, `INSECURE_PROTOCOL`, `BAD_REQUEST` | 400 |
| `NOT_FOUND` | 404 |
| `UNAUTHORIZED` | 401 |
| `FORBIDDEN` | 403 |
| anything else | 500 |

### Purity, permissions and budgets

Derived from the whole function slice. Console markers are matched as
standalone identifiers so `map(` is not mistaken for `p(`.

| Marker | Effect |
| --- | --- |
| `puts`, `print`, `p`, `pp`, `warn`, `STDOUT`, `$stdout`, `STDERR`, `$stderr` | `Purity::SideEffect`, `console:out` |
| `File.`, `IO.`, `Dir.`, `FileUtils.` | `fs:read`; `fs:write` when a write marker is also present |
| `Net::HTTP`, `URI.open`, `Socket` | `net:outbound` |
| `ENV["KEY"]`, `ENV.fetch("KEY")` | `secrets:read:<KEY>` |

Budgets and runtime labels are the contract values already used by the Python
adapter: `150 ms` / `universal` for pure code, `1000 ms` / `node20` for side
effects, `5000 ms` / `node20` for network effects. Ruby code is not actually
executed by the Node runner; these labels stay stable so the schema and
enricher need no language branch.

## Fixture

`tests/fixtures/ruby_app/` is a single coherent file with five functions, a
module-level singleton method, a class instance method, a call edge, a guarded
`EMPTY_TEXT` raise, a `puts()` side effect and a `Struct` type entry.
`README.md` lists the expected output.

## Out-of-scope edits this job cannot make

J37 owns only `src/ruby/`, `tests/fixtures/ruby_app/` and this document. Before
the adapter compiles or is reachable, the orchestrator must:

1. Add `pub mod ruby;` to `crates/fnscope-parser/src/lib.rs`.
2. Add `tree-sitter-ruby` to `[workspace.dependencies]` in the root
   `Cargo.toml` and to `[dependencies]` in `crates/fnscope-parser/Cargo.toml`.
3. Register `RubyAdapter` in `Registry::with_builtin_adapters` and extend the
   built-in extension test if Ruby should become a built-in. J37 deliberately
   does **not** register it globally.
