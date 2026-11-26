# fnscope — Review Awareness

You are told you will be reviewed **before** you start, not after you finish.
That is deliberate. It changes the work.

## Why up front

- **Clean diffs.** Knowing a second agent will read your diff, you keep to your
  owned files and stop tidying other people's code. Iteration 17's CRLF
  regression in six copied ethics files was caught in the merge review, not by
  the implementer; a reviewer who exists on paper only catches it late.
- **Smaller scope.** The brief names your exact paths and your checklist. You
  do not have to guess what "done" means, and you cannot drift into a neighbour
  job to make your own result green.
- **Honest reports.** You cannot claim a gate you did not run, because the
  reviewer checks every marker against a command and its output. A verdict
  without a command is not evidence.

The sentence in every brief is short: *an independent subagent with no
implementation authority will audit your work; PASS is required before merge.*
The full loop is `docs/workflows/reviewer-process.md`.

## What "clean workspace" means, concretely

- Only the files in your brief are changed. `git status --short` shows nothing
  else that you created.
- LF endings. No CR bytes in text files.
- No temp files, logs, scratch scripts, runner directories or caches left in
  the repo. Scratch work goes outside the repo, under the system temp folder.
- Every server, watcher or runner you started is stopped, its port is closed,
  and its PID was tracked rather than guessed.
- No commits, no merges, no staging. The orchestrator owns those.
- Your report states what you ran and what you did not run; unrun checks say
  `PENDING`.

If you cannot clean something up, say what it is, where it lives and why. An
honest leftover is safer than a silent one (`docs/ethics/cleanup.md`).

## Hiding a failure is worse than reporting it

A reported failure costs one iteration. A hidden failure costs trust in every
later report from that job.

- **L1 (iteration 3).** Seven compiler errors surfaced because the failures
  were real and recorded. Reporting them sent exact diagnostics to J02 and J03
  and produced the compile-window rule. Pretending the parallel write window
  was fine would have shipped broken code.
- **L6 (iteration 13).** A drift row's status cell said `Open` while its note
  said `Resolved`. The prose looked finished; the machine-readable cell was a
  lie. Reporting the cell, not the paragraph, is what closed D-2.
- **L7 (iteration 12).** A concern about runner code was raised after the fix
  had already landed. The correct move was to open the current line, see the
  fix, and withdraw the concern in writing. Escalating stale work is a report
  that lies about the present state.

The rule is one line: **a failed gate is recorded before the approach changes**
(`AGENTS.md`). Paste the real command and output into
`.reports/goal-build-fnscope.md`; do not paraphrase it into a pass.

## DO NOT

- Do not soften `FAIL` into `PARTIAL` to keep a summary clean.
- Do not copy a marker from an earlier iteration into a new one.
- Do not edit a file you do not own to make a finding disappear.
- Do not delete a review finding from a report to make the verdict look better.
