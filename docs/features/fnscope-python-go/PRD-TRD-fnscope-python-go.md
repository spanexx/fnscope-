# fnscope-python-go — PRD-TRD (reconstructed)

RECONSTRUCTED 2026-11-26. This document was written after the Python/Go batch
shipped (commit `01062af`). It is a reconstruction from the locked spec, the
ledger, the reviews and the shipped files — not a pre-impl artifact. No date
is backdated and no quote is invented. Status: SHIPPED.

## Why

The original goal scans TS/JS into a schema-valid function graph manifest.
Iteration 15 added a Rust backend so `fnscope scan crates/` worked; iteration
16 expanded the goal to a polyglot tool whose outcome is:

> "fnscope scans Rust, TS/JS, Python, Go and the standard language set with
> accurate function graphs and execution steps; adding a language requires
> ZERO changes to core graph builder, schema, enricher or UI"
>
> — `.reports/goal-build-fnscope.md:181`

Python and Go are the first languages added after that expansion. The adapter
registry from J21 supplies the seam, so each language is a plugin and the core
graph builder stays untouched (iteration 24, `:246-252`; iteration 30,
`:294-300`). They ship together as one logical change because both needed the
same registry/glob edits and one gate.

## Behavioral spec

### Adapter identity

| Fact | Python | Go |
| --- | --- | --- |
| Module | `crates/fnscope-parser/src/python/` | `crates/fnscope-parser/src/golang/` |
| Public type | `PythonAdapter` (`new()` + `Default`) | `GoAdapter` (`new()` + `Default`) |
| `id()` | `"python"` | `"go"` |
| `extensions()` | `&["py"]` | `&["go"]` |
| Grammar | `tree_sitter_python::LANGUAGE` 0.25 | `tree_sitter_go::LANGUAGE` 0.25 |

Both implement all six `LanguageAdapter` methods with the exact signatures in
`docs/contract/adapter-api.md:13-20`, hold no global state, and delegate
`resolve_return_fields` to the shared grammar-free helper in `crate::resolve`
(`python/mod.rs:53-63`, `golang/mod.rs:55-65`). Suffixes are bare: the registry
trims a caller's leading dot and lowercases before lookup
(`docs/contract/adapters/python.md:18`, `docs/contract/adapters/go.md:22`).
The Go module is `golang`, not `go`, because `go` is a Rust keyword
(`golang/mod.rs:3-6`); neither adapter claims a `.h` suffix
(`docs/contract/adapters/c.md:23-27`).

### Discovery

- Python: module-level `function_definition` and `decorated_definition`
  wrappers become declarations; direct class-body functions become methods
  with `qualified_name = ClassName.method` and `class_name = ClassName`;
  nested functions are ignored. Parameters, defaults, `->` annotations,
  docstrings and leading `#` comments map to the unchanged model types.
  `collect_types` indexes module-level aliases and skips literal constants
  (`docs/contract/adapters/python.md:21-66`).
- Go: `function_declaration` and `method_declaration` (receiver qualified to
  a bare type name) become declarations/methods; closures (`func_literal`)
  are ignored. Grouped parameters share a declared type, Go has no optional
  parameters. `collect_types` indexes structs, interfaces and aliases into
  the type index (`docs/contract/adapters/go.md:50-82`).
- Results are sorted by `(line, qualified_name)` and deduplicated by that
  pair. Nothing is appended to the schema: `DiscoveredFn`, `BodyAnalysis`,
  `Step`, `ErrorSpec` and `ReturnShape` keep their locked shapes
  (`docs/contract/rust-api.md`, `docs/contract/adapters/{python,go}.md:1-6`).

### Body analysis

- Top-level statements in source order get ids `step-1`, `step-2`, ... and one
  of four kinds: `call`, `branch`, `transform`, `return` (spec step 3,
  `fnscope_architecture_agent_build_spec.md:75-91`).
- `code` is one line; `inputs` are referenced identifiers in first-seen
  order; `title`/`detail` come from adjacent comments or deterministic
  fallbacks. `callee_names` carries raw callee names; a `Call` step also
  carries `calls_fn_id` when the callee is known, and core alone resolves it
  into a bidirectional `calledBy` edge.
- Errors: `raise`/`panic`/`errors.New` carry a code, status and `when`; sites
  dedupe by code. Purity, permissions and budgets use the existing labels
  (`150 ms`/`universal` pure, `1000 ms`/`node20` side effect, `5000 ms`/
  `node20` network). Those runtime strings are labels, not execution
  (`docs/contract/adapters/{python,go}.md`).
- **Contract decision:** a declaration whose initializer calls a function is
  a `call` step, not `transform` — the locked spec says `transform` excludes
  external calls and its schema example labels
  `const tokens = tokenize(input.sentence)` as `"kind": "call"` (spec
  `:81`, `:151-161`). Triage decided the Python case TEST-only and the Go
  case BOTH (`.reports/reviews/lang-batch-triage.md:70-78`, `:96-102`); D-5
  closed the Go doc/code drift (`docs/drift.md:51-55`).

### Grammar wrappers (root cause of the nine failures)

The 0.25 grammars insert wrappers; the adapters must look through them:

| Wrapper | Where | Fix |
| --- | --- | --- |
| `expression_statement` | Python module aliases | `python/discover.rs:280-281` unwraps to the inner `assignment`; `python/body.rs:186-189` has the shared statement helper |
| `statement_list` | Go function block | `golang/body.rs:90-95` uses the wrapper's children, with the original node as fallback |
| `field_declaration_list` | Go `struct_type` | `golang/discover.rs:289-294` iterates the wrapper's `field_declaration` children |

Evidence: ledger iterations 28-29 (`:278-292`) and triage
(`.reports/reviews/lang-batch-triage.md:21-26`, `:43`, `:51`, `:93`).

### Fixtures

- `tests/fixtures/python_app/` — six expected functions (`normalize_text`,
  `classify`, `report`, `add`, `Calculator.__init__`, `Calculator.scale`), one
  call edge (`Calculator.scale` -> `add`), a guarded `EMPTY_TEXT` raise, a
  `print()` side effect, module aliases, and a documented `__pycache__`
  decoy that default excludes must drop (`README.md:18-62`).
- `tests/fixtures/go_app/` — six callables (`normalize`, `analyze`, `load`,
  `Service.Apply`, `report`, `main`), struct `Service` with field `Prefix`,
  guard errors, and the `main` -> `report` edge (`README.md:12-51`).

## Simulation contract

- `simulate.sh` is the canonical **post-impl** simulation. It is pure shell:
  no `cargo`, no server, no Python or Go toolchain. It resolves the repo root
  from its own path (no hardcoded drive or user directory), scans the two
  fixtures, asserts that both languages are present with the expected
  functions and call edges, asserts every edge endpoint is a known function
  (no dangling), asserts the `__pycache__` decoy is excluded, and checks the
  structural registration facts (`pub mod python/golang`, grammar deps,
  default `**/*.py`/`**/*.go` globs). It prints one `[PASS]`/`[FAIL]` line per
  assertion and a final `PASS` or `FAIL`, exiting 0 only on PASS.
- `archive/simulate-pre.reconstructed.sh` is a **labelled design-time
  placeholder**. No pre-impl simulation artifact exists in the evidence, so
  it asserts nothing and prints a placeholder marker. It is not a verdict and
  must not be quoted as one.
- All five deliverables are LF-only: verified with
  `tr -cd '\r' < file | wc -c` equal to 0, using the single-quoted WSL form
  from a script file — never `sed` (`docs/ethics/lessons.md:238-243`).

## Technical design

- One adapter crate area per language (`python/`, `golang/`) with `mod.rs`,
  `discover.rs`, `body.rs`; grammar knowledge stays inside it. The shared
  resolver stays grammar-free, so no core, schema, enricher or UI file
  changes for a new language (iteration 16 invariant).
- Cargo: `tree-sitter-python = "0.25"` and `tree-sitter-go = "0.25"` in
  workspace dependencies, `.workspace = true` entries in the parser crate
  (J23-J24 review findings F6, `.reports/reviews/J23-J24.md:56`).
- Registration is one separate job after review: `pub mod golang;` /
  `pub mod python;` in `lib.rs`, two `register(...)` calls in
  `with_builtin_adapters`, id-table rows, `**/*.py`/`**/*.go` defaults in
  `crates/fnscope-core/src/scan.rs`, and the `__pycache__` default exclude
  (`.reports/reviews/J23-J24.md:64-74`, ledger iteration 26).
- Tests live beside the code: Python 10 tests, Go 16 tests as reported in
  iteration 25 and matched by the reviewer (`:254-260`,
  `.reports/reviews/J23-J24.md:28`, `:43`). The real gates are the
  orchestrator's, recorded in iterations 27-30.
- Delivery: `01062af` (29 files); gate evidence in `IMPL-fnscope-python-go.md`.

## Non-goals

- **No Python or Go execution via `invoke`.** The runtime labels
  (`node20`/`universal`) are schema labels only; Python/Go code is never run
  by the Node runner (`python.md:118-122`, `go.md:114-118`).
- No new schema field, no rename of locked contract names, no change to the
  core graph builder, enricher or UI for either language.
- No registration edit inside an adapter worktree; no `.h` suffix; no
  C-style header indexing.
- No claim that a syntax check (`python3 -m py_compile`, `go vet`) or a
  format check is a Rust compile verdict (L11,
  `docs/ethics/lessons.md:226-252`).
- No backdating: this reconstruction is dated 2026-11-26 and cites the
  ledger iterations as historical evidence.

## Sources

`.reports/goal-build-fnscope.md` iterations 15-16, 21-30;
`.reports/reviews/J23-J24.md`; `.reports/reviews/lang-batch-triage.md`;
`docs/drift.md` D-5; `docs/ethics/lessons.md` L1/L10/L11;
`fnscope_architecture_agent_build_spec.md` sections 3-4 and step 3 example;
`docs/contract/{rust-api,adapter-api}.md`;
`docs/contract/adapters/{python,go}.md`;
`crates/fnscope-parser/src/{python,golang}/`;
`tests/fixtures/{python_app,go_app}/README.md`.
