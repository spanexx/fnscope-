# Workflow — Reviewer process (orchestrator-owned)

## Why this exists

Four real incidents had the same ending: a mistake was noticed, a lesson was
written down, and the next job still walked into it.

- Iteration 3 (L1): seven compiler errors from parallel writes with no compile
  window. The lesson existed in words; no brief carried it to the next agent.
- Iteration 13 (L6): a drift row's status cell said `Open` while its note said
  `Resolved`. A prose claim was accepted as a machine-readable fact.
- Iteration 12 (L7): a concern was escalated after the fix had already landed,
  because nobody re-read the current line before reporting.
- Iteration 17: a CRLF regression in six copied ethics files was caught only
  during a worktree merge review, not by the implementer.

**Rule.** A mistake becomes culture only when three things exist: the rule is
stated in the implementer's brief, a machine check enforces it
(`scripts/check-culture.sh`), and an independent reviewer audits the diff before
merge. No job merges on the implementer's word.

This workflow is the loop the orchestrator runs for **every** implementation
job. The orchestrator owns cargo, the full gate, the merge and the commit. The
reviewer owns neither implementation nor gate execution.

## Phase 1 — The implementer brief

The brief must be self-contained. The implementer does not have to guess which
files it owns or whether anyone will look at its work.

1. **Exact owned paths.** List them as explicit paths. Say that any other file
   is out of scope and must be reported as a finding, not edited. If the job
   runs in a worktree, name it.
2. **Independent review notice.** Use this sentence verbatim or near-verbatim:

   > You will be reviewed by an independent subagent that has no implementation
   > authority. It reads your diff and your owned files; it does not edit them,
   > and it does not run the full gate. PASS is required before merge.

3. **The review checklist** from Phase 2, pasted into the brief. An
   unstated checklist item is not enforceable against the implementer.
4. **Clean workspace** (see `docs/ethics/review-awareness.md`): only owned
   files changed, LF endings, no temp files, no logs, no scratch processes, no
   commits, no merges.
5. **No cargo.** The orchestrator runs all gates. The implementer writes files
   and reports; it does not run `cargo build`, `cargo test` or `cargo check`.

The brief also names the job id, the contract section it must satisfy, and the
evidence it must quote if it claims a check passed.

## Phase 2 — The independent reviewer

One subagent, with **no implementation authority**, audits one job. It may be
the same model as the implementer, never the same role in that job.

- Read the owned files and the diff. Do not write to them.
- Do not run cargo, the full gate, or any script that starts a service. A review
  is a source-and-diff audit; the orchestrator runs gates separately.
- Check every item in the checklist below. Each finding must name an exact
  `path:line` in the current file. A finding with no line is a comment, not a
  finding.
- Write `.reports/reviews/<job-id>.md` with `PASS` or `FAIL`. One report per
  job, one verdict per report.
- Report what the review did **not** check under "Known limits".

### Review checklist

1. Locked contract names and JSON schema unchanged (`docs/contract/rust-api.md`).
2. File ownership respected. No file outside the brief's list was touched.
3. Public items carry a one-line `///` comment that says *why*.
4. New behaviour has tests, beside the code in `#[cfg(test)]`.
5. No hardcoded drive letters, user directories or absolute paths; a waiver
   marker must carry a reason.
6. LF endings. No CR bytes in text files.
7. Scripts use bounded waits and tracked PIDs, never `pkill -f`, `nohup` or
   `setsid` without a reasoned waiver.
8. No invented verdict markers. Every marker in a report has a command and real
   output next to it; pending work says `PENDING`.
9. No scope drift. An out-of-scope observation is a finding with an owner, not
   an edit.
10. Every claim is backed by a command/output or by a quoted line. "Looks
    right" is not evidence.
11. Drift: any doc-vs-code gap found was written to `docs/drift.md` before the
    edit, and its status cell agrees with its note (L6).

### Review report template

    # Review: <job-id>

    - Verdict: PASS | FAIL
    - Reviewer: <subagent id> (no implementation authority)
    - Date: YYYY-MM-DD
    - Scope: <owned files read; diff range>
    - Evidence: <commands/outputs read, or "source and diff read only">

    ## Findings

    | # | file:line | checklist item | Finding | Required fix |
    |---|---|---|---|---|
    | 1 | `crates/foo/src/bar.rs:42` | 3 | public `fn baz` has no `///` | add one line saying why |

    Write `None` on its own line when there are no findings.

    ## Known limits

    - <what this review did not check, e.g. "no cargo run; gate owned by orchestrator">

A report that says `PASS` with no Scope, no Evidence and no Known limits is not
a review. It is a mood.

## Phase 3 — The fix loop

1. **FAIL goes back with lines.** The orchestrator sends the implementer the
   exact `file:line` findings, the checklist item, and the required fix. It does
   not fix the file itself.
2. **The implementer fixes only those findings.** No opportunistic cleanup, no
   edits to another job's file, no new scope. If a finding is disputed, quote
   the current line and the contract clause in the reply.
3. **The reviewer re-audits** only the changed files and the previous findings,
   then updates the same `.reports/reviews/<job-id>.md` with a new verdict. Keep
   prior findings in the report with `fixed` noted; do not delete them.
4. **Repeat until PASS.** A job does not merge with an open FAIL.
5. **Record the loop** in `.reports/goal-build-fnscope.md`: job id, each
   PASS/FAIL, the exact finding or command/output, and the iteration count.
   A failed review is recorded, not hidden, before the next approach.

## Phase 4 — Merge

Only the orchestrator merges, and only after the review report says PASS.

1. Re-read `AGENTS.md` and confirm the review checklist against the final diff.
2. Run the scope-appropriate gates and the full gate; append the real commands
   and markers to the ledger. A PASS review is not a gate result.
3. Stage explicit paths (`git add <specific paths>`), never `git add -A`, and
   read `git diff --cached` before committing.
4. One logical change, one commit, conventional prefix (`feat`, `fix`, `docs`,
   `test`, `refactor`, `chore`). The subject says what changed.
5. Remove the job's worktree after the commit. A leftover worktree is cleanup
   debt, not evidence.
6. Append the commit hash and gate verdicts to the ledger.

## Checkable exit criteria

- The job brief named owned paths, the independent-review sentence and the
  checklist before any file was written.
- `.reports/reviews/<job-id>.md` exists and ends in PASS.
- Every FAIL was sent back with `file:line` and was re-audited.
- The ledger records each review verdict and each gate command/output.
- The commit contains only the named paths and one logical change.

## DO NOT

- Do not let an implementer review its own job.
- Do not let a reviewer edit code or run the full gate.
- Do not merge on an implementer's self-report.
- Do not write a finding without a `path:line`.
- Do not delete a prior FAIL from a review report to make the final verdict
  look clean.
- Do not turn a known limit into a pass because it is "only cosmetic".
