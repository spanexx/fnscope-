# fnscope-final-languages — PRD-TRD (reconstructed)

RECONSTRUCTED 2026-11-26. This document was written after the final five
adapters shipped (code commit `d6b02cf`, ledger iteration 42). It is a
reconstruction from the locked spec, the ledger, the review, the drift entry
and the shipped files — not a pre-impl artifact. No date is backdated and no
quote is invented. Status: SHIPPED.

## Why

The polyglot goal in iteration 16 is:

> "fnscope scans Rust, TS/JS, Python, Go and the standard language set with
> accurate function graphs and execution steps; adding a language requires
> ZERO changes to core graph builder, schema, enricher or UI"
>
> — `.reports/goal-build-fnscope.md:181`

Iterations 30-34 delivered Java, C and C++ and brought the built-in registry
to ten adapters. Iterations 35-42 delivered the remaining five named
languages — Kotlin, C#, Ruby, PHP and Swift — in one batch. Each is a plugin
behind `LanguageAdapter` (`docs/contract/adapter-api.md`); the core graph
builder, schema, enricher, server and UI are unchanged. Iteration 42 records
the proof: `ADAPTER_GATE_STATUS=0`, 233 tests, and a 13-language mixed scan
with 0 dangling edges.

## Behavioral spec

### Adapter identity

| Fact | Kotlin | C# | Ruby | PHP | Swift |
| --- | --- | --- | --- | --- | --- |
| Module | `src/kotlin/` | `src/csharp/` | `src/ruby/` | `src/php/` | `src/swift/` |
| Public type | `KotlinAdapter` | `CSharpAdapter` | `RubyAdapter` | `PhpAdapter` | `SwiftAdapter` |
| `id()` | `kotlin` | `csharp` | `ruby` | `php` | `swift` |
| `extensions()` | `&["kt"]` | `&["cs"]` | `&["rb"]` | `&["php"]` | `&["swift"]` |
| Grammar | `tree-sitter-kotlin-ng` 1.1, `LANGUAGE` | `tree-sitter-c-sharp` 0.24, `LANGUAGE` | `tree-sitter-ruby` 0.23.1, `LANGUAGE` | `tree-sitter-php` 0.25, `LANGUAGE_PHP` | `tree-sitter-swift` 0.7.4, `LANGUAGE` |

Each type exposes `new()` and `Default`, implements all six `LanguageAdapter`
methods with the exact signatures in `docs/contract/adapter-api.md:13-20`,
holds no global state, and delegates `resolve_return_fields` to the shared
grammar-free helper in `crate::resolve`. Suffixes are bare: the registry trims
a leading dot and lowercases before lookup. Kotlin deliberately does not
claim `.kts` (`docs/contract/adapters/kotlin.md:19-24`).

### Grammar decisions with contract weight

- **Kotlin uses `tree-sitter-kotlin-ng`.** The old `tree-sitter-kotlin 0.3.8`
  pins tree-sitter `< 0.23`; Cargo permits only one package with
  `links = "tree-sitter"`, so it cannot coexist with workspace 0.25. The -ng
  crate depends on `tree-sitter-language 0.1`, exports `LANGUAGE`, and has
  tree-sitter 0.24 as dev-only. All unsafe bridge code was removed. This is
  drift D-6 in `docs/drift.md:57-61`, closed in iteration 42.
  `src/kotlin/mod.rs:45` is `tree_sitter_kotlin_ng::LANGUAGE.into()`.
- **PHP uses `LANGUAGE_PHP`, not `LANGUAGE`.** The cached crate exports
  `LANGUAGE_PHP` (full grammar) and `LANGUAGE_PHP_ONLY`; no `LANGUAGE` symbol
  exists. Both `php/discover.rs:24` and `php/body.rs:79` use the full constant
  so `<?php`, inline HTML and `?>` are understood
  (`docs/contract/adapters/php.md:15-26`; review check 12 at
  `.reports/reviews/J35-J39.md:120-121`).
- **The Kotlin raw-pointer bridge was judged SOUND but rejected.** The review
  verified `repr(transparent)` thin pointers, ABI 14 inside the workspace
  range 13..=15, and no double-drop (`.reports/reviews/J35-J39.md:48-69`),
  but native `links` uniqueness is a build constraint, not a UB question.

### Discovery

- Kotlin: `function_declaration`, primary/secondary constructors and
  companion-object members; qualified names `Class.method`; wrapper kinds
  renamed by the -ng grammar (`identifier`, `non_nullable_type`,
  `block_comment`, `return_expression`/`throw_expression`, `statement`,
  `class_parameters`, …) are handled explicitly
  (`docs/contract/adapters/kotlin.md:54-102`).
- C#: methods, constructors, C# 12 primary constructors and first-level local
  functions; `file_scoped_namespace_declaration` has no `body` field, so its
  members are reached as siblings in `compilation_unit`
  (`docs/contract/adapters/csharp.md:36-74`, `:90-111`).
- Ruby: module-level `method`/`singleton_method` and direct `class`/`module`
  body members; `body_statement`, `method_parameters`, `argument_list`,
  `begin`/`rescue`/`elsif` and `parenthesized_statements` are unwrapped;
  `_`-prefixed grammar types are field descriptors, never concrete nodes
  (`docs/contract/adapters/ruby.md:22-79`).
- PHP: program/namespace-level `function_definition` and class
  `method_declaration`; nested functions, anonymous and arrow functions are
  ignored; `$` sigils are stripped from parameter and input names
  (`docs/contract/adapters/php.md:36-84`).
- Swift: `function_declaration`, `init_declaration` and type members;
  `class_declaration` is the grammar's single node for class/struct/actor/
  extension/enum, filtered by its `declaration_kind` field; nested functions
  and lambdas are ignored (`docs/contract/adapters/swift.md:43-64`,
  `:102-121`).
- All five sort by `(line, qualified_name)`, dedupe on that pair, keep exact
  source slices and 1-based lines, and append nothing to the locked schema
  (`docs/contract/rust-api.md`).

### Body analysis

- Top-level statements in source order get ids `step-1`, `step-2`, … and one
  of four kinds: `call`, `branch`, `transform`, `return` (spec step 3).
- A declaration or assignment whose initializer invokes a function is a
  `call` step, not `transform` — the locked spec example labels
  `const tokens = tokenize(input.sentence)` as `"kind": "call"`. Iteration 40
  records the four logic fixes in this batch as "All judged CODE against
  locked spec §4."
- `code` is one line; `inputs` are referenced identifiers in first-seen
  order; `title`/`detail` come from adjacent comments or deterministic
  fallbacks. `callee_names` carry raw names; a `Call` step carries
  `calls_fn_id` when the callee is known, and core alone resolves it into a
  bidirectional `calledBy` edge.
- Errors are de-duplicated by code with an enclosing-condition `when`. Status
  hints are shared across adapters: 422 for `TOKEN_NOT_FOUND`,
  `VALIDATION_ERROR`, `INVALID_INPUT`; 400 for `EMPTY_TEXT`,
  `EMPTY_SENTENCE`, `INSECURE_PROTOCOL`, `BAD_REQUEST`; 404/401/403 for
  named auth codes; 500 otherwise. Swift adds 429 for `RATE_LIMITED`
  (`docs/contract/adapters/swift.md:154-157`).
- Purity and permissions follow conservative marker scans of the function
  slice: console (`console:out`), filesystem (`fs:read`/`fs:write`), network
  (`net:outbound`) and environment (`secrets:read:<KEY>`). Budgets stay
  `150 ms`/`universal` pure, `1000 ms`/`node20` side effect,
  `5000 ms`/`node20` network. These runtime strings are recorded labels,
  not executors.

### Fixtures

| Fixture | Documented callables | Call edge / evidence |
| --- | --- | --- |
| `tests/fixtures/kotlin_app/` | `Result.<init>`, `Greeter.<init>`, `Greeter.of`, `Greeter.greet`, `echo`, `evaluate` | `evaluate` → `of`/`greet`/`echo`; `require` + `throw`; `println` (`README.md:19-54`) |
| `tests/fixtures/csharp_app/` | at least 8, including `TextTools.Analyze`, `Normalize`, `Add`, `TextTools.Run.Localize`, `Ping.Ping` | `Analyze` → `Normalize`/`Add`; guarded throws; `Console.WriteLine`, `File.ReadAllText`, `HttpClient` (`README.md:19-83`) |
| `tests/fixtures/ruby_app/` | `normalize_text`, `strip_controls`, `report`, `Formatter.format`, `default_formatter` | `normalize_text` → `strip_controls`; `EMPTY_TEXT` raise; `puts`; `Struct` index (`README.md:17-50`) |
| `tests/fixtures/php_app/` | `normalize_text`, `classify`, `report`, `add`, `Calculator.scale` | `classify` → `normalize_text`, `Calculator.scale` → `add`; `echo`/`printf`; `Calculator` index (`README.md:17-57`) |
| `tests/fixtures/swift_app/` | `Grade.describe`, `grade`, `main`, `normalize`, `report` | `grade` → `normalize`; `main` → `report`; guard + `if` throws; `print` (`README.md:10-45`) |

### Registration and default globs

J41 registered all five in `with_builtin_adapters` after the original ten,
with a comment that new languages register last so first-claim order cannot
change (`crates/fnscope-parser/src/adapter.rs:69-86`). The built-in extension
test pins `cs`, `kt`, `php`, `rb`, `swift` (`:121-148`). Core's default
includes carry `**/*.cs`, `**/*.kt`, `**/*.php`, `**/*.rb`, `**/*.swift`
(`crates/fnscope-core/src/scan.rs:29-46`, assertion `:534-555`). No graph
builder, schema, enricher, server or UI file changed.

## Simulation contract

- `simulate.sh` is the canonical **post-impl** simulation, RECONSTRUCTED
  2026-11-26. It is pure shell: no `cargo`, no server, no language toolchain.
  It reads the five committed fixtures, asserts each extension's documented
  callables, asserts every documented edge has both endpoints in that
  fixture's known set (no dangling), and checks the shipped registration,
  grammar and glob facts that make the scan reachable. It prints one
  `[PASS]`/`[FAIL]` line per assertion and a final `PASS` or `FAIL`, exiting
  0 only on PASS.
- `archive/simulate-pre.reconstructed.sh` is a **labelled design-time
  placeholder**. No pre-impl simulation artifact exists in the evidence; it
  asserts nothing and prints no verdict.
- All files here are LF-only: verified with `tr -cd '\r' < file | wc -c`
  equal to 0, in single-quoted WSL form, never `sed`
  (`docs/ethics/lessons.md:238-243`).

## Technical design

- One adapter directory per language with `mod.rs`, `discover.rs`, `body.rs`;
  grammar knowledge stays inside it. The shared resolver remains
  grammar-free, so the iteration 16 zero-core-change invariant holds.
- Workspace dependencies: `tree-sitter-kotlin-ng = "1.1"` (not the old
  `tree-sitter-kotlin`), `tree-sitter-c-sharp = "0.24"`,
  `tree-sitter-ruby = "0.23.1"`, `tree-sitter-php = "0.25"`,
  `tree-sitter-swift = "0.7.4"`; matching `.workspace = true` entries in
  `crates/fnscope-parser/Cargo.toml` (`Cargo.toml:30-43`).
- Tests live beside the code. Final source counts are Kotlin 14, C# 13,
  Ruby 9, PHP 16, Swift 10 (`docs/STATUS.md:148-153`); the review-time Swift
  count was 9 and F1 was routed back to J39 (iteration 36). The gate, not a
  format marker, is the correctness evidence (L11,
  `docs/ethics/lessons.md:226-252`).
- Delivery: iteration 42 records `ADAPTER_GATE_STATUS=0` with 233 tests
  (parser 176), a mixed scan of 78 functions / 159 steps / 33 resolved calls
  / 38 caller edges / 23 error facts / 0 dangling edges, `CULTURE_OK`,
  `ALL_TESTS_OK`, D-6 Resolved, and commit `d6b02cf`.
  `docs/STATUS.md:369` records `d6b02cf`, 39 files, +11404 / -1.

## Non-goals

- **No Kotlin/C#/Ruby/PHP/Swift execution via `invoke`.** Node invocation
  stays a TypeScript/JavaScript runner; other languages return the
  deterministic simulated trace (`docs/features/fnscope-base/PRD-TRD.md:250-257`,
  `docs/STATUS.md:267-271`).
- No new schema field, no rename of locked contract names, no change to the
  core graph builder, enricher, server or UI.
- No registration edit inside an adapter worktree; registration belongs to
  one later reviewed job (J41).
- No claim that a syntax or format check is a Rust compile verdict
  (`docs/ethics/lessons.md:226-252`).
- No backdating: this reconstruction is dated 2026-11-26 and cites the
  iterations as historical evidence.

## Sources

`.reports/goal-build-fnscope.md` iterations 32-46;
`.reports/reviews/J35-J39.md`; `docs/drift.md` D-6;
`docs/ethics/lessons.md` L1/L2/L6/L10/L11;
`docs/STATUS.md`; `fnscope_architecture_agent_build_spec.md` section 4 step 3;
`docs/contract/{rust-api,adapter-api}.md`;
`docs/contract/adapters/{kotlin,csharp,ruby,php,swift}.md`;
`crates/fnscope-parser/src/{kotlin,csharp,ruby,php,swift}/`;
`tests/fixtures/{kotlin_app,csharp_app,ruby_app,php_app,swift_app}/README.md`.
