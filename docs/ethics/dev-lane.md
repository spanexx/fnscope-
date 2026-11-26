# fnscope — Dev Lane

Commands must work from Windows PowerShell and WSL Debian. Do not hardcode a
drive letter, user directory, or absolute path in committed code or scripts.

## Build and test

- Rust builds and tests run in WSL Debian:
  `wsl.exe -d Debian --exec bash -lc '...'`. Rust 1.99.1 and gcc 14.2.0 are
  installed there. The same tree is mounted under `/mnt/c/...` in WSL — derive
  the path; do not assume it in committed files.
- Multiline shell logic goes in a script file under `scripts/`, never a
  heredoc across the Windows/WSL boundary.
- Never run `cargo build`, `cargo test`, or `cargo check` while other agents
  are writing the workspace. The orchestrator runs all gates.
- Smallest check first; full suite once at the end. A check that passed is not
  re-run.

## Serve and verify

- Never put a server and a test in one command. Start the server detached,
  check its port separately, stop it when done.
- Every wait is bounded. Long builds and suites run detached with a log
  outside the repo.
- Verify the API from Windows with `curl.exe` against the default host and
  port `127.0.0.1:4310`. Confirm the port is closed after the check.

## History

- Decisions D1-D5 live in `.reports/goal-build-fnscope.md`.
- Requirements live in `docs/features/fnscope-base/`.
- Gaps live in `docs/drift.md`.
Read them before changing a boundary.
