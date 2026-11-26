# fnscope-adapter-registry — IMPL (implementation plan, reconstructed)

RECONSTRUCTED 2026-11-26, written after the refactor shipped. Job names, gate
markers and the commit hash below are quoted from the ledger, the review,
`docs/drift.md` and git — none is invented. Status: SHIPPED.

This pack is the seam refactor: J21 wrote the `LanguageAdapter` trait, the
`Registry` and the `scan_with_registry` injection point in an isolated
worktree; J22 audited it with no implementation authority. The orchestrator
owned every gate, the re-merge and the commit. Agents wrote files, reported
what they wrote, and never ran cargo, merged or committed. The phase-by-phase
record is `.reports/goal-build-fnscope.md` iterations 20-24 (`:214-252`); the
audit is `.reports/reviews/J21.md`; the drift is `docs/drift.md:45-49`.

## Phases J21 and J22

| Phase | Owner | Owned files / scope | Reported deliverable | Ledger |
| --- | --- | --- | --- | --- |
| J21 | adapter-registry agent (isolated worktree) | `crates/fnscope-parser/src/**` (trait, registry, TS/Rust adapter modules, compatibility delegates, shared resolver, tests), `crates/fnscope-core/src/scan.rs` (seam only), `docs/contract/adapter-api.md`, `docs/features/fnscope-base/IMPL.md` | `LanguageAdapter` + `Registry`, seven built-ins at the time, five suffix adapters, one `for_extension` lookup per file in core, `scan_with_registry`, fake-adapter/zig test, 38 original tests preserved verbatim | 20, 22, 23 |
| J22 | independent reviewer (no implementation authority) | read-only across the J21 worktree and later current main | First `Verdict: PASS` with 0 findings; after the false green, `## Re-review 2026-11-26` with semantic-sanity item 12, `Verdict (re-review): PASS`, 0 findings, F1 superseded and F2 logged for the orchestrator | 22, 23, 24 |
| Orchestrator | gate owner (not an agent job) | re-merge, gates, drift closure, staging and commit | `cp`-only re-merge, `tr`-verified CR=0, real compile gate, D-4 Resolved, commit `a6f5cde` | 21, 23, 24 |

Between the phases the orchestrator adopted the reviewer-process workflow
(iteration 20, `:219`): each later brief states that an independent reviewer
audits it, pastes the checklist and forbids cargo, temp files and commits.
J21's reported markers were explicitly not trusted until review and gate
(iteration 22, `:232`).

## Delivery notes

- J21's `rustfmt --edition 2021 --check -> ALL_OWNED_RS_FMT_OK` proved
  formatting only. J22's own "Known limits" said it had not re-run rustfmt and
  had run no cargo; the first PASS was recorded at `.reports/reviews/J21.md:2`
  and `:27-28` and is kept there as historical evidence, not rewritten.
- The merge produced 12 `E0425` errors: `adapter` -> `adapte`, `declarator` ->
  `declarato`, `inner` -> `inne`, `member` -> `membe`. The cause was a
  byte-normalization command that treated `\r$` as a literal trailing `r`,
  traced to the orchestrator's merge step — not to J21's worktree files
  (`.reports/reviews/J21.md` re-review, "Root cause and fix actually applied";
  `docs/ethics/lessons.md:226-243`; `docs/drift.md:45`).
- The fix re-merged J21's files with `cp` only (no `sed`), verified 0 CR bytes
  with `tr -cd '\r' < file | wc -c` on all 13 files (`REMERGE_OK`), re-ran the
  real gate, and re-reviewed with a new semantic-sanity item. D-4 was appended
  before the fix and closed after it with a `Verified by:` note; the false PASS
  was not deleted (`docs/drift.md:45-49`).
- J22's re-review checked item 12 by scanning for the exact truncated tokens
  (0 hits) and for benign `+r` pairs (`parse`/`parser`, `resolve`/`resolver`,
  `use`/`user`). Compile correctness is cited only from the orchestrator's
  gate; a reviewer PASS means "no defect found by source audit".
- Scope stayed disjoint: `.reports/reviews/J21.md` checklist 9 lists only
  `crates/fnscope-parser/src/**`, `crates/fnscope-core/src/scan.rs`,
  `docs/contract/adapter-api.md` and `docs/features/fnscope-base/IMPL.md`.
  `crates/fnscope-parser/src/types.rs` was absent from the diff, so the locked
  contract types and serde names were untouched.
- The registration of later languages was deliberately deferred to separate
  jobs (ledger iteration 24, `:251`): two worktrees never edit `adapter.rs` at
  once. Python and Go were the first to use that rule (iterations 25-26).
- No agent committed. The refactor landed as one orchestrator commit after the
  gates, matching "one logical change = one commit".

## Real gate markers (quoted)

- **J21 reported (iteration 22, `.reports/goal-build-fnscope.md:232`):**
  `rustfmt --edition 2021 --check` clean, "five built-in suffix adapters, one
  registry lookup per file in core, `scan_with_registry` injection seam, and a
  fake-adapter/zig test. Not yet trusted or merged."
- **First compile gate after merge (iteration 23, `:240`):**
  `cargo test --workspace` -> 12 `E0425` errors; verdict fail, "reviewed code
  did not compile". `bash scripts/check-culture.sh` -> `CULTURE_OK` after J20's
  review-report evidence exception.
- **Re-merge and real gate (iteration 24, `:248`):**
  `cargo test --workspace` -> `ADAPTER_GATE_STATUS=0`, 101 tests (parser 44);
  `tr -cd '\r' < file | wc -c` -> 0 for all 13 files; SHA256 comparison
  13/13 byte-identical between worktree and main; J22 `Verdict (re-review):
  PASS` with 0 findings.
- **Commit (iteration 24, `:248`):**
  `a6f5cde refactor: pluggable language adapter registry`. `docs/drift.md:49`
  closes D-4 as Resolved with the gate output and the re-review as evidence;
  `docs/features/fnscope-base/IMPL.md:241-245` records the same batch.

## Verification artifacts

`simulate.sh` in this directory is the canonical post-impl simulation. It
reads the shipped Rust source, resolves every built-in suffix to its adapter
id, checks that an unregistered suffix resolves to `None`, checks that the
fake adapter is selected only through `scan_with_registry`, checks that core
has no language branch, and checks that this pack's five files are LF-only.
It prints one `[PASS]`/`[FAIL]` line per scenario, then `ADAPTER_SIM_OK` or
`ADAPTER_SIM_FAIL`. It runs no cargo and replaces no gate.

The archived `archive/simulate-pre.reconstructed.sh` is a labelled placeholder:
no pre-impl simulation script exists in the evidence for J21/J22.

All five files in this pack were checked with `tr -cd '\r' < file | wc -c`
== 0, using the single-quoted WSL script-file form — never `sed` — following
L10/L11 (`docs/ethics/lessons.md:202-252`).
