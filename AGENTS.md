# fnscope — Agent Guide

Rust-first, local-first function inspector. The locked contract lives in `docs/contract/rust-api.md`.
The original product spec lives in `fnscope_architecture_agent_build_spec.md`; the UI contract lives in `fnscope_ui.tsx`.

## Ethos inherited from the reference codebases

- **Modular or it does not merge.** CLI, AST extraction, core graph, enrichment, server, UI and
  fixtures each live in their own crate/directory. A module knows only its own job.
- **Commenting is not optional.** Public items get a one-line `///` explaining *why*, not *what*.
- **Every piece of code has an origin.** Work arrives as a delegated job in `.reports/goal-build-fnscope.md`
  with a named owner and a review. No orphan edits.
- **Drift is a gate.** A gap between the spec/contract and the code goes into `docs/drift.md` before
  the next edit. Never delete an entry to silence a check.
- **Environment agnostic.** Same command works from Windows PowerShell and WSL Debian. Do not hardcode
  a drive letter, user directory or absolute path in committed code or scripts.
- **Clean up after yourself.** Stop processes you started, free ports, remove temp logs and scratch files.
- **Challenge, don't please.** If a decision conflicts with the locked contract, surface it.

## Command lane

- Rust builds/tests run in **WSL Debian** (`wsl.exe -d Debian --exec bash -lc '...'`), where Rust 1.99.1
  and gcc 14.2.0 are installed. The repo is the same tree at `/mnt/c/Users/shuga/Desktop/shared/fnscope`.
- Multiline shell logic goes in a script file under `scripts/`, never a heredoc across the boundary.
- **Never put a server and a test in one command.** Start the server detached, then check its port separately.
- Every wait is bounded. A long build or test suite runs detached with a log outside the repo.
- Run the smallest check first; the full suite once at the end. A check that passed is not re-run.

## Commit and worktree culture (orchestrator-owned)

- **One logical change = one commit.** Conventional prefixes: `feat`, `fix`, `docs`, `test`,
  `refactor`, `chore`. The subject says what changed, not how great it is.
- **Never commit red.** Code batches pass `cargo fmt --all -- --check` and the scope-appropriate
  tests before the commit. The orchestrator runs every gate; agents never run cargo.
- **Agents never commit and never merge.** Each implementation job gets its own git worktree
  outside the main tree. The orchestrator reviews the diff, runs gates, stages explicit paths,
  commits, then removes the worktree.
- **Stage explicitly:** `git add <specific paths>`. Never `git add -A`, never stage a file you
  have not read. A commit must not contain another job's half-written work.
- **Review checklist before every commit:** locked contract names unchanged, JSON schema stable,
  `///` comments on public items, tests for new behavior, no hardcoded absolute paths, LF endings,
  drift log updated if a doc-vs-code gap was found.
- **A failed gate is recorded, not hidden.** Append the real command and output to
  `.reports/goal-build-fnscope.md` before changing approach.

## Parallel work rule

Subagents may run concurrently **only on disjoint file sets**. Never run `cargo build`, `cargo test`
or `cargo check` while other agents are writing the workspace — the orchestrator runs all gates.
Each agent writes files only and reports what it wrote. The orchestrator reviews, criticises and integrates.

## Before you finish a job

- Re-read `docs/contract/rust-api.md` and confirm exact type/function names.
- Add unit tests for the behavior you implemented (tests live beside the code in `#[cfg(test)]`).
- Report: files written, public API added, tests added, and any deviation from the contract.
