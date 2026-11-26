# Workflow — Test failure triage

## Why this exists

Three real failures in `.reports/goal-build-fnscope.md` had the same shape: the
test was wrong, not the code. Changing production behavior to make them pass
would have broken the locked contract.

- Iteration 4: `cargo test --workspace` -> "model 9/10 — `validate_aggregates_every_problem` expected `steps[0].id` but only faulted indices are reported." Ledger failure class: "test expectation disagrees with correct production behavior." Pivot: "sent exact assertion + real output back to J01; no production change requested." The same iteration records a wrong assertion in core, corrected by J04.
- Iteration 5: "`discover::tests::deterministic_order_and_duplicate_suppression` expected `[\"a\",\"b\"]` but got `[\"b\",\"a\",\"b\"]`." Ledger failure class: "test expectation contradicts the locked dedup key `(qualified_name, line)`."
- Iteration 11: "`scripts/test-all.sh` -> 101; new test failed with `Directory import ... fnscope-node-scalar-.../ is not supported`." Ledger failure class: "test's FnRecord omitted `file_path`, so the runner targeted a directory." Pivot: "J06 to set `file_path: \"shout.mjs\"` in the new test."

The brief also names a "call_line relative vs absolute" assertion; the ledge
records only "a wrong test assertion in core, corrected by J04" without that
detail. It is flagged in `docs/ethics/lessons.md`, not quoted as evidence.

**Rule.** On a failing test, decide whether CODE or TEST is wrong by reading
the real output and quoting the locked contract. Never bend production
behavior to satisfy a bad assertion. Never change both sides blindly.

## Decision tree

1. **Read the real output.** Copy the failing assertion, the left/right values,
   and the test name verbatim. Do not triage from memory or from a summary.
2. **Quote the locked contract.** Open `docs/contract/rust-api.md` (or the spec
   section the test claims to check) and quote the exact clause with its path
   and line. If the test has no clause behind it, say so.
3. **Decide which side is wrong.**
   - The assertion contradicts the contract -> TEST is wrong. Fix the test,
     and write one sentence in the job report explaining which clause the old
     assertion broke. Example: iteration 5's expected `["a","b"]` ignored the
     locked dedup key `(qualified_name, line)`.
   - The code contradicts the contract -> CODE is wrong. Fix the code in the
     owning crate only.
   - The contract itself is wrong or ambiguous -> stop. Record a drift entry o
     an `Awaiting decision` in `docs/drift.md` before editing either side.
4. **Change exactly one side.** Never "fix" the test and the implementation in
   the same pass. If you changed the test, the production diff must be empty.
5. **Re-run the smallest check.** Run only the failing test first. Then run its
   crate. Only when those are green, run `cargo test --workspace` once.
6. **Record the verdict.** Append the original output, the quoted clause, the
   decision, and the re-run result to the job report and the ledger.

## How to tell a setup bug from a logic bug

Iteration 11 is the model for this branch. The error text
`Directory import ... is not supported` did not describe a wrong expected
value; it described the runner being pointed at a directory. The cause was a
missing `file_path` in the test's `FnRecord`. Treat an error that names a
filesystem path or a missing field as a setup problem first: inspect the test
fixture before touching production logic.

## Checkable exit criteria

- The failing output is quoted verbatim in the report.
- A contract clause with `path:line` is quoted next to it.
- The report states `CODE wrong` or `TEST wrong` and why.
- Only one side changed.
- The smallest check passed, then the full suite passed once.

## DO NOT

- Do not relax an assertion just to get green.
- Do not change a schema or a crate boundary to accommodate a test.
- Do not run the full suite repeatedly while iterating on one test.
- Do not hide a red gate. A failed gate is recorded before the approach changes.
