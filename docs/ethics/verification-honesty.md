# fnscope — Verification Honesty

A verdict is a claim about a command you actually ran. If you did not run it,
you do not have that verdict.

## The rule

- Never claim a gate you did not run.
- Never invent a verdict marker such as `ALL_TESTS_OK`, `FMT_STATUS=0`,
  `CARGO_TEST_STATUS=0`, `PASS`, or `SHIPPED`.
- Leave `PENDING` when the check is pending. `PENDING` is an honest state, not
  a failure to hide.
- Record the real command and the real output, verbatim, with the lane and the
  date. A paraphrase of a gate is not evidence.
- Say what a result does **not** prove. A green unit suite is not a browse
  audit; a schema-valid response is not a rendered UI behavior.

## The model: J18's `VERIFY.md`

The brief attributes this file to J18. The ledger lists J18 as the docs job but
does not name the file's author, so the attribution is recorded, not proven.

`docs/features/fnscope-base/VERIFY.md` is the file to copy. Its header says:
"This file reports results; it does not restate the requirements (PRD-TRD.md)
or the plan (IMPL.md)." It then gives a table with three columns — what was
checked, the exact command, and the marker/result. Examples recorded there:

- `cargo test --workspace`, `FNSCOPE_NODE` set to native Node 24 -> `CARGO_TEST_STATUS=0`; 72 tests, 0 failures.
- `cargo fmt --all -- --check` -> `FMT_STATUS=0`.
- `bash scripts/test-all.sh` -> `ALL_TESTS_OK`, with `MANIFEST_OK: 9 functions, 8 call edges` and `SERVER_SMOKE_OK`.
- `bash scripts/enrich-e2e.sh` -> `ENRICH_E2E_OK`: 9 first-run calls, 0 unchanged, 0 formatting-only, new call on a semantic change.

Two parts make it trustworthy. First, "Known limits (do not read these as
passes)" lists what the green result does not cover — Node invocation is best
effort, constructors are listed as functions, cross-file type resolution is
bounded. Second, the "Rust addendum (pending)" section ends with: "Nothing in
this section is a recorded result: every verdict is `PENDING` until the
orchestrator replaces it with evidence." The rows under it say `PENDING` in
the verdict column. That is the correct way to record unrun work.

## The incidents behind this rule

**Iteration 13 (D-2 status, L6).** The ledger records "drift D-1 + D-2
`Resolved` (J10 caught and fixed a stale row status)." The D-2 row's status
cell said `Open` while its note said `Resolved`. The machine-readable cell was
wrong even though the prose sounded finished. Read cells, not paragraphs.

**Iteration 10 (green suite, broken UI, L4/L5).** The ledger records
"`cargo test --workspace` still 0 with 71+ tests" in the same iteration that
the browser audit returned "7 PASS, 1 FAIL, 1 PARTIAL" and found three real
bugs. Reporting only the green suite would have been an honest number attached
to a false conclusion.

**Iteration 13/14 (empty `trace`, L8).** The re-audit wrote "PASS 6, FAIL 0"
and then, in "Remaining minor observations (not FAILs, no new bug requiring
delegation)", noted "Node-mode responses carry `"trace":{}` (empty)". Both
statements are kept. A minor gap is still recorded, not silently promoted to a
pass or hidden to keep the summary clean.

## Checkable procedure

1. Write down the exact command before you run it. Include the lane (WSL
   Debian / Windows / browser) and any environment override such as
   `FNSCOPE_NODE`.
2. Run it. Copy the output verbatim into the report. Keep the marker exactly as
   printed.
3. If you did not run it, write `PENDING` and name who will run it. Do not copy
   a marker from an earlier iteration into a new one.
4. Add a "known limits" line for anything the check does not cover.
5. Before you report done, re-read your own verdict table and confirm every
   marker has a command next to it that you personally ran.
6. When a gate fails, append the command and output to
   `.reports/goal-build-fnscope.md` before changing approach (AGENTS.md).

## DO NOT

- Do not write `SHIPPED`, `pass`, or a status marker for work still in flight.
- Do not turn a bounded observation into a full pass because it is "only
  cosmetic".
- Do not quote a result from memory. Open the file and copy it.
- Do not claim a browser check from a source review, or a real Node run from a
  simulated trace. The browser audit explicitly distinguishes the initial
  `{"simulated": true}` placeholder from a post-Run response, and reports that
  no post-Run response contained it.
