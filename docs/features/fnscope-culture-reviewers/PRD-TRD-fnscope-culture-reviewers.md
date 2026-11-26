# PRD-TRD — fnscope culture / reviewer workflow

RECONSTRUCTED 2026-11-26 from durable repo evidence. This document restates
the contract that the culture and reviewer workflow already implements; it is
not a new design and it is not a contemporary transcript. Decisions are locked
in `GRILL-fnscope-culture-reviewers.txt`.

## 1. Why this exists

Four incidents had the same ending: a mistake was noticed, a lesson was
written down, and the next job still walked into it
(`docs/workflows/reviewer-process.md:5-15`). Iteration 3 produced seven
compiler errors from parallel writes with no compile window; iteration 13 had
a drift status cell reading `Open` while its note said `Resolved`; iteration
12 escalated a concern after the fix had landed; iteration 17's CRLF
regression was caught only during a worktree merge review. Iterations 23 and
26 added two more: a reviewer PASS on code that did not compile, and a review
FAIL with ten findings before merge.

The user's process correction at iteration 44 closed the loop: no human gates,
but every decision documented at decision time with why and source, aligned
with the goal contract and prior decisions
(`.reports/goal-build-fnscope.md`, main copy, lines 403-409). The verbatim
waiver sentence is quoted in the GRILL header from line 422 of the same ledger.
Waiver of approval is not waiver of evidence.

**Product outcome.** A mistake that has actually happened becomes (1) a rule
in the implementer's brief, (2) a mechanical check, and (3) a reviewer item —
before the next job starts. That is the three-part rule at
`docs/workflows/reviewer-process.md:17-20`.

## 2. What it does — behavioral spec of the review loop

### 2.1 Phase 1 — the implementer brief

The orchestrator writes a self-contained brief before any file is written
(`docs/workflows/reviewer-process.md:26-49`):

1. **Exact owned paths**, with every other file declared out of scope and
   reportable as a finding, not editable. If a worktree is used, it is named.
2. **Independent review notice**, verbatim or near-verbatim:

   > You will be reviewed by an independent subagent that has no implementation
   > authority. It reads your diff and your owned files; it does not edit them,
   > and it does not run the full gate. PASS is required before merge.

3. **The review checklist** from Phase 2, pasted into the brief. An unstated
   item is not enforceable.
4. **A clean workspace**: only owned files, LF endings, no temp files, logs,
   scratch processes, commits or merges.
5. **No cargo.** The implementer writes files and reports; the orchestrator
   runs every gate.

The brief also names the job id, the contract section to satisfy and the
evidence the implementer must quote for any claim that a check passed.

### 2.2 Phase 2 — the independent reviewer

One subagent, with **no implementation authority**, audits one job
(`docs/workflows/reviewer-process.md:51-65`):

- Read the owned files and the diff; do not write to them.
- Do not run cargo, the full gate or any script that starts a service.
- Check every item below. Each finding names an exact `path:line` in the
  current file; a finding with no line is a comment.
- Write `.reports/reviews/<job-id>.md` with one verdict (`PASS` or `FAIL`) and
  a "Known limits" section naming what was not checked.
- Report no-findings as `None` on its own line, not as an empty section.

Checklist (`docs/workflows/reviewer-process.md:66-97`):

1. Locked contract names and JSON schema unchanged.
2. File ownership respected; no file outside the brief was touched.
3. Public items carry a one-line `///` comment saying *why*.
4. New behaviour has tests beside the code in `#[cfg(test)]`.
5. No hardcoded drive letters, user directories or absolute paths; a waiver
   marker must carry a reason.
6. LF endings; no CR bytes in text files.
7. Bounded waits and tracked PIDs; no `pkill -f`, `nohup` or `setsid` without a
   reasoned waiver.
8. No invented verdict markers; each marker has a command and real output next
   to it; pending work says `PENDING`.
9. No scope drift; an out-of-scope observation is a finding with an owner.
10. Every claim is backed by a command/output or a quoted line.
11. Drift: any doc-vs-code gap was written to `docs/drift.md` before the edit,
    and its status cell agrees with its note (L6).
12. Semantic sanity: no truncated or undefined identifiers; formatted code is
    not proven-correct code; the orchestrator's compile gate is the only
    compile evidence (L11).

A `PASS` means "no defect found by source audit", not "compiles". If a later
gate proves it false, the re-review records the miss as a finding and marks
the prior verdict superseded; the old verdict is not deleted
(`docs/workflows/reviewer-process.md:89-97`; `.reports/reviews/J21.md:38-79`).

### 2.3 Phase 3 — the fix loop

A `FAIL` goes back to the implementer with the exact `file:line`, the checklist
item and the required fix. The implementer changes only those findings; an
out-of-scope concern is reported with an owner, not edited. The reviewer
re-audits only the changed files and prior findings, keeps earlier findings in
the report noted as `fixed`, and issues a new verdict in the same report. The
loop repeats until `PASS`; a job never merges with an open `FAIL`. Every
verdict is recorded in the ledger with the exact finding or command/output
(`docs/workflows/reviewer-process.md:124-138`).

### 2.4 Phase 4 — merge

Only the orchestrator merges, and only after the report says `PASS`: re-read
the rules, run the scope-appropriate gates and the full gate, stage explicit
paths, read `git diff --cached`, commit one logical change with a conventional
prefix, remove the job's worktree, and append the commit hash and gate
verdicts to the ledger (`docs/workflows/reviewer-process.md:140-153`).

### 2.5 The mechanical gate

`scripts/check-culture.sh` enforces the mechanical subset: CR bytes, absolute
paths, process patterns, drift status/note agreement, doc headings, contract
symbols, scratch files, crate-origin documentation and decision records. It
prints one `[PASS]`/`[FAIL]` line per check, then `CULTURE_OK` or
`CULTURE_FAIL: <reason>`, and exits 0 or 1. `--self-test` proves the CRLF
detector against a known positive and negative and prints
`CULTURE_SELFTEST_OK` (`scripts/check-culture.sh:1-4,56-60,257-295,301-314`).

## 3. Simulation contract

The canonical post-impl simulation is `docs/features/fnscope-culture-reviewers/
simulate.sh`. It must:

1. Run `bash scripts/check-culture.sh` from the repository root it resolves
   relative to its own location — no hardcoded drive letter or user path.
2. Run `bash scripts/check-culture.sh --self-test`.
3. Capture both outputs verbatim and echo them, so the markers are traceable.
4. Assert `CULTURE_OK` as an exact line in the first output and that no
   `CULTURE_FAIL` line is present; assert `CULTURE_SELFTEST_OK` as an exact line
   in the second output and that no `CULTURE_SELFTEST_FAIL` line is present;
   check both process exit statuses.
5. Print `PASS: ...` only when all assertions hold, otherwise print
   `FAIL: ...` and exit non-zero.
6. Run **no cargo**, start no service, write no file into the repository and
   clean up its own temp directory by tracked path.

`archive/simulate-pre.reconstructed.sh` is a labelled design-time placeholder:
it states the intended operator flow but runs no gate. It is not evidence.
LF endings are verified with `tr -cd '\r' < file | wc -c` equal to 0 using a
script file (never an inline check across two shells, per L10).

## 4. Technical design

### 4.1 Workflow layer

Five short procedures, each with an incident, a rule and a checkable procedure
(`docs/workflows/README.md:3-13`):

| Workflow | Use when | Lesson |
|---|---|---|
| `compile-window.md` | an agent batch finished writing and the workspace must still compile | L1 |
| `test-failure-triage.md` | a test is red and code vs test must be decided | L2 |
| `cross-environment-runner.md` | a process or generated file crosses WSL/Windows | L3 |
| `boundary-contract-audit.md` | data crosses manifest/form/POST/runner/export, or UI behavior must be proven as rendered | L4, L5, L8 |
| `reviewer-process.md` | every implementation job | L1, L6, L7, L11, iteration 17 |

The incident-to-rule index is `docs/ethics/lessons.md` (L1-L11). The role
ethics are `docs/ethics/boundary-discipline.md` (disjoint ownership, no cargo
while peers write, no agent commits or merges, explicit staging),
`docs/ethics/verification-honesty.md` (no invented verdicts, `PENDING` for
unrun work) and `docs/ethics/review-awareness.md` (notice before the work,
clean workspace, report failures).

### 4.2 Checker layer

`scripts/check-culture.sh` sources `repo-root.sh`, collects tracked plus
untracked non-ignored text files, and runs these checks:

- `check_crlf` — shared `grep -rlU $'\r'` detector (L10).
- `check_absolute_paths` — forbidden drive/user/home shapes with explicit
  quote-evidence exceptions and `culture-allow:` waivers.
- `check_process_patterns` — forbidden `pkill -f`/`nohup`/`setsid` with a
  written explanation on a waiver.
- `check_drift` — status cells in `Open|Awaiting decision|Resolved` and every
  `-> Resolved` note matching a `Resolved` row (L6).
- `check_doc_headings` — ethics/workflows/concepts docs non-empty with a `# `
  heading.
- `check_contract_symbols` — key locked symbols present in crate sources.
- `check_scratch_files` — no `fnscope-node-*`, `fnscope-invoke-*` or `*.log`
  files in the repo.
- `check_origin_gate` — any crate changed relative to `main` must have a
  GRILL/PRD-TRD/IMPL pack (allow-listed base crates keep legacy filenames).
- `check_decision_record` — every `GRILL-<slug>.txt` has `Status: Locked` and
  a `[agent decision]` backed by `WHY`/`SOURCE`.
- `self_test` — known CRLF/LF positive and negative controls, temp directory
  removed before the marker.

The checker is deliberately mechanical: no cargo, no network, no service, no
long waits. Semantic correctness stays with the orchestrator's compile gate
and the reviewer's source audit (L11).

### 4.3 Decision lane

When the active goal contract says `Authority: delegated` and the user told
the agent to make all decisions, `feature-pipeline` and `surgical-change` use
their "Goal-skill delegated lane" sections
(`feature-pipeline/SKILL.md:34-67`, `surgical-change/SKILL.md:33-69`):
decide and record at decision time, check alignment with the Goal Contract and
prior locked decisions, waive human gates only when explicitly recorded, and
keep the worktree isolation gate unchanged. This pack was reconstructed under
that correction (iteration 44).

## 5. Non-goals

- No new behavior in the Rust engine, CLI, server, UI or adapters; their
  contracts stay locked at `docs/contract/rust-api.md` and
  `docs/contract/adapter-api.md`.
- No agent runs cargo, merges, commits or reviews its own job.
- No reviewer edits a file or runs the full gate; a review is a source-and-diff
  audit.
- No invented verdict marker, no copied marker from an earlier iteration, and
  no promotion of a minor observation into a pass.
- No silent reinterpretation of a locked decision and no deleted false PASS.
- No claim that a format check proves compilation (L11) or that a check that
  never saw a CR byte proves LF (L10).
- No change to the worktree isolation rule or the explicit-staging rule.
- `archive/simulate-pre.reconstructed.sh` is not a runnable gate and is not
  evidence of a design-time user sign-off.
