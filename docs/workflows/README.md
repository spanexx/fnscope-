# fnscope — Workflows

Five short procedures for the failures that actually happened during the
build. Each one names its incident, states the rule, and ends with a checkable
procedure. Use the smallest one that fits; do not run all five by default.

| Workflow | Use it when | Lesson |
|---|---|---|
| `compile-window.md` | An agent batch finished writing and you need to know whether the workspace still compiles | L1 |
| `test-failure-triage.md` | A test is red and you must decide whether code or test is wrong | L2 |
| `cross-environment-runner.md` | A process is launched from one OS into another, or a generated file crosses WSL/Windows | L3 |
| `boundary-contract-audit.md` | Data crosses a boundary (manifest, form, POST, runner, export), or a UI feature must be proven as rendered | L4, L5, L8 |
| `reviewer-process.md` | Every implementation job: self-contained brief, independent review, FAIL fix loop, then merge only on PASS | L1, L6, L7, iteration 17 |

`scripts/check-culture.sh` is the machine gate for the rules these procedures
share. Run it after staging; it prints `CULTURE_OK` only when every mechanical
check passes. Run `bash scripts/check-culture.sh --self-test` before trusting
that green: it proves the CRLF detector against a known CRLF sample and a known
LF sample and prints `CULTURE_SELFTEST_OK` (L10). A detector that goes green on
its positive control is not evidence.

## What is not here

- Commit and staging rules: `AGENTS.md` and `docs/ethics/boundary-discipline.md`.
- What to write in a report: `docs/ethics/verification-honesty.md` and
  `docs/features/fnscope-base/VERIFY.md`.
- How a language adapter is structured: `docs/concepts/language-adapter.md`.
- The incident-to-rule index: `docs/ethics/lessons.md`.

## Order of use in a normal batch

1. Each agent gets a brief with exact owned paths, the independent-review
   notice and the checklist (`reviewer-process.md`, Phase 1).
2. Agents write files in disjoint sets. No cargo during this window.
3. Orchestrator closes the window and runs `compile-window.md`.
4. If a test fails, run `test-failure-triage.md` and send the exact output back
   to the owning agent.
5. If a change touches a process or payload crossing a boundary, run
   `cross-environment-runner.md` and/or `boundary-contract-audit.md`.
6. The independent reviewer audits the diff (`reviewer-process.md`, Phase 2).
   A FAIL goes back with exact lines; merge only on PASS.
7. Record every result, review verdict included, in
   `.reports/goal-build-fnscope.md`, including failures.

A check that passed is not re-run. A verdict not recorded is not claimed.
