# fnscope-python-go — IMPL (implementation plan, reconstructed)

RECONSTRUCTED 2026-11-26, written after the batch shipped. Job names, gate
markers and the commit hash below are quoted from the ledger, the reviews and
git — none is invented. Status: SHIPPED.

This is the first non-Rust polyglot batch: J23 (Python) and J24 (Go) adapters,
J25 (independent review), J26 (single registration job), J27 (read-only
triage). The orchestrator owned the workspace root, the registry, the
manifests, core defaults and every gate. Agents wrote files in isolated
worktrees, reported what they wrote, and never ran cargo, merged or committed.
The exact phase-by-phase record is in `.reports/goal-build-fnscope.md`
iterations 25-30; the review is `.reports/reviews/J23-J24.md`; the failure
triage is `.reports/reviews/lang-batch-triage.md`.

## Phases J23-J27

| Phase | Owner | Owned files / scope | Reported deliverable | Ledger |
| --- | --- | --- | --- | --- |
| J23 | adapter agent (Python) | `crates/fnscope-parser/src/python/{mod,discover,body}.rs`, `docs/contract/adapters/python.md`, `tests/fixtures/python_app/` | `PythonAdapter`, six trait methods, 10 tests, coherent fixture with `__pycache__` decoy | 25, 26, 27 |
| J24 | adapter agent (Go) | `crates/fnscope-parser/src/golang/{mod,discover,body}.rs`, `docs/contract/adapters/go.md`, `tests/fixtures/go_app/` | `GoAdapter`, six trait methods, 16 tests, struct/interface type index | 25, 26, 27 |
| J25 | independent reviewer (no implementation authority) | read-only across both detached worktrees at `a6f5cde` | `Verdict: FAIL`, 10 findings (F1-F10); no cargo run; "PASS (source)" only where no defect was found | 26 |
| J26 | registration agent (fresh worktree) | shared files only: manifests, `lib.rs`, `adapter.rs`, core `scan.rs` defaults | F3/F5-F8 registration edits, grammar deps, module lines, globs and `__pycache__` exclude | 26, 27 |
| J27 | read-only triage agent | no edits, no cargo, no test re-run | 9 failures decided CODE 7 / TEST 1 / BOTH 1 with contract citations | 28, 29 |

Orchestrator steps between the phases: re-merge worktrees with `cp` and
`tr`-verified `CR=0` on 23 files (iteration 27); route F1/F2/F4 back to J23,
F9/F10 back to J20, registration findings to J26; re-gate; mixed scan; culture
check; stage explicit paths; commit (iteration 30).

## Delivery notes

- Reported counts were **not trusted until review and gate**. Iteration 25
  says so verbatim: "Verification reported by implementers (NOT trusted until
  review+gate): Python 10 tests, Go 16 tests". The reviewer matched the counts
  at `.reports/reviews/J23-J24.md:28`, `:43`, and noted that per-test names
  were not recorded (`:79`).
- Format and syntax checks were labelled as such. The review states:
  "No `cargo` command was run by this review. A `rustfmt --check` marker would
  prove formatting only, not that identifiers or types resolve (L11)" and
  records the agents' Python AST / `go vet` syntax-only checks
  (`.reports/reviews/J23-J24.md:15-17`).
- Byte hygiene used `tr`, never `sed`. The reviewer re-ran
  `tr -cd '\r' < file | wc -c` on 16 Python-owned and 7 Go-owned files with
  `CR=0` each, positive control 2 and negative control 0
  (`.reports/reviews/J23-J24.md:15`, `:30`, `:45`); the orchestrator repeated
  the check on 23 merged files (iteration 27). Rule: L11,
  `docs/ethics/lessons.md:238-243`.
- Review findings about unowned files were recorded, not edited away: F5
  (`adapter.rs:124`), F6-F8 (manifests, `lib.rs`, core globs) and F9-F10
  (stale `language-adapter.md` plus L11-class truncated tokens) are all in
  `.reports/reviews/J23-J24.md:55-60` and were routed to J26/J20.
- The `__pycache__/generated.py` decoy is an intentional fixture, not a
  leftover temp file (`.reports/reviews/J23-J24.md:13`, `:62`).
- No agent committed. The batch landed as one orchestrator commit after the
  gates, matching the "one logical change = one commit" culture rule.

## Real gate markers (quoted)

- **J21 pre-batch warning (iteration 23, `.reports/goal-build-fnscope.md:240`):**
  `cargo test --workspace` -> 12 `E0425` errors after a false reviewer PASS;
  cause recorded as L10 recurrence + L11 format-vs-correctness confusion.
  This is why J25 carried the semantic-sanity checklist item.
- **J25 review (iteration 26, `:263-265`):**
  `.reports/reviews/J23-J24.md` `Verdict: **FAIL**` with 10 findings; F1 is a
  deterministic E0308 defect (nine `&[u8]`/`&str` call sites,
  `python/discover.rs:381`). Go's own source had no defects.
- **First real compile gate (iteration 27, `:270-276`):**
  `cargo test --workspace` -> `ADAPTER_GATE_STATUS=101`; six E0597
  iterator-temporary errors (4 Python, 2 Go) plus one `unused_mut` warning in
  the TypeScript adapter; `Cargo.lock` correctly gained tree-sitter-python/go
  0.25. Verdict: fail (borrow lifetimes, L1 class).
- **Second gate (iteration 28, `:278-284`):**
  `ADAPTER_GATE_STATUS=101`; parser 61 passed / 9 failed; exact assertions in
  `/tmp/fnscope-adapter-gate.log`. Pivot: read-only triage (J27) before edits.
- **Triage (iteration 29, `:286-292`):**
  CODE 7, TEST 1, BOTH 1; root causes `statement_list`,
  `field_declaration_list`, `expression_statement` plus one contract-breaking
  expectation; D-5 appended for the Go adapter-doc/step-kind drift.
- **Final gate (iteration 30, `:294-300`):**
  127 tests green, `ALL_TESTS_OK`, `CULTURE_OK`, mixed polyglot scan "28
  functions across 5 extensions with 0 dangling edges", D-5 Resolved.
  `docs/drift.md:55` adds the mixed-scan detail: "28 functions, 73 steps, 13
  resolved calls and 0 dangling edges" under `cargo test --workspace` ->
  `ADAPTER_GATE_STATUS=0`, 127 tests (parser 70).
- **Commit (iteration 30, `:295`):**
  `01062af` — 29 files; `git show --stat --oneline 01062af` confirms the two
  adapter directories, their contract docs, both fixtures, registry/glob edits
  and the three evidence files. The commit subject in git is `feat:`.

## What changed on each side

- Python wrapper fixes: `python/discover.rs:280-281` unwraps
  `expression_statement`; `python/body.rs:186-189` provides the shared
  statement helper; the one TEST-only change is the expected step vector in
  `python/mod.rs` (`Call, Branch, Call, Return`), with no production diff
  (`.reports/reviews/lang-batch-triage.md:94-102`).
- Go wrapper fixes: `golang/body.rs:90-95` unwraps `statement_list`;
  `golang/discover.rs:289-294` unwraps `field_declaration_list`;
  `golang/body.rs:328-334` returns `Call` when a declaration initializer has
  a call, and the BOTH test now expects `[Branch, Call, Call, Return]`
  (`.reports/reviews/lang-batch-triage.md:70-78`). `docs/contract/adapters/go.md:46`
  and D-5 were corrected as the doc half of the drift.
- Registration edits (J26, iteration 26) are the only changes to shared
  files: `Cargo.toml`, `crates/fnscope-parser/Cargo.toml`, `src/lib.rs`,
  `src/adapter.rs`, `crates/fnscope-core/src/scan.rs` — exactly the numbered
  list in `.reports/reviews/J23-J24.md:66-74`.

## Verification artifacts

`simulate.sh` in this directory is the canonical post-impl simulation: it
scans both fixtures, asserts the expected functions and edges, checks that no
edge is dangling and prints PASS/FAIL. It does not run cargo. The archived
`archive/simulate-pre.reconstructed.sh` is a labelled placeholder: no pre-impl
simulation script exists in the evidence.

All five files in this pack were checked for LF-only bytes with
`tr -cd '\r' < file | wc -c` == 0 via a single-quoted WSL script-file form,
never `sed`, following L10/L11 (`docs/ethics/lessons.md:202-252`).
