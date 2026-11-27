# fnscope-rust-backend — PRD + TRD

Status: Locked (reconstructed 2026-11-26)

This pack reconstructs the origin document for the Rust language backend. The
decisions are locked in `GRILL-fnscope-rust-backend.txt`; the implementation
history is in `IMPL-fnscope-rust-backend.md`. It is written from the committed
record on 2026-11-26, not backdated, and it quotes gate results from
`.reports/goal-build-fnscope.md` iterations 15-20 rather than re-running them.
The locked integration contract remains `docs/contract/rust-api.md`; after the
J21 refactor the adapter surface is `docs/contract/adapter-api.md`.

## Why

The base feature shipped with a TS/JS scanner and an explorer that could show
its function graph. The user then asked to scan the repository's own Rust tree
(`crates/`). The real command returned `scanned 0 functions in 0 files`: a
capability gap, not a bug (`.reports/goal-build-fnscope.md:170-174`). The
product need is that a Rust tree yields the same useful, bidirectional function
graph as a TypeScript tree, with no new manifest, no new schema field and no
Rust runtime. The user removed the question loop, so all answers below are
`[agent decision]` with WHY and SOURCE in the GRILL.

The base pack had already fixed the architecture direction: one binary in Rust,
tree-sitter grammars for deterministic AST truth, a schema-valid manifest and
an embedded UI. The Rust backend is the first proof that this design extends
past TS/JS; the later polyglot work (iteration 16) depends on it.

## Product goal and outcome

A developer points fnscope at a Rust directory and sees real functions,
ordered steps, throws, purity, permissions and call edges in the same explorer.
The observable outcome is:

- `fnscope scan tests/fixtures/rust_app` writes a schema-valid `.fnscope/fn-graph.json`
  with at least seven functions.
- `fnscope scan crates/` reports a non-zero, accurate graph over the repository's
  own source. Recorded: 188 functions / 722 steps / 243 `calledBy` edges /
  243 resolved calls / 0 dangling (iteration 19, ledger line 208), then
  190 / 732 / 245 / 245 / 0 after the quality fix (iteration 20, line 216).
- The four explorer views (Raw, Split, Story, 1-Hop Map) need no Rust-specific
  code; the graph shape is unchanged.
- Invoking a Rust function returns the deterministic simulated trace. There is
  no Rust executor.

## Behavioral spec

Commands run from the repository root in WSL Debian unless a Windows
`curl.exe` probe is named. These are operator-visible actions, not a claim that
this reconstruction re-ran the gates.

1. Scan the Rust fixture.
   `./target/release/fnscope scan ./tests/fixtures/rust_app --out <dir>`
   writes `<dir>/fn-graph.json`. At least seven functions are present:
   `add`, `Calculator.new`, `Calculator.scale`, `normalize_text`, `classify`,
   `report` and `passthrough`. The names and evidence are tabulated in
   `tests/fixtures/rust_app/README.md` ("Expected scanner output").
   Recorded result: 7 functions, 12 steps, 2 `calledBy` edges, 2 resolved
   calls, 0 dangling (ledger line 208).

2. Dotted method identity. `Calculator.scale` is discovered as a method with
   qualified name `Calculator.scale` and graph id `calculator-scale`. The
   qualification lives in the id, not in `name`; the locked schema has no
   `qualified_name` field. Its steps include a `call` step whose `callsFnId`
   resolves to `add` (ledger line 210's accepted expectation; fixture README
   section "Expected scanner output"; `make_id` at
   `crates/fnscope-core/src/normalize.rs:232-265`).

3. Fallible guard. `classify` reports `EMPTY_TEXT` from
   `Err(AppError { code: "EMPTY_TEXT" })`. The first scan reported no error
   (ledger line 210); after J15's fix the recorded fixture detail is
   `classifyErrors == ["EMPTY_TEXT"]` (ledger line 216). Its body also calls
   `normalize_text` and branches on `normalized.ends_with("er")`.

4. Side effect. `report` has `purity: "side-effect"` and `permissions` include
   `console:out` because of `println!`. This is the fixture's effect example
   (`tests/fixtures/rust_app/README.md` section "`report` is a side effect";
   `crates/fnscope-parser/src/rust/body.rs:970,989`).

5. File filtering. `src/skip.rs` is included by default despite its name. A
   generated file under `target/` is excluded by the default `**/target/**`
   glob; the include and exclude sets are in
   `crates/fnscope-core/src/scan.rs:29-46`.

6. Inspect through the server. `fnscope open <fixture>` serves the same UI and
   routes as before: `GET /`, `GET /api/fn-graph.json`, `POST /api/invoke`.
   A Rust id posted to `/api/invoke` in `--invoke simulate` mode returns
   `ok: true` with a `trace` object; it never compiles or runs Rust.

7. Inspect the real tree. `fnscope scan crates/` is the headline request
   (ledger line 171). It must report non-zero functions with valid `calledBy`
   links and zero dangling edges (expanded acceptance evidence at line 183).

## Simulation contract

The canonical simulation is `docs/features/fnscope-rust-backend/simulate.sh`.
It demonstrates the post-impl behavior with the real built binary when one is
present; otherwise it enters a documented placeholder mode that prints SKIP
for every scenario and `CANONICAL_SIM_PLACEHOLDER`, never a fabricated PASS.
It never runs cargo. Its verdicts are local observations, not a re-run of the
orchestrator's recorded gates in `VERIFY.md` or the ledger.

- S1 - scan. Run `fnscope scan tests/fixtures/rust_app --out <scratch>`, require
  the manifest to exist, to parse, and to contain at least seven functions.
- S2 - dotted method id. Require a function named `scale`, id
  `calculator-scale`, qualified name `Calculator.scale`, and a `call` step
  whose `callsFnId` resolves to the id of `add`.
- S3 - `EMPTY_TEXT`. Require the `classify` record's `errors` array to contain
  a code `EMPTY_TEXT`.
- S4 - side-effect permission. Require the `report` record to have
  `purity == "side-effect"` and `permissions` containing `console:out`.
- S5 - no dangling edges. Run `node scripts/check-manifest.mjs <manifest>` and
  require `MANIFEST_OK`; the checker rejects any non-null `calledBy[].fnId` or
  `steps[].callsFnId` that does not resolve to a known id
  (`scripts/check-manifest.mjs:92-135`).
- Verdicts. Print one `[PASS]` or `[FAIL]` line per scenario and end with
  `CANONICAL_SIM_OK` only if every scenario passed, `CANONICAL_SIM_FAIL`
  otherwise, and `CANONICAL_SIM_PLACEHOLDER` when no binary or prerequisite is
  available.
- Bounds and cleanup. No server is started, so there are no waits to bound.
  Scratch files live outside the repo in a `mktemp -d` directory removed by a
  trap. No `.fnscope` directory is left behind (AGENTS.md, "Command lane").

The design-time mirror is
`archive/simulate-pre.reconstructed.sh`; it uses placeholder data and is not
backdated or treated as evidence.

## Technical design

### Grammar and module layout

`tree-sitter-rust` 0.24.2 is a workspace/parser dependency (ledger line 172).
All Rust grammar knowledge lives in `crates/fnscope-parser/src/rust/`:
`mod.rs` defines `RustAdapter`, `discover.rs` finds callable nodes and
signatures, `body.rs` produces steps, errors, purity, permissions and callees.
After J21, `RustAdapter` implements `LanguageAdapter` (`id() = "rust"`,
`extensions() = &["rs"]`) and `fnscope-core` selects it once per file from the
suffix (`.reports/reviews/J21.md` checklist items 1-5). There is no Rust branch
in the graph builder, schema, enricher or UI.

### Discovery facts

- Free functions (`function_item` at file scope), associated functions and
  methods inside `impl` blocks are discovered; `self` is a receiver, not a
  named parameter.
- `name` is the bare identifier; `qualified_name` is `Type.method` for methods.
- `line` / `end_line` are 1-based; `source_code` is the exact node byte slice.
- Parameters keep declared type text; `Result<Label, AppError>` is reported as
  the declared return type and unpacked via the project-wide type index.
- Leading `///` comments become `doc_description`; `@tags` populate `tags`.
- No Rust async keyword is mapped, so `is_async` is false.

### Body analysis

Top-level statements become ordered steps with ids `step-1`, `step-2`, ...
(`docs/contract/rust-api.md:51-59`). Assignments and expressions that call a
function are `Call` steps (spec section 4 step 3); `if` becomes `Branch`; a
`return` or final expression becomes `Return`. `inputs` are referenced
identifiers in first-seen source order; `callee_names` are raw bare callee
names, and a callee known in the file set becomes that step's `callsFnId`.
`fnscope-core` maps the name to the graph id and populates the reverse
`calledBy` edges, with `fn_id: None` for external entrypoints.

Errors come from `return Err(...)`. `rust_err_code` and
`rust_struct_code_field` read a literal string, a `code: "VALUE"` field of a
struct expression, a variant name (`AppError::NotFound`) or a declared const;
`enclosing_condition` provides `when` (for example `text.is_empty()`).
`build_errors` deduplicates by code with a stable status hint. This is the
J15 fix for the iteration-19 defect (`body.rs:772-950`).

### Purity, permissions, budgets

`classify_rust_effects` scans the function source for conservative markers:
`println!` / `eprintln!` -> `console:out`, network markers -> `net:outbound`,
filesystem markers -> `fs:read` / `fs:write`, environment reads ->
`secrets:read:<KEY>`. Any marker means `Purity::SideEffect`; no marker means
`Pure` with no permissions (`body.rs:964-1024`). Budget labels stay
150 / 1000 / 5000 ms with `universal` / `node20` as carried data (decision R6).

### Identifiers and hashes

`make_id` slugifies the qualified name to stable kebab-case; `Calculator.scale`
becomes `calculator-scale` (`crates/fnscope-core/src/normalize.rs:232-265`).
`ast_hash` is `sha256:` plus the hash of whitespace/comment-normalized source
(`docs/contract/rust-api.md:152-153`), so the enrichment cache keeps working
on Rust functions without a language branch.

### Tests and gates

Parser unit tests live beside the code in `#[cfg(test)]` (`rust/mod.rs:44-105`
holds the pre-refactor fact tests). The facade `discover` / `analyze`
functions still exist and delegate to adapters, preserving the 38 original
test bodies byte-for-byte (`.reports/reviews/J21.md` checklist item 6).
Compile gates are orchestrator-owned: `RUST_GATE_STATUS=101` at iteration 18
(E0597 at `body.rs:240`), `RUST_GATE2_STATUS=0` with 35 parser tests at
iteration 19 (ledger lines 200 and 208). No agent ran cargo, and this
documentation job did not either.

## Acceptance evidence (recorded, not re-run)

- Iteration 19: `cargo test --workspace` -> `RUST_GATE2_STATUS=0`, parser 35
  tests (ledger line 208).
- Iteration 19: fixture -> 7 functions, 12 steps, 2 `calledBy` edges,
  2 resolved calls, 0 dangling (line 208).
- Iteration 19: `crates/` -> 188 functions, 722 steps, 243 edges,
  243 resolved calls, 0 dangling (line 208).
- Iteration 20: fixture `classifyErrors == ["EMPTY_TEXT"]`; `crates/` ->
  190 functions, 732 steps, 245 edges, 245 resolved calls, 0 dangling
  (line 216).
- Committed as one reviewed code batch: `a961c53` (62 files, explicit staging),
  recorded at line 216. The registry refactor followed as `a6f5cde`
  (iteration 24); see `IMPL-fnscope-rust-backend.md` for the hash caveat.
- TS/JS gates stayed green; the scope expansion required that existing TS/JS
  behavior and tests be preserved (ledger line 184).

## Non-goals

- **No Rust execution via invoke.** `/api/invoke` is a TypeScript/JavaScript
  runner in `node` mode and a deterministic trace builder in `simulate` mode.
  A Rust function is inspected and navigated, never executed; invoking one
  returns the simulated trace. This is the explicit non-goal named in the job
  and in `README.md` "Only TS/JS are executed".
- **No new schema field or manifest.** There is no `qualified_name` field, no
  per-language manifest and no schema-version bump. The impl qualification is
  carried in `id` (`calculator-scale`), accepted at ledger line 210.
- **No Rust-specific runtime or executor field.** `universal` / `node20`
  remain carried labels, not promises (decision R6).
- **No hand-rolled or regex parser.** Grammar facts come from tree-sitter
  (decision R1).
- **No core, schema, enricher or UI change per language.** After J21 a new
  language is an adapter plus registration only (ledger lines 182-184; L9).
- **Not a gate re-run.** This reconstructed pack does not claim fresh cargo,
  server or browser results; it quotes the orchestrator's recorded evidence.
