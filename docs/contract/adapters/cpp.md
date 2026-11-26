# Adapter contract — C++

This document records the C++ adapter's implementation choices. It does not
change `docs/contract/adapter-api.md` or the locked JSON schema in
`docs/contract/rust-api.md`. The adapter is deliberately **not registered
globally**: wiring it into `Registry::with_builtin_adapters` is a separate job.

## Identity and extensions

- `id()` returns `"cpp"`.
- `extensions()` returns `&["cpp", "cc"]` — bare lower-case suffixes, exactly the
  form `adapter.rs` stores and `Registry::for_extension` compares (it trims a
  leading dot and ignores case). `.cxx`, `.c++`, `.hpp` and `.hh` are intentionally
  **not** claimed: this job owns only the two suffixes in the brief, and a wider
  table belongs to a future registration decision. The two selected suffixes
  cover the most common C++ translation-unit spellings without overlapping any
  existing adapter.

`CppAdapter` is stateless, exposes `new()` and `Default`, and implements all six
`LanguageAdapter` methods. Discovery, body analysis and the type index live in
`discover.rs` and `body.rs`; return-field resolution delegates to the shared
`crate::resolve::resolve_fields`.

## Grammar facts verified before writing child filters

The child filters were written after reading
`~/.cargo/registry/src/*/tree-sitter-cpp-0.23.4/src/node-types.json` and were
cross-checked against a real parse of representative C++ source (free function,
class method, constructor, typedef, using alias, default parameter, trailing
return, throw, stream write and effect calls). Relevant findings:

- `tree_sitter_cpp::LANGUAGE` is a `LanguageFn` in
  `bindings/rust/lib.rs:35`; `let language: tree_sitter::Language =
  tree_sitter_cpp::LANGUAGE.into();` is the same construction the Go/Rust
  adapters use.
- `method_definition` does **not** exist in this grammar. A member with a body
  is a `function_definition` inside a class/struct body; a bodyless method is a
  `field_declaration`; a constructor declaration is a `declaration`. All three
  wrap a `function_declarator` and are recorded as `ClassName.method` with
  `FnKind::Method`.
- `throw_expression` does **not** exist. The node is `throw_statement`, and the
  error code is the first `string_literal` argument of the `call_expression`
  below it (`throw X("CODE")`, `throw std::runtime_error("CODE")`).
- `statement_list` does **not** exist in 0.23.4: a `compound_statement` holds its
  statements directly. `body.rs` still checks for a `statement_list` child
  before falling back to the compound statement, so a future grammar that
  reintroduces the wrapper is handled.

## Wrappers handled explicitly

| Wrapper node | Where | Handling |
|---|---|---|
| `translation_unit` | root | Only its direct named children are considered callables. |
| `field_declaration_list` | `class_specifier`/`struct_specifier` `body`, anonymous `type_definition` struct | Members are read from this wrapper; methods are separated from data fields here. |
| `statement_list` | defensive, absent in 0.23.4 | `body.rs` checks for it, then falls back to `compound_statement` children. |
| `parameter_list` | `function_declarator` `parameters` | Named children `parameter_declaration` / `optional_parameter_declaration` become parameters. |
| `argument_list` | `call_expression` `arguments`, `init_declarator` `value` | First string literal yields an error code; a constructor-style initializer becomes a Call. |
| `condition_clause` | `if_statement` / `switch_statement` `condition` | Its `value` field supplies branch conditions and throw `when`. |
| `type_descriptor` | `alias_declaration` `type`, `trailing_return_type` child | Read as an alias target or trailing return type. |
| `trailing_return_type` | `function_declarator` child | Supplies the return type when the declared type is `auto` or absent. |
| `qualified_identifier` | scopes and call targets | `scope` + last `name` give `Calc.method`; the last identifier gives a raw callee name. |
| `init_declarator` | declarations with initializers | `value` distinguishes a call initializer (Call) from pure arithmetic (Transform) and gives the output name. |
| `function_declarator` | definitions and member declarations | Carries `declarator`, `parameters`, optional `trailing_return_type`; its inner declarator names the function. |
| `throw_statement` | body descendants | Wraps the call that carries the thrown code. |

The abstract supertypes `_declarator`, `_field_declarator` and `_type_declarator`
appear in `node-types.json` but are **not** emitted by the parser. The concrete
kinds reached through them are handled explicitly: `identifier`,
`field_identifier`, `type_identifier`, `namespace_identifier`,
`pointer_declarator`, `reference_declarator`, `parenthesized_declarator`,
`attributed_declarator`, `array_declarator`, `template_function`,
`operator_name` and `destructor_name`. `placeholder_type_specifier` (`auto`) is
handled for trailing returns.

## Extraction rules

- **Discovery:** direct `function_definition` children of the root become free
  functions unless their declarator is scoped (`Calc::method`), in which case
  they become methods. Members are visited only inside `field_declaration_list`;
  no pass descends into a `compound_statement`, so nested helpers are excluded.
- **Parameters:** `parameter_declaration` is required;
  `optional_parameter_declaration` supplies a JSON `defaultValue` and is not
  required. Contiguous leading `//` comments become `doc_description` and
  provide `@param` descriptions and `@tags`.
- **Types:** `class_specifier`/`struct_specifier` data fields and `type_definition`
  / `alias_declaration` aliases form a `BTreeMap<String, ReturnShape>`;
  `resolve_return_fields` reuses the shared resolver.
- **Steps:** top-level `compound_statement` statements in source order. `if`,
  `switch` and `try` are `Branch`; `return` is `Return`; statements containing a
  `call_expression`, a `std::cout`/`std::cerr` stream write or a
  constructor-style initializer are `Call`; everything else is `Transform`.
- **Errors:** thrown string literals become `ErrorSpec` with a stable status
  map and the nearest enclosing `if` condition as `when`; duplicates are
  removed by code, keeping the first.
- **Effects:** `std::cout`/`std::cerr`/`printf` → `console:out`;
  `std::fstream`/`fopen` → `fs:read` (plus `fs:write` for write modes);
  `curl_*`/`socket` → `net:outbound`; `std::getenv("KEY")` →
  `secrets:read:<KEY>`. Any marker makes the function `SideEffect`; pure
  functions keep timeout `150` / runtime `universal`, side effects use `1000` /
  `node20`, and network effects use `5000`.

## Tests and fixture

Unit tests beside the code cover free function, class method/constructor,
parameters/defaults/trailing return, class and alias type index, ordered
call/branch/stream/return steps, throw errors, console/fs/net/secret effects,
deterministic repeat and the adapter identity/resolution seam. The fixture at
`tests/fixtures/cpp_app/` has seven callables and documents expected output in
its README. No cargo command or gate was run by this job.
