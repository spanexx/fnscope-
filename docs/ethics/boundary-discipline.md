# fnscope — Boundary Discipline

The build is split across agents. Boundaries are what keep the split from
turning into a mess.

## The rule

- Disjoint file ownership. Each job owns a named set of files. Edit only those.
- No cargo while peers write. `cargo build`, `cargo test` and `cargo check`
  belong to the orchestrator. An agent may run a read-only `rustfmt --check`
  on its own files but never a cargo gate during a write window.
- Agents never commit and never merge. Each implementation job gets its own
  git worktree outside the main tree. The orchestrator reviews the diff, runs
  gates, stages explicit paths, commits, then removes the worktree.
- Explicit staging. `git add <specific paths>`. Never `git add -A`. Never stage
  a file you have not read. A commit must not contain another job's
  half-written work.
- Review before merge. The checklist: locked contract names unchanged, JSON
  schema stable, `///` comments on public items, tests for new behavior, no
  hardcoded absolute paths, LF endings, drift log updated if a doc-vs-code gap
  was found.
- Report an out-of-scope concern; do not fix it. If a file belongs to anothe
  job, say what you saw and cite the line. Do not edit it.

These rules are already in `AGENTS.md` under "Parallel work rule" and "Commit
and worktree culture (orchestrator-owned)". This file explains why each one is
there, from the actual failures.

## The incidents

**Iteration 3 (L1).** Eleven jobs were written in parallel without compile
feedback, then the first compile produced:
`cargo check -p fnscope-model -p fnscope-parser -p fnscope-core -p fnscope-enrich -p fnscope-server` -> `CARGO_CHECK_STATUS=101`; 7 errors, all in `fnscope-parser`. The ledger names the cause: "delegated code written without compiler feedback (by design, to avoid parallel cargo contention)". The fix was to send diagnostics back to the owning agents (J02, J03), not to have the orchestrator rewrite their files. Bounded write windows and a compile window after each batch are the fix. See `docs/workflows/compile-window.md`.

**Iteration 4 (L2).** A wrong assertion in core and a wrong assertion in the
model were sent back to J04 and J01 respectively. The ledger pivot for the
model case: "sent exact assertion + real output back to J01; no production
change requested." Ownership held: the orchestrator did not silently edit
another job's test to make its own gate green.

**Iteration 12 (L7).** The ledger records "Integrated J06 runner arg mapping
and J08 UI fixes; formatted and re-ran the full suite and repo harness" and
then gates green. The brief also describes J08 escalating a concern about
runner code J06 had already fixed; that escalation is not in the ledge
iterations I read, so it is flagged in `docs/ethics/lessons.md` rather than
quoted here. The rule it illustrates is still checkable: before you escalate,
open the current code, quote the exact line that is still wrong, and name the
job that owns it. If the code is already fixed, withdraw the concern.

**Iteration 16 (polyglot expansion).** The contract update made the main agent
"orchestrator/reviewer/critic ONLY; all investigation, implementation,
refactoring and test-writing delegated to isolated subagents". The acceptance
criteria add that adding a language requires zero changes to core graph
builder, schema, enricher or UI. Boundary discipline is now also an
architecture requirement, not just a git habit.

## Reporting an out-of-scope concern

Use this shape in your job report or in `.reports/goal-build-fnscope.md`:

    FINDING: <one sentence: what looks wrong>
    LOCATION: <path:line, copied from the current file>
    OWNER: <job id from the decomposition, or "unowned">
    EVIDENCE: <exact command/output, or "source read only">
    ACTION TAKEN: none (out of scope)

Do not change the file. Do not call it a fix. If no job owns it, say so and let
the orchestrator decide whether it is a new job or a drift entry.

## Checkable procedure for the orchestrato

1. Before dispatch, write each job's owned paths into the job brief.
2. During the write window, confirm no two briefs name the same path.
3. After the batch, close writes and run the compile window
   (`docs/workflows/compile-window.md`).
4. Review the diff against the checklist above.
5. Stage only the named paths (`git add <path> ...`), read `git diff --cached`,
   then commit.
6. Append gate results — including failures — to the ledger before the next
   batch.

## DO NOT

- Do not run a gate while an agent is still writing.
- Do not edit a file you do not own to "help" a peer.
- Do not commit a file you have not read.
- Do not use `git add -A`, amend, or merge someone else's branch.
- Do not leave a finding implicit in a chat message; write it down.
