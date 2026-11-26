# Workflow — Compile window (orchestrator-owned)

## Why this exists

Iteration 3 of `.reports/goal-build-fnscope.md` records the first compile afte
all eleven jobs wrote code in parallel:

> `cargo check -p fnscope-model -p fnscope-parser -p fnscope-core -p fnscope-enrich -p fnscope-server` -> `CARGO_CHECK_STATUS=101`; 7 errors, all in `fnscope-parser` (1 E0308 kind-vs-node in discover.rs, cascading E0599/E0382-style inference error, 4 E0597 iterator-temporary lifetimes, 1 E0382 partial move in body.rs).

The ledger names the cause: "Rust borrow/type errors from delegated code
written without compiler feedback (by design, to avoid parallel cargo
contention)." The pivot: "sent exact compiler diagnostics back to J02 and J03
for targeted fixes; orchestrator retains all gate execution."

**Rule.** Parallel writes happen in bounded windows. A compile window follows
every batch. Only the orchestrator runs cargo. Agents may use a read-only
`rustfmt --check` on their own files while peers write, never cargo.

## Trigge

Any of these ends a write window and starts a compile window:

- A batch of agent jobs reports "done".
- An agent batch touches a shared type, a `Cargo.toml`, or a crate boundary.
- Before a commit is staged.

## Procedure

1. **Close writes.** Confirm every agent in the batch has reported done. Do not
   start the window while any agent is still writing.
2. **Format first.** Run `cargo fmt --all` in WSL Debian (the lane with Rust
   1.99.1 and gcc 14.2.0), then `cargo fmt --all -- --check`. Record
   `FMT_STATUS` exactly as printed.
3. **Compile smallest scope first.** Run `cargo check` on the crates the batch
   touched before the whole workspace. Example from iteration 3: the five
   stable crates. Do not skip to the full workspace if a small scope fails.
4. **Run tests at scope, then workspace.** Start with the crate that changed.
   Once green, run `cargo test --workspace`. Set `FNSCOPE_NODE` if the Node
   invocation test needs native Node 24 in the WSL lane.
5. **Record output.** Append the exact commands, markers, counts and erro
   text to `.reports/goal-build-fnscope.md` as a new iteration. Include the
   failing command and output, not just the verdict.
6. **Dispatch fixes by owner.** For each error, identify the owning job from
   the decomposition and send it the exact diagnostic — error code, file, and
   message — plus the command that produced it. The orchestrator does not edit
   the agent's file to make the gate green.
7. **Reopen writes only after fixes land.** Then repeat steps 2–5 for the
   smallest scope that was red. Once green, run the full suite once.

## Checkable exit criteria

- `cargo fmt --all -- --check` returned `FMT_STATUS=0`.
- The smallest compile scope returned success.
- `cargo test --workspace` returned success with the test count recorded.
- Every failure has a named owner and its verbatim diagnostic in the ledger.
- No gate ran while an agent was writing.
- A check that passed earlier in this window was not repeated.

## DO NOT

- Do not let agents run `cargo` during a write window.
- Do not run a format gate and a test in the same shell command as a server.
- Do not paraphrase a compiler error when the owner needs the exact line.
- Do not mark an iteration `pass` while any diagnostic is still open.
