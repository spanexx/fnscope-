# Contract — Kotlin adapter (J35)

This document records the shape of the Kotlin language adapter implemented in
`crates/fnscope-parser/src/kotlin/`. It implements the pluggable seam in
`docs/contract/adapter-api.md` without changing any contract type or JSON field.
The adapter is **not registered globally**: a caller creates a private
`Registry`, calls `register(Box::new(KotlinAdapter::new()))` and injects it into
`fnscope_core::scan::scan_with_registry`.

## 1. Public API

```rust
pub struct KotlinAdapter;                 // src/kotlin/mod.rs
impl KotlinAdapter { pub fn new() -> Self; }
impl Default for KotlinAdapter;           // delegates to new()
impl LanguageAdapter for KotlinAdapter { /* all six methods */ }
```

`id()` returns `"kotlin"`. `extensions()` returns `&["kt"]`, the bare-suffix
form used by `Registry::for_extension`. `.kts` is intentionally not claimed:
script files execute top-level code rather than declaring callable units, and
keeping one suffix avoids counting a generated script beside its source as two
discoveries of the same callable. Adding `kts` later needs only a one-line edit
here plus a check that no script companion is scanned twice.

The six trait methods delegate to `src/kotlin/discover.rs` and
`src/kotlin/body.rs`. `resolve_return_fields` calls the shared
`crate::resolve::resolve_fields`, so no new resolver is introduced.

## 2. Grammar selection — why `tree-sitter-kotlin-ng`

The original `tree-sitter-kotlin 0.3.8` cannot be used in this workspace. Its
`Cargo.toml:35` pins `tree-sitter >= 0.21, < 0.23`, and tree-sitter declares
`links = "tree-sitter"`, so Cargo permits only one version: dependency
resolution fails against the workspace's `tree-sitter = "0.25"` (`Cargo.toml:30`).
It also exports no `LANGUAGE` constant — only `language()` at
`bindings/rust/lib.rs:27`. The earlier raw-pointer `mem::transmute` bridge
addressed the Rust type mismatch but not the native `links` conflict, so it was
removed. **No unsafe code remains in the adapter.**

The replacement is `tree-sitter-kotlin-ng = "1.1"`, verified in the cached source
at `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tree-sitter-kotlin-ng-1.1.0`:

- `bindings/rust/lib.rs:34` — `pub const LANGUAGE: LanguageFn`
- `Cargo.toml:49-50` — depends only on `tree-sitter-language = "0.1"`
- `Cargo.toml:52-53` — `tree-sitter 0.24` is **dev-only**, so it is not linked
  into dependents and cannot conflict with the workspace's 0.25 runtime

`kotlin_language()` in `src/kotlin/mod.rs` is now simply
`tree_sitter_kotlin_ng::LANGUAGE.into()`. The `///` on that function records the
single-native-runtime reason.

## 3. Grammar facts that shape the implementation

Inspection of the `-ng` crate's `src/node-types.json` and `grammar.js` found the
following differences from the old 0.3.8 grammar. Every changed kind is listed
here and handled in `discover.rs` / `body.rs`:

| Old kind (0.3.8) | `-ng` kind (1.1) | Adapter change |
|---|---|---|
| `simple_identifier` | `identifier` | name/parameter/callee/input reads use `identifier`; declaration names use the new `name` field |
| `type_identifier` | `identifier` + `name` field | class, object, companion and alias names use `child_by_field_name("name")` (`type` field on `type_alias`) |
| `not_nullable_type` | `non_nullable_type` | type-kind filters renamed |
| (no wrapper) | `type` supertype | declared types are read from the `type` node; added to `TYPE_KINDS` |
| `multiline_comment` | `block_comment` | doc-comment and statement filters renamed |
| `jump_expression` | `return_expression` / `throw_expression` | distinct kinds for `return` (step kind) and `throw` (error site) |
| `control_structure_body` | (removed) | `if_expression` now has a `condition` field, read directly |
| `call_suffix` | (removed) | a `call_expression` holds `expression`, `type_arguments`, `value_arguments` / `annotated_lambda` directly; callee is the last identifier of the first child |
| `statements` | `statement` + `block` | body pass unwraps `block` children; a `statement` wraps `declaration` / `expression` |
| (no wrapper) | `declaration`, `class_member_declaration` | supertype wrappers unwrapped in discovery and type index |
| (no wrapper) | `expression`, `primary_expression`, `index_expression`, `annotated_expression` | expression supertypes unwrapped before step classification |
| (no wrapper) | `class_parameters` | primary-constructor parameters and fields are read from this wrapper |
| (no named fields) | named `name` / `condition` / `type` fields | field access used where supplied; kind-based fallback retained |

Other facts that still hold: callables are `function_declaration` (there is no
`constructor_declaration`); constructors are `primary_constructor` and
`secondary_constructor`; a default value remains a sibling of `parameter` inside
`function_value_parameters` and is read from the byte gap so anonymous tokens
such as `null` still work; `require(...)` / `check(...)` messages may live in a
trailing lambda, so the error scanner searches the whole call subtree for the
first string literal.

### Wrappers handled explicitly

`source_file` → `statement` → `declaration` → callable; `class_body` →
`class_member_declaration` → `declaration` → member; `function_body` → `block` →
`statement` → expression; `function_value_parameters` → `parameter` plus sibling
default; `primary_constructor` → `class_parameters` → `class_parameter`;
`companion_object` → `class_body` → members recorded with the *enclosing* class
name; `if_expression` → `condition` field; `when_expression` → `when_subject` /
`when_entry`; `try_expression` → `block` / `catch_block` / `finally_block`;
`call_expression` → `expression` / `value_arguments` / `annotated_lambda`;
`navigation_expression` → `obj.method()` resolves to bare `method`;
`property_declaration` → `variable_declaration`; `constructor_invocation` →
`throw X("CODE")` string literal; `type_alias` → `type` field for the alias name
and a `type` child for the target.

`function_declaration` nodes found inside a body are still skipped by
construction: discovery walks only `source_file`, `class_body`,
`companion_object` and nested `object_declaration` bodies, never
`function_body` or `block`.

## 4. Body, error and effect mapping

- Steps follow spec §4 step 3: a declaration/assignment with a call initializer
  is `call`; `if`/`when`/`try` and `require`/`check` guards are `branch`; pure
  expressions and `throw` are `transform`; `return` is `return`. `id` is
  `step-{index}`, `code` is a one-line normalized statement, titles/details come
  from the closest `//` comments or deterministic fallbacks.
- `throw X("CODE")` and `require`/`check("CODE")` produce `ErrorSpec` with a
  de-duplicated code, an enclosing-condition `when` and a status hint
  (422 for validation, 400 for empty/bad input, 404/401/403 for named codes,
  500 otherwise).
- `println`/`print` → `console:out`; `java.io`/`File`/`Files` → `fs`;
  `URLConnection`/`HttpClient` → `net:outbound`; `System.getenv("KEY")` →
  `secrets:read:KEY`. Any permission makes purity `side-effect`. Budgets and
  runtime labels match the existing adapters: pure → `150` / `universal`,
  console or fs → `1000` / `node20`, network → `5000` / `node20`.

## 5. Tests and fixture

Unit tests live beside the code (`#[cfg(test)]` modules) and cover: adapter
identity and cross-file return resolution; free function, class/companion
methods, defaults and `suspend`, type index, nested-function exclusion,
secondary constructor; ordered step kinds, call-initializer classification,
throw/guard errors, side effects, pure defaults and deterministic repeat. The
data-only fixture is `tests/fixtures/kotlin_app/` with `src/Main.kt` and a
`README.md` describing expected output.

## 6. External findings for the orchestrator

This job owns only `src/kotlin/`, `tests/fixtures/kotlin_app/` and this document.
The following edits are **not** made here:

1. `Cargo.toml:30-35` — add `tree-sitter-kotlin-ng = "1.1"` to
   `[workspace.dependencies]` and `tree-sitter-kotlin-ng.workspace = true` to
   `crates/fnscope-parser/Cargo.toml:8-17`. **Do not add**
   `tree-sitter-kotlin 0.3.8`: its native `links = "tree-sitter"` conflicts with
   the workspace's 0.25 runtime and breaks dependency resolution.
2. `crates/fnscope-parser/src/lib.rs:4-11` — add `pub mod kotlin;` so the adapter
   can be imported. No change to `adapter.rs` is needed: callers register it via
   `Registry::register`.
3. The old `mem::transmute` bridge has been deleted; the adapter contains no
   `unsafe` code and needs no further dependency-policy decision.
