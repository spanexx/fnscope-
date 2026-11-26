# IMPL — fnscope culture / reviewer workflow

RECONSTRUCTED 2026-11-26. This is the delivery plan and evidence index for
work that already landed in iterations 17, 20-23 and 44-46. It does not claim
that this file existed when those jobs ran. The contract is
`PRD-TRD-fnscope-culture-reviewers.md`; the decisions are in
`GRILL-fnscope-culture-reviewers.txt`.

## Phase J19 — turn real mistakes into durable culture

**Job.** Dispatched at iteration 17: convert nine real incidents into
ethics/concepts/workflows under delegated authority, after the user required
"manager behaviour — commit in batches and turn mistakes into durable culture,
not just git hygiene" (`.reports/goal-build-fnscope.md:190-196`).

**Scope delivered (as recorded).**

- `docs/ethics/lessons.md` — incident-to-rule index; the nine incidents that
  became L1-L9 (`lessons.md:17-25`). L10 (false green from `$"\r"`) and L11
  (format check mistaken for compile) were added later, after iteration 17's
  CRLF discovery and iteration 23's false PASS (`lessons.md:26-27,202-252`;
  ledger lines 238-244, where the ledger names "J20 writing L11").
- `docs/workflows/{compile-window,test-failure-triage,cross-environment-runner,boundary-contract-audit,reviewer-process,README}.md` — the checkable procedures.
- `docs/ethics/{boundary-discipline,verification-honesty,review-awareness}.md`
  and `docs/concepts/language-adapter.md` — role and architecture rules.
- `AGENTS.md` — Commit and worktree culture section.

**Evidence recorded at the time.**

- `git log --oneline` -> `4c26063 docs: turn build mistakes into enforceable ethics and workflows` on top of `ff793af docs: record verified base build and Rust addendum plan` (ledger line 192).
- Review of J19 found a CRLF regression in six copied ethics files; it was normalized and the commit amended, and the ledger records `CR_COUNT=0` (line 192). **Evidence limit:** that specific `CR_COUNT=0` is the false green later explained by L10; the real closure is the D-3 detector output quoted below, not this marker (`lessons.md:202-211`; `docs/drift.md:39-43`).
- Verdict at iteration 17: `pass (culture docs)`; failure class: line-ending regression during worktree merge (ledger lines 193-195).

**Exit.** Reviewed docs batch; no agent commit or merge; orchestrator owned the
gate.

## Phase J20 — reviewer process and the culture gate

**Job.** Dispatched at iteration 20 together with the adapter-registry job,
with the verbatim review notice and checklist in every brief
(`.reports/goal-build-fnscope.md:214-220`). Integrated and committed at
iteration 21 (lines 222-228).

**Scope delivered.**

- `scripts/check-culture.sh` — the mechanical gate and its `--self-test`.
- `docs/workflows/reviewer-process.md` — brief, independent reviewer, FAIL fix
  loop, merge rule, review report template and the 12-item checklist.
- D-3 closure in `docs/drift.md` and the L10 lesson in `docs/ethics/lessons.md`.
- L11 was written after iteration 23 exposed the false PASS
  (ledger line 243; `lessons.md:226-252`).
- Iteration 26 routed two L11-class truncated tokens in a culture doc back to
  J20 (ledger line 263; `.reports/reviews/J23-J24.md:60`), kept as findings.

**Evidence recorded at the time (verbatim markers).**

| Command | Marker / output | Source |
|---|---|---|
| `bash -n scripts/check-culture.sh` | exit 0 | ledger line 224 |
| `bash scripts/check-culture.sh --self-test` | `CULTURE_SELFTEST_OK` | ledger line 224 |
| `bash scripts/check-culture.sh` | eight `[PASS]` lines and `CULTURE_OK` | ledger line 224 |
| `git log --oneline` | `ec2e8b0 docs: enforce reviewed-workflow culture and prove the culture gate` | ledger line 224 |
| `grep -rlU $'\r' docs/ethics/*.md \| wc -l` | `0` with `grep_exit_status=1`; `tr -cd '\r'` -> `CR=0` for all ten files | `docs/drift.md:43` (D-3 `-> Resolved`) |

**Fix loop proof.** Iteration 21 records that D-3's status cell was `Open`
before the LF fix and `Resolved` after, with the real command/output quoted,
and that the orchestrator re-read `docs/drift.md` and
`scripts/check-culture.sh` before staging (ledger line 224). The original
false green was kept, not deleted (L10; D-3 note).

## Phase J46 — goal-skill delegated lane (user process correction)

**Job.** Iteration 44: the user corrected the process — no human gates, but
every decision documented at decision time with why and source, aligned with
the goal contract and prior decisions
(`.reports/goal-build-fnscope.md`, main copy, lines 403-409).

**Scope delivered (reviewed at line 405).**

- `feature-pipeline/SKILL.md:34-67` — "Goal-skill delegated lane".
- `surgical-change/SKILL.md:33-69` — "Goal-skill delegated lane".
- `goal/decisions.md:40-58,88,102-110` — Case B, Authority and the explicit
  recorded-waiver rule.

**Rules added:** decide and record at decision time with `[agent decision]`,
WHY and SOURCE; alignment with the Goal Contract and prior locked decisions;
human gates waived only by an explicit recorded waiver; isolation gate
unchanged (main ledger line 405).

**Evidence and limits.** The ledger records the reviewed file line ranges but
no commit hash for J46; the skill files live outside this repository and are
not a git repository, so no commit ref is invented here. Iteration 45 recorded
the reconstruction worktree and J47-J53; iteration 46 recorded the subagent
DNS outage and then quoted the waiver sentence verbatim at line 422. This
pack is a J53 reconstruction under that correction.

## Delivery notes — real markers and commit refs

| Phase | Marker / ref exactly as recorded | Where |
|---|---|---|
| J19 | `4c26063 docs: turn build mistakes into enforceable ethics and workflows` | ledger line 192 |
| J19 | `ff793af docs: record verified base build and Rust addendum plan` | ledger line 192 |
| J19 | `CR_COUNT=0` after the LF amend — superseded as proof by D-3's real detector | ledger line 192; `lessons.md:202-211` |
| J20 | `CULTURE_SELFTEST_OK` | ledger line 224 |
| J20 | eight `[PASS]` lines and `CULTURE_OK` | ledger line 224 |
| J20 | `ec2e8b0 docs: enforce reviewed-workflow culture and prove the culture gate` | ledger line 224 |
| D-3 | `grep -rlU $'\r'` -> `0`; ten files `CR=0` | `docs/drift.md:43` |
| Review loop | `.reports/reviews/J21.md` original `Verdict: PASS`, then `Verdict (re-review): PASS` with the original false PASS retained | `J21.md:2,79` |
| Review loop | `.reports/reviews/J23-J24.md` `Verdict: **FAIL**` with ten findings, no merge before fix | `J23-J24.md:3,49-60` |
| J46 | reviewed line ranges only; no commit ref exists in the ledger | main ledger line 405 |

**Current-state observation (2026-11-26, WSL Debian, this worktree).** Run by
this reconstruction agent, not by the original jobs and not by J54:

- `bash scripts/check-culture.sh --self-test` -> `CULTURE_SELFTEST_OK`.
- `bash scripts/check-culture.sh` -> the eight original checks plus
  `ORIGIN_GATE` and `DECISION_RECORD` `[PASS]` lines. J54 widened the glob from
  `GRILL-*.txt` to `GRILL*.txt` (`scripts/check-culture.sh:389-390`), so the gate
  now scans **7 GRILL files** — the legacy base `GRILL.txt` plus the six
  `GRILL-<slug>.txt` files — and J54's follow-up report records **7
  `DECISION_RECORD` PASS lines**, one per file. Both checks were added after
  iteration 21; the gate ends in `CULTURE_OK`. The total `[PASS]` line count
  therefore grows as other reconstruction packs land; the fixed iteration-21
  evidence is eight lines, not a current count.

These two commands were run in this worktree and their output is reproducible
with `simulate.sh`; that is the extent of the committed evidence here. J54's
negative probes for `ORIGIN_GATE` (missing pack) and `DECISION_RECORD` (missing
`Status: Locked` / WHY / SOURCE) were **reported in J54's job report and in the
orchestrator ledger, not committed as a script output**: the main ledger
records J54's gate-check scope at line 412 (iteration 45) and records at line
427 (iteration 47) that this pack's earlier "real commands with real output"
wording overstated committed probe output; J55 at
`.reports/reviews/pack-reconstruction.md:26` records that no negative-probe
output is committed in this worktree. No verdict is claimed for those probes
from a committed artifact. No cargo was run by this reconstruction job. Gate
results quoted from the ledger (`CARGO_TEST_STATUS`, `ADAPTER_GATE_STATUS`,
`ALL_TESTS_OK`) belong to the orchestrator and are not claimed here.

## Verification of this pack

- LF endings checked with `tr -cd '\r' < file | wc -c` -> `0` for every file in
  this pack, using a script-file/single-quoted form (never inline across two
  shells and never `sed`), per L10/L11.
- `bash scripts/check-culture.sh` and its self-test are executed by
  `simulate.sh`; the simulation asserts `CULTURE_OK` and
  `CULTURE_SELFTEST_OK` and prints `PASS` or `FAIL`.
- `archive/simulate-pre.reconstructed.sh` is a labelled design-time placeholder
  and is not run as evidence.

## Delivery note — J53 finalization (2026-11-26)

**Decision record.** The locked GRILL carries 17 `[agent decision]` entries
(D1-D17), each with DECISION / WHY / SOURCE / ALIGNMENT and a RECONSTRUCTED
label where the words are a reconstruction of earlier locked decisions, not a
contemporary transcript. D2-D12 map L1-L11 one-to-one and now carry an explicit
VALIDATION and REVIEWER CHECK line each, so every real incident shows the full
chain: incident -> rule -> validation -> reviewer check. D13-D15 cover reviewer
authority, the review notice in every brief and the checker self-test; D16
records the iteration-44 user correction that added the goal-skill delegated
lane to `feature-pipeline` and `surgical-change`; D17 prohibits invented quotes
and verdicts.

**Verification run after the final GRILL edit (WSL Debian, this worktree).**
`bash docs/features/fnscope-culture-reviewers/simulate.sh` -> the culture gate
printed `[PASS]` for CRLF, ABSOLUTE_PATH, PROCESS_PATTERN, DRIFT_STATUS,
DRIFT_NOTE, DOC_HEADING, CONTRACT_SYMBOL, SCRATCH, ORIGIN_GATE and **7
`DECISION_RECORD` `[PASS]` lines** — one per GRILL file under J54's widened
`GRILL*.txt` glob (`scripts/check-culture.sh:389-390`): the legacy base
`GRILL.txt` plus six `GRILL-<slug>.txt` files, as reported by J54 in its
follow-up — ending `CULTURE_OK`; the self-test printed
`CULTURE_SELFTEST_OK`; the script printed
`PASS: CULTURE_OK and CULTURE_SELFTEST_OK verified; no cargo was run`.
`tr -cd '\r' < file | wc -c` from a script file returned `0` for all five pack
files. No cargo, server, commit or merge was run by this reconstruction job.
