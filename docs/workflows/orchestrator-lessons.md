# Orchestrator lessons

Every orchestration run ends here. The next orchestrator reads this before planning.

| ID | Date | Run | What happened | Root cause | Guardrail | Applied to |
|---|---|---|---|---|---|---|
| L-1 | 2026-11-26 | orchestrator-skill-v2 | The old skill routed to `cline-one` / `opencode-action` CLI wrappers, hid model choice, and relied on report files and exit codes. | The orchestration layer was built around external processes instead of OpenCode's built-in child sessions. | Route to explicit OpenCode agents (`impl`, `scout`, `review`, `critic`); every child has an explicit model; use background subagent reports; keep the model scorecard. | skill, agent roster, scorecard |
| L-2 | 2026-11-26 | workspace-ui discovery | Five DeepSeek `#max` scouts were launched at once; two returned empty or hit the free usage limit. | Free-tier concurrency/usage cap, not task complexity; the wave ignored the model's known quota risk. | Cap concurrent `#max` scouts at 1–2; use Hy4/MiMo for broad reads; reserve DeepSeek `#max` for synthesis, review and high-risk context; fall back within the free tier and record the degradation. | scorecard, board, scout routing |
| L-3 | 2026-11-26 | worktree setup | The Windows-created worktree checked out CRLF files; `check-culture.sh` died at `set -euo pipefail`, and WSL Git could not resolve the worktree's Windows `.git` path. | No `.gitattributes` and a Windows-generated `gitdir` file used across the WSL boundary. | Add `* text=auto eol=lf`; normalize with `tr -d '\r'`; point the worktree `.git` at the WSL gitdir for WSL Git; re-run `CULTURE_OK`. | repo, worktree setup, ledger |
| L-4 | 2026-11-26 | discovery/review | DeepSeek `#max` and MiMo both hit free usage limits; the Hy4 fallbacks completed. | The free-tier cap applies across models and concurrent sessions, not just one model. | Check availability before a wave; cap free-agent concurrency; use the fallback ladder DeepSeek `#max` → Hy4 `#high` → orchestrator; record the degradation. | scorecard, scheduler |

## How to add a lesson

Use the next ID (`L-2`, `L-3`, …). Keep the row short; put detail below if needed.

Required fields:

- **What happened** — the observable failure or success.
- **Root cause** — the rule, brief, model or process that allowed it.
- **Guardrail** — the concrete check or routing change that prevents a repeat.
- **Applied to** — skill, agent prompt, scorecard, checklist or repo workflow.

Rules:

- A lesson that was not applied is not closed. Mark it `open` until the guardrail exists.
- A recurring lesson is an escalation, not another row. Strengthen the existing guardrail.
- If a model failed because the brief was vague, fix the brief; do not blame the model.
- If a model failed twice on the same task type, update the scorecard to route around it.
- If nothing needs changing, record an evidence-backed "no change" rather than inventing work.

## Open lessons

_None yet. The first calibration wave will populate this section._

## Retrospective prompts

At the end of every wave or pack:

1. What broke, and what did it cost?
2. Why did the process allow it?
3. Which rule, brief, model or gate should have caught it?
4. What changed now, and where is it recorded?
5. Did a previous lesson recur? If yes, escalate.
6. What is the next smallest experiment that would test the fix?
