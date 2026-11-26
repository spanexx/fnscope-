# Orchestrator repo profile — fnscope

This file wires the generic `orchestrator` skill to this repository. The skill stays
project-agnostic; this profile supplies the concrete paths, gates, locks and roster.

## Documents

| Purpose | Path |
|---|---|
| Agent guide | `AGENTS.md` |
| Domain glossary | `CONTEXT.md` |
| Locked contract | `docs/contract/rust-api.md` |
| Current status | `docs/STATUS.md` |
| Drift log | `docs/drift.md` |
| Review workflow | `docs/workflows/reviewer-process.md` |
| Culture gate | `scripts/check-culture.sh` |

## Gates

Run the gates named by the feature pack's IMPL and the repo guide. At minimum:

- format: `cargo fmt --all -- --check`
- tests: `cargo test --workspace`
- culture: `bash scripts/check-culture.sh`
- UI: use the scripts declared in `ui/package.json`; record the exact command when first used.

Rust commands run in WSL Debian. Do not put a server and a test in one command. Every wait is
bounded; long suites run detached with a log outside the repo.

## Shared files / serial locks

These must not be edited by two parallel jobs:

- `docs/contract/rust-api.md`
- `docs/STATUS.md`
- `docs/drift.md`
- `Cargo.toml`, `Cargo.lock`
- `docs/features/<slug>/` (feature-pack docs)
- `.reports/goal-build-fnscope.md`
- any router, schema or registry file touched by more than one job

## Registers

| Register | Path |
|---|---|
| Model scorecard | `docs/workflows/model-scorecard.md` |
| Lessons | `docs/workflows/orchestrator-lessons.md` |
| Job board (transient) | `.orchestrator/board.md` |

## Agents

The subagents are **global** OpenCode agents, shared across repositories. They carry no
repo-specific paths or gates; the job brief supplies those.

| Role | Agent ID | Global definition |
|---|---|---|
| Implementation | `impl` | `~/.config/opencode/agents/impl.md` |
| Scout | `scout` | `~/.config/opencode/agents/scout.md` |
| Reviewer | `review` | `~/.config/opencode/agents/review.md` |
| Critic | `critic` | `~/.config/opencode/agents/critic.md` |

## Model roster (current environment)

Keep this table in sync with the global agent files. The scorecard is the evidence; this is
the current default.

| Role | Model |
|---|---|
| Orchestrator | user-selected |
| Implementation | `opencode/deepseek-v4.1-flash-free#high` |
| Scout | `opencode/deepseek-v4.1-flash-free#max` |
| Reviewer | `opencode/deepseek-v4.1-flash-free#max` |
| Critic | `opencode/mimo-v2.6-flash-free` |

Anthropic Haiku is not in the current catalog. If it is connected, grade it before promoting it.

## Worktrees and command lane

- Use the current worktree root; derive paths from `pwd` instead of hardcoding a drive or home path.
- Rust commands run in WSL Debian per `AGENTS.md`; create one worktree per implementation job and give the child the absolute worktree path in its brief.
- Rust jobs need separate `CARGO_TARGET_DIR`s or a build semaphore.
- The orchestrator stays in the integration worktree and is the only writer to main.

## Fast-mode exception

`AGENTS.md` says agents never commit and never run cargo; the orchestrator runs every gate. For
parallel delivery this profile applies the skill's documented fast-mode exception:

- a worker may commit locally inside its own worktree, never merge/push/main;
- a worker may run scoped checks in its worktree;
- the orchestrator reviews, cherry-picks, runs the full gate and commits to main.

Record the exception in the run report. Amend `AGENTS.md` only with explicit approval.
