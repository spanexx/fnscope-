# Orchestrator board — workspace-ui

Goal ledger: `.reports/goal-workspace-ui.md`
Worktree: `~/.config/superpowers/worktrees/fnscope/feature-workspace-ui`
Branch: `feature/workspace-ui`

Status: **discovery wave** — no implementation until the GRILL is locked.

## Wave 0 — discovery (running)

| ID | Job | Agent | Model | Owned / read scope | Status |
|---|---|---|---|---|---|
| S1 | UI architecture and port surface | `scout` | DeepSeek `#max` → Hy4 `#high` fallback | `ui/`, `fnscope_ui.tsx` | complete (Hy4) |
| S2 | Server, manifest and real scope model | `scout` | DeepSeek Flash Free `#max` | `crates/fnscope-server`, core/model, CLI | complete |
| S3 | Wireframe acceptance inventory | `scout` | DeepSeek Flash Free `#max` | `wireframe/fnscope-workspace/` | complete |
| S4 | Process, gates and guardrails | `scout` | DeepSeek `#max` → Hy4 `#high` fallback | `docs/features/`, scripts, tests | complete (Hy4) |
| S5 | Public product, UX and security extension points | `scout` | DeepSeek Flash Free `#max` | prototype + UI + docs | complete |

Pack docs: GRILL/PRD-TRD/IMPL locked; independent critic review confirmed
`NO_REMAINING_BLOCKERS` on Hy4 after the MiMo free-tier limit. Phase 0 guardrails, UI checks,
contract check, and culture gate are green. Docs commit next, then job worktrees.

## Planned waves

| Wave | Deliverable | Gate |
|---|---|---|
| 1 | Discovery, GRILL/PRD-TRD/IMPL, Phase 0 guardrails | Docs locked; critic fixes applied |
| 2 | Real scope registry + additive API; CLI workspace config | API tests + review |
| 3 | UI foundation: theme tokens, shell, layout, state, primitives | Source check + review |
| 4 | Explorer: scopes, entry points, module tree, search, breadcrumb | Source check + review |
| 5 | Canvas: Story, 1-Hop, Split, Raw | Source check + review |
| 6 | Invoke drawer | Source check + review |
| 7 | Accessibility, responsive, shortcuts, offline polish | Build + browser audit |
| 8 | Post-impl sim, drift check, reconcile, full gate | PR-ready evidence |

Workers never build `ui/dist`; the orchestrator runs `bash ui/build.sh` after integration.
Waves 4–6 are parallel only because they own disjoint directories.

## Shared locks

- `docs/contract/rust-api.md`
- `docs/STATUS.md`, `docs/drift.md`
- `Cargo.toml`, `Cargo.lock`
- `docs/features/workspace-ui/`
- `ui/` entry points and build config — one owner at a time
- server router/schema and scope registry — one owner at a time

## Rules

- One writer to main: the orchestrator.
- Workers get disjoint file sets and their own worktrees.
- Every worker report is graded in `docs/workflows/model-scorecard.md`.
- Every code diff gets `review`; high-risk/contract changes also get `critic`.
- No model inheritance; every child has an explicit model.
