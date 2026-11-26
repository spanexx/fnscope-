# Goal: ship the validated workspace UI as the real fnscope product

Contract: a real, documented, tested, public-ready fnscope workspace UI with isolated project
scopes, explorer navigation, Story / 1-Hop / Split / Raw views, an invoke sandbox, both themes,
and every interaction that was validated in `wireframe/fnscope-workspace`.

Pipeline: feature-pipeline (targeted exploration → GRILL → pre-impl sim → PRD-TRD → IMPL →
implement → post-impl sim → drift check → reconcile)

Worktree: `~/.config/superpowers/worktrees/fnscope/feature-workspace-ui`
Branch: `feature/workspace-ui`

## Contract

| Field | Value |
|---|---|
| Outcome | The real fnscope app ships the validated workspace experience end-to-end, backed by real manifests and isolated scopes. |
| Acceptance evidence | `cargo fmt --all -- --check` exits 0; `cargo test --workspace` exits 0; `bash scripts/check-culture.sh` prints `CULTURE_OK`; the UI build and test/lint scripts in `ui/package.json` exit 0 (exact commands recorded after discovery); a browser acceptance run against the real server demonstrates every item in the PRD-TRD Behavioral Spec with recorded output/screenshots; the feature pack, drift report, and status/context updates exist. |
| Scope | May change: `ui/`, `crates/fnscope-server`, `crates/fnscope-cli`, `crates/fnscope-core`, `crates/fnscope-model`, tests, scripts, fixtures, docs, and the design reference under `wireframe/`. Must not break the locked public Rust API names/JSON schema, existing single-scope CLI behavior, or existing feature contracts. Breaking public-contract changes escalate. No hardcoded absolute paths. No fixture pollution in a production scope. |
| Authority | Delegated by the user on 2026-11-26 to make design and implementation decisions inside this scope. Human gates for PR review/merge, stable blessing, and the commit-to-main decision remain unless explicitly waived in writing. Local worktree commits are allowed for review. |
| Budget | 3 attempts per failure class; 2 review-fix rounds per job; 12 hours wall clock; compact context at phase boundaries. Start with 3–4 scouts and 3–4 implementation workers; scale only while rate limits, CPU and integration stay healthy. |
| Stop conditions | A breaking public-contract change; missing access or credentials; the same failure class after 3 attempts; a scope expansion beyond the validated prototype and real scope isolation; the human PR/merge/stable gate. |

## Authority and gates

- **Decisions:** delegated. Every non-obvious decision is recorded in
  `docs/features/workspace-ui/GRILL-workspace-ui.txt` with `[agent decision]`, the why, and the
  source.
- **Human gates:** the user did not explicitly waive PR review/merge, stable blessing, or the
  commit-to-main decision. The loop stops at the PR review gate after implementation,
  post-impl simulation, drift check, and full verification. It does not merge to `main`.
- **Fast mode:** workers may commit locally inside their own worktrees and run scoped checks;
  the orchestrator reviews, cherry-picks, runs the full gate and owns the final verdict. This is
  the documented exception in `.orchestrator/profile.md`; `AGENTS.md` remains the general rule.

## Pre-work

The validated prototype is `wireframe/fnscope-workspace/` (Paper / Graphite themes, three scopes,
Story / 1-Hop / Split / Raw, invoke drawer, shortcuts). It is the design reference and the
pre-implementation simulation. It is not the implementation.

## Iterations

### 1 — 2026-11-26T22:57

Action: created the isolated feature worktree and restored the orchestrator/design scaffold into it.
Verification: `agent-worktree feature workspace-ui` → `BRANCH=feature/workspace-ui`,
`WORKTREE=~/.config/superpowers/worktrees/fnscope/feature-workspace-ui`; `git status`
shows only the scaffold files to be committed by the pack.
Verdict: pass
Failure class: none
Pivot: none
Next: targeted exploration with deep-dive scouts; no implementation yet.

### 2 — 2026-11-26T23:00

Action: launched five read-only deep scouts on DeepSeek Flash Free `#max` (UI architecture, server/scope model, wireframe acceptance, process/gates, product/security extension points). Scout agent model updated to `#max` and step budget raised to 24.
Verification: five background subagent sessions returned; no files edited by scouts; board written at `.orchestrator/board.md`.
Verdict: pass (discovery running)
Failure class: none
Pivot: none
Next: review each brief, run critic on the synthesis, lock the GRILL, then write PRD-TRD/IMPL.

### 3 — 2026-11-26T23:16

Action: completed discovery with three DeepSeek `#max` briefs (server/scopes, wireframe acceptance,
product/security) and two Hy4 `#high` fallbacks (UI architecture, process/gates) after the free-tier
ceiling. Wrote the locked GRILL, PRD-TRD and IMPL; wrote the discovery synthesis. Ran the culture
gate.
Verification: `CULTURE_SELFTEST_OK` and `CULTURE_OK` after normalizing 187 text files with
`tr -d '\r'` and adding `.gitattributes`; the worktree `.git` was repointed at the WSL gitdir so
WSL Git could resolve `main`. The independent critic (MiMo) hit the same free-tier ceiling and was
relaunched on Hy4.
Verdict: pass (docs locked; critic review pending)
Failure class: cross-environment line endings / WSL worktree gitdir; free-tier concurrency cap
Pivot: add `.gitattributes`, normalize with `tr`, repair the worktree gitdir; use the Hy4 fallback
ladder and record the degradation.
Next: apply critic findings, commit the Phase 0 docs/guardrails, then create the first disjoint
job worktrees (server scopes + UI foundation).

### 4 — 2026-11-26T23:30

Action: independent critic review (Hy4 fallback after the MiMo free-tier limit) found one blocker
(unverifiable verbatim quote), scope/build/acceptance gaps, and guardrail gaps. Fixed the quote,
added `crates/fnscope-cli` to scope, made the orchestrator the sole `ui/dist` builder, added
`check`/`check:source`/`test` scripts, named the browser-audit driver, added file/subdir origin
mapping and positive/negative self-tests, added CSP/size/limit security lines, aligned worker
commit wording, and wired the UI checks into `test-all.sh`/`scan-fixture.sh`.
Verification: `UI_CHECK_OK`, `UI_CHECK_SELFTEST_OK`, `UI_TEST_OK` (2 tests), `UI_CONTRACT_OK`
(1068 functions), `CULTURE_SELFTEST_OK`, `CULTURE_OK` with `ui/src/lib -> workspace-ui` origin
mapping; `git diff --check` clean after fixing the ledger EOF.
Verdict: pass (critic follow-up: `NO_REMAINING_BLOCKERS`)
Failure class: none; free-tier fallback recorded in L-2/L-4
Pivot: use the detector self-tests and the origin mapping probe as the guardrail, not a prose
claim.
Next: commit the docs pack and guardrail batch, then create the server/UI job worktrees.

### 5 — 2026-11-26T23:35

Action: committed the docs pack (`50f0395`) and guardrails (`3f32c5f`); created the J1 server,
J2 UI-foundation and J3 fixture worktrees. Launched J1/J2 on the free `#high` model; both failed
immediately with the free usage limit before editing. Relaunched J1/J2 on the paid orchestrator
model as the documented last-resort fallback.
Verification: free-tier error captured for both jobs; all three job worktrees clean; paid
sessions started; no implementation files integrated yet. Culture gate green before launch.
Verdict: pass (paid fallback running; budget risk recorded)
Failure class: free-tier exhaustion
Pivot: paid fallback, limited concurrency; if the paid budget tightens, finish one critical job
at a time and use the orchestrator directly for integration.
Next: review J1/J2 commits, integrate one at a time, then launch CLI/explorer/views/invoke.

### 6 — 2026-11-26T23:48

Action: J3 fixture reviewed PASS and cherry-picked as `681c691`; additive contract addendum
committed as `9a5dd51`. J1 independent review FAILED on one real blocker: a timed-out Node child
is left running while its temp dir is removed. J2 independent review PASSED as a source audit,
with one finding: the client `ScopeDescriptor` omits the server's nullable `generatedAt`.
Verification: `WORKSPACE_FIXTURE_OK` plus temp-copy mutation proofs (`/absolute` path and leaked
`betaEntry` both fail correctly); J1 `cargo fmt` + 25 scoped server tests green but not accepted
until the timeout cleanup is fixed; J2 `check-ui.mjs --source` passed, but the reviewer's WSL
`npm test` hit a win32 esbuild binary mismatch (environment, not code). No full UI build yet.
Verdict: in progress
Failure class: Node process cleanup; client descriptor type gap
Pivot: send J1 back for process-tree termination + test; send J2 a small `generatedAt` follow-up.
Font bundling remains an explicit Phase 7 task, not silently dropped.
Next: re-review J1; integrate J1/J2; run the full workspace/UI gate; launch CLI/explorer/views/
invoke.
