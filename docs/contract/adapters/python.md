# Adapter contract — Python (J23)

This page describes the Python adapter's syntax-to-contract mapping. It is a
documentation-only addition: `docs/contract/rust-api.md` is unchanged, and no
schema field, `DiscoveredFn`, `BodyAnalysis`, `Step`, `ErrorSpec` or
`ReturnShape` was renamed or reshaped.

The adapter lives in `crates/fnscope-parser/src/python/`. It exposes exactly
`pub struct PythonAdapter` with `pub fn new() -> Self` and `impl Default`, and
implements all six `LanguageAdapter` methods from `docs/contract/adapter-api.md`.
It is **not** registered globally by J23; registration is a separate job.

## Identity

| Method | Value | Note |
| --- | --- | --- |
| `id()` | `"python"` | stable diagnostic name |
| `extensions()` | `&["py"]` | `Registry::for_extension` trims a leading dot, so this is bare `py`, not `.py` |
| `resolve_return_fields` | `crate::resolve::resolve_fields` | the shared grammar-free helper, not a duplicate |

## Discovery (`src/python/discover.rs`)

Uses `tree_sitter_python::LANGUAGE` through `tree_sitter::Parser`. Discovery
walks only module-level items and direct class-body members, so a function
nested in another body is never a second callable. Results are sorted by
`(line, qualified_name)` and de-duplicated by the same key.

| Python node | Contract result |
| --- | --- |
| module-level `function_definition` | `FnKind::Declaration`, `name` from the `name` field, `qualified_name` = name |
| module-level `decorated_definition` wrapping a function | same, with the wrapped function's slice and line |
| `class_definition` body member `function_definition` | `FnKind::Method`, `qualified_name` = `ClassName.method`, `class_name` = `ClassName` |
| `decorated_definition` in a class body wrapping a function | same method shape |
| `async` keyword child of `function_definition` | `is_async = true` |
| nested `function_definition` inside a body | ignored |

`source_code` is the exact `function_definition` slice (decorators excluded),
`line`/`end_line` are 1-based rows of that node, and `relative_path` is never
used to invent a module path.

### Parameters and returns

| Python syntax | Contract field |
| --- | --- |
| `parameter`, `default_parameter`, `typed_parameter`, `typed_default_parameter`, bare `identifier` in `parameters` | one `Parameter` |
| `name` field | `Parameter.name` |
| `type` field (annotation after `:`) | `Parameter.type_`, exact source text; `"unknown"` when absent |
| `value` field (default after `=`) | `Parameter.default_value = Some(Value::String(expr))`, `required = false` |
| no default | `required = true`, `default_value = None` |
| receiver `self` / `cls` | skipped, matching the Rust adapter's treatment of `self` |
| `->` annotation field `return_type` | `ReturnShape.type_`, exact source text; empty declared text when absent, reported as `void` by the `Return` step |
| return fields | resolved through `crate::resolve::resolve_fields_from_map` only |

`Parameter.description` is empty: J23 does not parse `:param`/`@param` doc lines
into parameter descriptions. This is additive (the field defaults empty) and
does not change the schema.

### Descriptions and type index

- `doc_description` prefers the first body `expression_statement` holding a
  `string`/`concatenated_string` (a docstring), then falls back to contiguous
  leading `#` comment lines. Both are trimmed and whitespace-normalized.
- `collect_types` indexes simple module-level aliases (`UserId = int`,
  `Alias = Dict[str, int]`) keyed by alias name, with `ReturnShape.type_` set to
  the aliased expression and empty `fields`. Literal right-hand sides such as
  `MAX = 10` are not aliases and are skipped.

## Body analysis (`src/python/body.rs`)

The function node is found by exact `source_code` match with a line+name fallback;
a miss returns `BodyAnalysis::default()`. Only the body's direct statements are
steps; comments and the leading docstring are excluded.

| Python node | `StepKind` | `code` / `inputs` / `output` |
| --- | --- | --- |
| `expression_statement` containing a `call` node | `Call` | single-line normalized source; identifier inputs; `calls_fn_id` set when a callee is in `known_names` |
| `if_statement`, `match_statement` | `Branch` | `condition` (or `match` `subject`); `throws <CODE> | continues` when the branch contains a raise, else `<condition> | continues` |
| `assignment`, `augmented_assignment` | `Transform` | normalized source; right-hand identifiers as inputs; annotation or inferred `name: type` output, else `void` |
| `return_statement` | `Return` | `ReturnShape.type_` from discovery, or `void` |
| `raise_statement`, `named_expression`, other statements | `Transform` | deterministic fallback title/detail |

`code` is always collapsed to one line. `inputs` are referenced identifiers in
first-seen source order; string literals, type nodes and assignment targets are
not inputs. `callee_names` are raw callee identifiers in step order, including
calls nested in assignments; core resolves them for edges.

Titles and details come from the two contiguous `#` lines immediately above a
statement, with deterministic fallbacks (`Call name()`, `Guard: …`,
`Transform name`, `Return <type>`).

### Errors

`raise ValueError("CODE")` and `raise AppError("CODE", ...)` yield an
`ErrorSpec` with `code = CODE`. `when` is the nearest enclosing `if` condition,
or `explicit raise`. Sites are deduplicated by code in source order. Status
hints mirror the existing adapters, with `EMPTY_TEXT` added as `400`:

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
| `print(`, `logging`, `logger.` | `Purity::SideEffect`, `console:out` |
| `open(`, `pathlib`, `os.`, `shutil` | `fs:read`; `fs:write` when a write marker is also present |
| `requests`, `urllib`, `socket`, `http.client` | `net:outbound` |
| `os.environ[...]`, `os.getenv(...)`, `environ.get(...)` | `secrets:read:<KEY>` |

Budgets and runtime labels are the contract values already used by the Rust and
TypeScript adapters: `150 ms` / `universal` for pure code, `1000 ms` /
`node20` for side effects, `5000 ms` / `node20` for network effects. Python code
is not actually executed by the Node runner; these labels stay stable so the
schema and enricher need no language branch.

## Fixture

`tests/fixtures/python_app/` is a coherent package with a module-level alias,
free functions, a class method, a call edge, a guarded `EMPTY_TEXT` raise and a
`print()` side effect. `README.md` lists the expected functions. A decoy under
`__pycache__/` documents the exclusion expectation; glob-based exclusion is
core's job and is not implemented in this adapter.
