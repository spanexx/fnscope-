# Model scorecard

Evidence-based routing for the orchestrator. The scorecard answers: **which model, for which
task, at which variant, and why**.

Status: **no observed subagent runs yet.** Every "prior" below is a hypothesis, not a grade.
The first calibration wave replaces priors with evidence.

## Rubric

Grade each dimension 1–5 after every child session:

| Dimension | 5 | 3 | 1 |
|---|---|---|---|
| Correctness | Solved it; no rework | Solved with review fixes | Failed or unsafe |
| Scope / contract | Stayed inside owned paths and contract | Minor drift caught in review | Scope drift or contract break |
| Tool use | Read, edited and tested cleanly | Needed guidance | Wrong tools or repeated mistakes |
| Test quality | Meaningful tests, gate evidence | Basic tests | No tests or self-reported only |
| Communication | Terse, exact, actionable | Acceptable | Vague or misleading |
| Efficiency / reliability | Low steps, no rate limits | Normal | Timed out, throttled or looped |

Confidence:

- **Low** — one run, or a task unlike the next one.
- **Medium** — two consistent runs on the same task type.
- **High** — repeated runs across jobs and repos.

Rules:

- Grade **task fit**, not general intelligence.
- Do not blame the model for a vague brief. Fix the brief first.
- Two failures on a task type → mark the model **avoid** for that type and route to the backup.
- One success does not make a model the default. Require repeat evidence.
- The orchestrator grades workers. The critic grades the orchestrator's decisions; the
  orchestrator does not grade itself.
- A reviewer PASS is a source audit, not compile evidence unless the reviewer ran the gate.

## Current routing

Initial defaults only. Update this table after calibration; do not append a second table.

| Task type | Primary | Backup | Avoid until proven | Evidence |
|---|---|---|---|---|
| Rust implementation | `opencode/deepseek-v4.1-flash-free#high` | `…#max` | `hy4-flash-free` | none |
| TypeScript / UI implementation | `opencode/deepseek-v4.1-flash-free#high` | `mimo-v2.6-flash-free` | `hy4-flash-free` | none |
| Research / deep scouting | `opencode/deepseek-v4.1-flash-free#max` | `hy4-flash-free#high` | — | user-routed 2026-11-26 |
| Code review | `opencode/deepseek-v4.1-flash-free#max` | `mimo-v2.6-flash-free` | `hy4-flash-free` | none |
| Process critique | `opencode/mimo-v2.6-flash-free` | `hy4-flash-free#high` | — | none |
| Docs / mechanical | `opencode/deepseek-v4.1-flash-free#low` | `mimo-v2.6-flash-free` | — | none |
| Architecture / contract | orchestrator + `…#max` reviewer | — | free-only worker | none |
| Integration / conflict | orchestrator | `…#max` | — | none |

Unlisted free models may be used as **calibration candidates**. Do not route production work to
them until they appear in this table with evidence.

## Models

### DeepSeek V4.1 Flash Free

- IDs: `opencode/deepseek-v4.1-flash-free` with `#low`, `#high`, `#max`.
- Cost: free in the current catalog.
- Prior strengths: reasoning, code reading, tool use, test writing.
- Prior weaknesses: can be verbose; free-tier rate limits unknown; `#max` is slower and
  token-heavier.
- Observed: 3/5 deep scouts succeeded; 2 hit the free usage limit when five `#max` scouts ran
  concurrently. Quality of completed briefs was high; the failure was quota/concurrency, not
  task complexity.
- Best at (prior): deep scouting/context gathering, Rust/TS implementation, code review,
  contract reading.
- Routing note: cap concurrent `#max` scouts at 1–2; use Hy4/MiMo for broad reads and DeepSeek
  `#max` for synthesis and high-risk review.
- Avoid (prior): final architecture decisions without a second model; high-stakes contract
  changes alone.
- Grades:

| Date | Job | Task type | Correctness | Scope | Tools | Tests | Comms | Efficiency | Confidence | Notes |
|---|---|---|---|---|---|---|---|---|---|---|
| — | — | — | — | — | — | — | — | — | — | pending |

### Hy4 Flash Free

- ID: `opencode/hy4-flash-free` with `#none`, `#high`.
- Cost: free.
- Prior strengths: speed, search, summarization, breadth-first exploration.
- Prior weaknesses: less proven on deep Rust and subtle contract work; may over-summarize.
- Observed: none.
- Best at (prior): light scouting, fast file maps, short briefs, docs lookup.
- Avoid (prior): deep context gathering, Rust implementation, contract changes, final review.
- Routing note: DeepSeek `#max` is the deep scout; Hy4 is the fast/backup scout.
- Grades:

| Date | Job | Task type | Correctness | Scope | Tools | Tests | Comms | Efficiency | Confidence | Notes |
|---|---|---|---|---|---|---|---|---|---|---|
| — | — | — | — | — | — | — | — | — | — | pending |

### MiMo V2.6 Flash Free

- ID: `opencode/mimo-v2.6-flash-free` (no variants).
- Cost: free.
- Prior strengths: multimodal input, fast summarization, a different model family from DeepSeek.
- Prior weaknesses: less proven on deep code correctness; may be less precise on Rust.
- Observed: one critic run hit the free usage limit before producing a review.
- Best at (prior): process critique, docs, UI copy, second-opinion reading.
- Reliability note: use Hy4 `#high` as the critic fallback when the free-tier cap is active.
- Avoid (prior): sole reviewer on high-risk code; deep Rust implementation.
- Grades:

| Date | Job | Task type | Correctness | Scope | Tools | Tests | Comms | Efficiency | Confidence | Notes |
|---|---|---|---|---|---|---|---|---|---|---|
| — | — | — | — | — | — | — | — | — | — | pending |

### Anthropic Claude Haiku (pending)

- Status: **not in the current catalog.** Connect Anthropic before routing to it.
- Cost: paid; do not assume it is cheap just because it is smaller.
- Intended use: scout and light implementation, if it grades better than the free roster.
- Do not route until it appears in `opencode models` and has calibration evidence.

### Orchestrator (user-selected)

- The user selects the orchestrator model; the skill does not override it.
- Use it for planning, final gate, conflict resolution and high-risk review.
- The critic grades the orchestrator's decisions, not the model in isolation.
- Track quota pressure. If the orchestrator is paid, keep bulk work on the free roster.

## Evidence log

Append one row per completed child. Link the job/commit/review where possible.

| Date | Job | Agent | Model | Task type | Correctness | Scope | Tools | Tests | Comms | Efficiency | Verdict | Notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 2026-11-26 | S1 UI architecture | scout | deepseek-v4.1-flash-free#max | deep research | — | — | — | — | — | 1 | fail | Free usage limit reached before a brief was produced; relaunched on Hy4 `#high`. |
| 2026-11-26 | S2 server/scopes | scout | deepseek-v4.1-flash-free#max | deep research | 5 | 5 | 5 | n/a | 5 | 4 | pass | Detailed, file:line, additive API, contract escalation identified. |
| 2026-11-26 | S3 wireframe acceptance | scout | deepseek-v4.1-flash-free#max | deep research | 5 | 5 | 5 | n/a | 5 | 4 | pass | Complete interaction inventory and gap list. |
| 2026-11-26 | S4 process/gates | scout | deepseek-v4.1-flash-free#max | deep research | — | — | — | — | — | 1 | fail | Empty report, then free usage limit; relaunched on Hy4 `#high`. |
| 2026-11-26 | S5 product quality | scout | deepseek-v4.1-flash-free#max | deep research | 5 | 5 | 5 | n/a | 5 | 4 | pass | Strong public-product/security extension brief; scope discipline noted. |
| 2026-11-26 | S1 UI fallback | scout | hy4-flash-free#high | deep research | 4 | 4 | 5 | n/a | 5 | 5 | pass | Useful module split; its client-side scope shortcut was rejected by the server evidence. |
| 2026-11-26 | S4 process fallback | scout | hy4-flash-free#high | deep research | 5 | 5 | 5 | n/a | 5 | 5 | pass | Exact gate/guardrail findings; used in Phase 0. |
| 2026-11-26 | pack critic | critic | mimo-v2.6-flash-free | process review | — | — | — | — | — | 1 | fail | Free usage limit; relaunched on Hy4. |
| 2026-11-26 | pack critic follow-up | critic | hy4-flash-free#high | process review | 5 | 5 | 5 | n/a | 5 | 4 | pass | Confirmed all blocker/high fixes; `NO_REMAINING_BLOCKERS`; first pass hit max steps. |
| 2026-11-26 | J1 server | impl | deepseek-v4.1-flash-free#high | server implementation | — | — | — | — | — | 1 | fail | Free usage limit reached before the first edit; relaunched on the paid fallback. |
| 2026-11-26 | J2 UI foundation | impl | deepseek-v4.1-flash-free#high | UI implementation | — | — | — | — | — | 1 | fail | Free usage limit reached before the first edit; relaunched on the paid fallback. |
| 2026-11-26 | J1/J2 paid fallback | impl | opencode-go/deepseek-v4.1-flash | implementation | — | — | — | — | — | — | running | Last-resort lane after the free tier was exhausted; budget risk recorded. |

## Calibration queue

Run these before trusting the routing table:

1. **Scout** — map `crates/fnscope-server` with `deepseek-v4.1-flash-free#max`; grade the brief.
2. **Impl** — one small, isolated Rust job with `deepseek-v4.1-flash-free#high`; grade rework.
3. **Review** — audit that diff with `deepseek-v4.1-flash-free#max`; grade findings and false
   positives.
4. **Critic** — audit the same brief/report/verdict with `mimo-v2.6-flash-free`; grade the
   process findings.
5. Record confidence and update **Current routing** only after the run.

## Update protocol

After every child:

1. Add a grade row.
2. Note one strength and one weakness observed.
3. Update confidence for that task type.
4. If the result contradicts the current routing, change the table or write why not.
5. If a model fails twice on a task type, mark it avoid and route to the backup.
6. If the failure was caused by the brief, record a lesson instead of downgrading the model.

The routing table is the only routing authority. A model's reputation, price or name is not
evidence.
