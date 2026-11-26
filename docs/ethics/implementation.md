# fnscope — Implementation Ethics

- Test first where practical: write the failing test beside the code, then
  make it pass. Every public behavior in the contract gets a unit test in
  `#[cfg(test)]` in the same crate.
- Real tests only. Aim high on coverage and add regression and edge cases for
  each module: empty source, formatting-only change, missing manifest, failed
  Node runner, unknown callee.
- The orchestrator runs all gates (`cargo test`, `cargo run`, curl, browser).
  Agents write files and report. No agent runs cargo while other agents are
  writing the workspace.
- Smallest check first; the full workspace suite once at the end. A check
  that passed is not re-run.
- Never put a server and a test in one command. Start the server detached,
  check its port separately, stop it afterwards.
- Leave the bench clean after every check — see `cleanup.md`.
- If a change would contradict `docs/contract/rust-api.md`, stop and record
  the gap in `docs/drift.md` before editing. Challenge the contract; do not
  quietly please it.
- Report at the end of a job: files written, public API added, tests added,
  and any deviation from the contract.
