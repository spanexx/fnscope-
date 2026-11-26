# fnscope-final-languages — IMPL (implementation plan, reconstructed)

RECONSTRUCTED 2026-11-26, written after the batch shipped. Job names, gate
markers, findings and the commit hash below are quoted from the ledger, the
review and git — none is invented. Status: SHIPPED.

This is the last polyglot batch: J35 (Kotlin), J36 (C#), J37 (Ruby), J38 (PHP)
and J39 (Swift) adapters; J40 (independent strict review); J41 (one
registration job); J42 (Kotlin grammar port); J43 (manifest rename); J44
(docs closure) and J45 (`docs/STATUS.md`). The orchestrator owned the
workspace root, the registry, the manifests, core defaults and every gate.
Agents wrote files in isolated worktrees, reported what they wrote, and never
ran cargo, merged or committed. The phase-by-phase record is in
`.reports/goal-build-fnscope.md` iterations 35-42; the review is
`.reports/reviews/J35-J39.md`; the drift entry is `docs/drift.md` D-6.

## Phases J35-J45

| Phase | Owner | Owned files / scope | Reported deliverable | Ledger |
| --- | --- | --- | --- | --- |
| J35 | adapter agent (Kotlin) | `crates/fnscope-parser/src/kotlin/{mod,discover,body}.rs`, `docs/contract/adapters/kotlin.md`, `tests/fixtures/kotlin_app/` | `KotlinAdapter`, six trait methods, 14 tests; later ported to the -ng grammar | 32, 35, 36, 37, 42 |
| J36 | adapter agent (C#) | `crates/fnscope-parser/src/csharp/{mod,discover,body}.rs`, `docs/contract/adapters/csharp.md`, `tests/fixtures/csharp_app/` | `CSharpAdapter`, 13 tests, primary constructors, records, local functions | 32, 35, 36, 38-41 |
| J37 | adapter agent (Ruby) | `crates/fnscope-parser/src/ruby/{mod,discover,body}.rs`, `docs/contract/adapters/ruby.md`, `tests/fixtures/ruby_app/` | `RubyAdapter`, 9 tests, `Struct`/`attr_*` type index | 32, 35, 39, 40 |
| J38 | adapter agent (PHP) | `crates/fnscope-parser/src/php/{mod,discover,body}.rs`, `docs/contract/adapters/php.md`, `tests/fixtures/php_app/` | `PhpAdapter`, 16 tests, `LANGUAGE_PHP`, class constant index | 32, 35, 39, 40 |
| J39 | adapter agent (Swift) | `crates/fnscope-parser/src/swift/{mod,discover,body}.rs`, `docs/contract/adapters/swift.md`, `tests/fixtures/swift_app/` | `SwiftAdapter`, 10 tests (9 at review), `declaration_kind` filtering | 32, 35, 36, 40 |
| J40 | independent reviewer (no implementation authority) | read-only across the five detached worktrees, 30 owned files | `Verdict: FAIL`, 3 findings (F1-F3); Kotlin bridge judged SOUND; no cargo | 35, 36 |
| J41 | registration agent (fresh worktree) | shared files only: manifests, `lib.rs`, `adapter.rs`, core `scan.rs`, mixed fixture | all five registered as built-ins with default globs | 36, 37 |
| J42 | Kotlin agent (fresh port) | `crates/fnscope-parser/src/kotlin/` | replaced the raw-pointer bridge with `tree_sitter_kotlin_ng::LANGUAGE` | 37, 38, 40 |
| J43 | manifest agent | root `Cargo.toml`, `crates/fnscope-parser/Cargo.toml` | renamed the two grammar dependency lines after D-6 | 37, 38, 42 |
| J44 | docs agent | `README.md`, `docs/features/fnscope-base/{PRD-TRD,IMPL,VERIFY}.md`, four adapter docs | SHIPPED status, polyglot evidence, stable F3 references | 42 |
| J45 | docs agent (delivery) | `docs/STATUS.md` | detailed current-state document, suffix counts reconciled | 43 |

Orchestrator steps between phases: merge the five worktrees plus registration
(iteration 37); diagnose and record D-6 before the grammar fix; re-merge and
gate repeatedly with exact diagnostics routed back to the owning jobs
(iterations 38-41); final gate, polyglot scan, culture check, explicit-path
stage and commit (iteration 42); docs batch and push (iteration 43).

## Delivery notes

- Reported counts were **not trusted until review and gate**. Iteration 35
  says the five were "reported complete" and that J40 was dispatched for one
  strict review. The review counted `#[test]` markers itself and found F1:
  Swift claimed 10 but had 9 (`.reports/reviews/J35-J39.md:135`, `:145`).
  Iteration 36 routed F1 to J39; final source has 10
  (`docs/STATUS.md:160-163`).
- Format and syntax checks were labelled as such. J36's doc states
  `rustfmt --edition 2021 --check` -> `RUSTFMT_STATUS=0` and calls it
  "format-only evidence, **not** a compile check"
  (`docs/contract/adapters/csharp.md:174-179`). The review states "No cargo,
  no rustfmt, no gate execution by this reviewer"
  (`.reports/reviews/J35-J39.md:28`). L11: a format marker never stands in
  for a compile result (`docs/ethics/lessons.md:226-252`).
- Byte hygiene used `tr`, never `sed`. The review re-ran
  `tr -cd '\r' < file | wc -c` on all 30 owned files with a positive control
  of 2 and a negative control of 0 (`.reports/reviews/J35-J39.md:16-18`,
  `:43`, `:84`, `:102`, `:119`, `:137`). L10/L11 forbid `sed 's/\r$//'`.
- Findings about unowned files were routed, not edited away: F2 (the C#
  README table row) went to J36, F3 (stale adapter-doc line numbers) to a
  later docs job, and the Kotlin registration risk to J41/J42/J43
  (`.reports/reviews/J35-J39.md:143-166`).
- No agent committed. The batch landed as one orchestrator commit after the
  gates, matching "one logical change = one commit".

## Real gate markers (quoted)

- **J40 review (iteration 36, `.reports/goal-build-fnscope.md:340-346`):**
  `Verdict: FAIL` with 3 findings; "Kotlin's raw-pointer bridge was explicitly
  judged SOUND (repr(transparent) thin pointers, ABI 14 supported, no
  double-drop)." No merge until F1/F2 were fixed.
- **First real gate (iteration 37, `:347-354`):** `cargo test --workspace`
  failed at dependency resolution. Root cause: native `links = "tree-sitter"`
  conflict from `tree-sitter-kotlin 0.3.8` (pins 0.21/0.22) with workspace
  0.25. D-6 was recorded before the fix; J42/J43 dispatched.
- **Second gate (iteration 38, `:355-362`):** `ADAPTER_GATE_STATUS=101`; 49
  compile errors across the five adapters — PHP/Ruby `type_index`
  accessibility, C# trait-call form and lifetimes, Swift `node_text(&Node)`
  borrows and an unused import, Kotlin `String.filter` and a missing trait
  import plus lifetimes. Exact locations sent back to J36-J39/J42.
- **Third gate (iteration 39, `:363-370`):** 6 lifetime errors remained —
  php/body.rs 198, 226, 498; php/discover.rs 96; ruby/body.rs 169, 211.
  Class: iterator temporary outliving cursor.
- **Fourth gate (iteration 40, `:371-378`):** compile green, 175 parser
  tests, 7 logic failures (C# 4, Kotlin 1, PHP 1, Swift 1). "All judged CODE
  against locked spec §4." Exact assertions sent to J36/J38/J39/J42.
- **Fifth gate (iteration 41, `:379-386`):** one compile error,
  `csharp/discover.rs:554:27` E0597 `type_cursor` outlived by
  `.children(...).any(...)` in J36's new nullable-parameter helper.
- **Final gate (iteration 42, `:387-394`):** `ADAPTER_GATE_STATUS=0`, 233
  tests (parser 176); mixed scan "78 functions across
  .ts/.js/.rs/.py/.go/.java/.c/.cpp/.cs/.kt/.rb/.php/.swift, 159 steps,
  33 resolved calls, 38 caller edges, 23 error facts, 0 dangling";
  `CULTURE_OK`, `ALL_TESTS_OK`; D-6 Resolved.
- **Commit (iteration 42, `:387-394`):** `d6b02cf feat: Kotlin, C#, Ruby, PHP
  and Swift language adapters`; `docs/STATUS.md:369` records 39 files,
  +11404 / -1.

## What changed on each side

- Kotlin: the `mem::transmute` bridge and all unsafe code were deleted; the
  dependency became `tree-sitter-kotlin-ng = "1.1"` in both manifests and
  `tree_sitter_kotlin_ng::LANGUAGE.into()` in `mod.rs:45`; -ng node kinds were
  remapped with all 14 tests preserved (D-6, `docs/drift.md:57-61`).
- C#: four logic fixes (call-initializer kind, required flag, default value,
  doc comment) plus the final nullable-parameter lifetime fix; the F2
  README table row was corrected.
- Ruby and PHP: iterator-temporary lifetime fixes at the sites quoted above;
  PHP also fixed `$result: unknown` inference to `int`.
- Swift: the F1 tenth test was added; the `"true)"` vs `Bool(true)` default
  value fix aligned inference with the locked contract.
- Registration (J41) is the only change to shared files: `Cargo.toml`,
  `crates/fnscope-parser/Cargo.toml`, `src/lib.rs`, `src/adapter.rs`,
  `crates/fnscope-core/src/scan.rs`, and the mixed polyglot fixture.

## Verification artifacts

`simulate.sh` in this directory is the canonical post-impl simulation: it
reads the five fixtures, asserts each extension's documented callables and
edges, checks that no edge endpoint is dangling, and prints PASS/FAIL. It
does not run cargo. `archive/simulate-pre.reconstructed.sh` is a labelled
placeholder: no pre-impl simulation script exists in the evidence.

All five files in this pack were checked for LF-only bytes with
`tr -cd '\r' < file | wc -c` == 0 via the single-quoted WSL form, never
`sed`, following L10/L11 (`docs/ethics/lessons.md:202-252`).
