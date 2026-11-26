# fnscope-rust-backend — IMPL (implementation plan)

Status: SHIPPED (reconstruction written 2026-11-26)

This plan reconstructs the delivery of the Rust backend from the committed
record. Requirements are locked in `GRILL-fnscope-rust-backend.txt` and the
behavioral/simulation contract in `PRD-TRD-fnscope-rust-backend.md`. The jobs
J15-J18 were dispatched at `.reports/goal-build-fnscope.md:175`, landed by
iteration 18 (line 199), and were gated, fixed and committed across iterations
18-20. This document was written from that record; it did not run cargo and did
not re-run any gate.

## Job ownership (disjoint file sets)

| Job | Owned files | Delivered |
|---|---|---|
| J15 | `crates/fnscope-parser/src/discover.rs`, `body.rs` (later moved under `src/rust/`), parser grammar dependencies | tree-sitter-rust discovery, signatures, steps, throws, purity/permissions, callee names; fixes for E0597, `&Node` and `EMPTY_TEXT` |
| J16 | `crates/fnscope-core/` | `**/*.rs` default include, `**/target/**` default exclude, `Lang::Rust` suffix dispatch (later a registry lookup), id and `calledBy` wiring via the unchanged graph builder |
| J17 | `tests/fixtures/rust_app/` | `Cargo.toml`, `src/lib.rs`, `src/skip.rs`, `target/generated.rs`, `README.md`; the small Rust fixture with the `EMPTY_TEXT` guard and `console:out` side effect |
| J18 | root `README.md`, `docs/features/fnscope-base/` | user-facing docs and the base pack's polyglot addendum |

Compression note: J15/J16 first extended the TS/JS-shaped code directly
(`Lang::Rust` in shared types, ledger line 171). Iteration 16 expanded the goal
to a pluggable architecture (lines 178-189), so J21 later moved the grammar
behind `LanguageAdapter`. That refactor is recorded in the adapter contract and
in `docs/features/fnscope-base/IMPL.md`; it is not a J15-J18 edit.

## Phases and gates

### Phase J15 — Rust parser

Write the grammar-backed discovery and body analysis beside the existing TS/JS
code using the exact `DiscoveredFn` / `BodyAnalysis` / `Step` / `ErrorSpec`
shapes from `docs/contract/rust-api.md:94-127`. No schema field is added.

Gate: the orchestrator's workspace compile gate, not an agent run. First
result `RUST_GATE_STATUS=101` with one E0597 iterator-temporary lifetime error
at `crates/fnscope-parser/src/body.rs:240` (ledger line 200, iteration 18).
This is the L1 failure class: delegated code written without compiler
feedback. The exact diagnostic was sent back to J15 with a local-binding fix;
the orchestrator made no code edit (line 202).

After the fix: `RUST_GATE2_STATUS=0`, parser 35 tests (line 208, iteration 19).
Quality pass: the `Err(AppError { code: "EMPTY_TEXT" })` case was not detected
(`classify.errors == []`, line 210); J15 added struct-field error-code
extraction and a `&Node` fix. Re-gated green after those changes (line 216,
iteration 20).

### Phase J16 — core `.rs` discovery

Extend `ScanOptions::default()` in `crates/fnscope-core/src/scan.rs:29-46` with
`**/*.rs` in `includes` and `**/target/**` in `excludes`. Route `.rs` files to
the Rust parser through the then-current `Lang::Rust` dispatch and let the
unchanged graph builder assign ids, resolve `callsFnId` to graph ids, populate
reverse `calledBy` edges and compute `ast_hash`.

Gate: a real scan must find functions in `.rs` files and none under `target/`.
Baseline before the phase: `fnscope scan crates --out /tmp/fnscope-rust-scan`
-> `scanned 0 functions in 0 files` (line 171). Recorded after integration:
fixture 7 functions and `crates/` 188 functions with 0 dangling edges
(line 208). After J21 this dispatch became one registry lookup per file
(`scan.rs:243-249`), with `scan_with_registry` as the injection seam.

### Phase J17 — Rust fixture

Create `tests/fixtures/rust_app/` as a small, dependency-free edition-2021
crate. `src/lib.rs` holds `add`, `Calculator` with `new` and `scale`, `Label`,
`AppError`, `normalize_text`, `classify` and `report`; `src/skip.rs` holds
`passthrough`; `target/generated.rs` holds a function the default exclude must
drop. `README.md` tabulates the seven expected functions and the four evidence
sections (dotted impl name, `EMPTY_TEXT` guard, side effect, file filtering).

Gate: a default scan yields at least seven functions, including `passthrough`
and excluding the generated one. The recorded fixture scan does (7 functions,
12 steps, 2 `calledBy` edges, 2 resolved calls, 0 dangling; line 208). The
`EMPTY_TEXT` assertion was added after iteration 19 found the defect and
verified at iteration 20 (`classifyErrors == ["EMPTY_TEXT"]`, line 216).

### Phase J18 — documentation

Update root `README.md` with the Rust quick-start command, `.rs` globs and the
limits section (no Rust execution, carried runtime labels, best-effort
cross-file types). Extend `docs/features/fnscope-base/PRD-TRD.md` and `IMPL.md`
with the polyglot addendum so the base pack owns the forward-looking statement
while this pack owns the Rust origin decisions.

Gate: documentation review, not a compile. `docs/STATUS.md` records that the
README counts were later corrected to 16 suffixes
(`docs/STATUS.md:485-486`).

## Delivery notes — real markers and commits

In ledger order (quoted, not re-run):

- Iteration 15, line 172: `cargo info tree-sitter-rust` -> 0.24.2 available.
- Iteration 18, line 200: `cargo fmt --all && cargo test --workspace` ->
  `RUST_GATE_STATUS=101`; one E0597 at `crates/fnscope-parser/src/body.rs:240`.
- Iteration 19, line 208: `cargo test --workspace` -> `RUST_GATE2_STATUS=0`,
  parser 35 tests. Fixture -> 7 functions, 12 steps, 2 `calledBy` edges,
  2 resolved calls, 0 dangling. `crates/` -> 188 functions, 722 steps,
  243 edges, 243 resolved calls, 0 dangling.
- Iteration 19, line 210: two quality gaps recorded. (a) impl qualification is
  in `id` (`calculator-scale`), not `name`; the schema has no `qualified_name`.
  Accepted; the fixture doc was corrected. (b) `classify.errors == []` - a
  real bug routed back to J15.
- Iteration 20, line 216: `cargo test --workspace` green after J15's `&Node`
  fixes; `fixture_detail.classifyErrors == ["EMPTY_TEXT"]`; `crates/` ->
  190 functions, 732 steps, 245 edges, 245 resolved calls, 0 dangling.
  The Rust code batch was committed with explicit staging as `a961c53`,
  which the ledger describes as "feat: fnscope engine, CLI, server, UI and
  Rust backend" (62 files) on top of the two docs batches.
- Iteration 24: the adapter-registry refactor closed with `ADAPTER_GATE_STATUS=0`
  and was committed as `a6f5cde` ("refactor: pluggable language adapter
  registry", ledger line 249). The Rust grammar then lived at
  `crates/fnscope-parser/src/rust/`, preserving the pre-refactor tests
  byte-for-byte (`.reports/reviews/J21.md` checklist item 6).

Hash caveat: `docs/STATUS.md:373-376` records a later history rewrite, so the
current git subject of `a961c53` is simply `feat:`; subjects are cited as the
ledger recorded them at the time, never as current object names.

## Failure handling and culture carried forward

- L1 (`docs/ethics/lessons.md:29-53`): the E0597 compile error came from a
  parallel write window with no compiler feedback for the owning agent. The
  exact diagnostic went back to J15; no orchestrator code edit and no agent
  cargo run.
- L10 (`docs/ethics/lessons.md:202-224`): byte-level checks must be proven
  against a known positive and negative and live in a script, not an inline
  double-quoted `\r`. This job verifies LF with `tr -cd '\r' < file | wc -c`
  from a script file.
- L11 (`docs/ethics/lessons.md:226-252`) and drift D-4: J21's `rustfmt --check`
  green and the reviewer's original PASS did not mean the code compiled; the
  real gate found 12 E0425 errors after a `sed 's/\r$//'` normalization. The
  false PASS stays in `.reports/reviews/J21.md` as evidence, and the re-review
  cites only the orchestrator's `ADAPTER_GATE_STATUS=0` as compile evidence.
- D-2 (`docs/drift.md:33-37`): cross-file return fields were a resolved gap
  from the base build. The Rust adapter reuses the shared resolver instead of
  reintroducing a per-file alias resolution.

## What this reconstruction job did and did not do

Owned paths are only `docs/features/fnscope-rust-backend/`:
`GRILL-fnscope-rust-backend.txt`, `PRD-TRD-fnscope-rust-backend.md`, this
`IMPL-fnscope-rust-backend.md`, `simulate.sh` and
`archive/simulate-pre.reconstructed.sh`. No code file, ledger, contract, README
or sibling feature pack was modified. No cargo, build, test, server or commit
was run by this job. `bash -n` was used on the two shell scripts and the LF
byte count was checked with the script-file `tr` form described above; those
checks prove syntax and line endings only, not correctness of the recorded
gates.
