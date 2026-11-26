# fnscope — Modular Ethics

**STRICT MODULAR DESIGN** — every responsibility belongs to one crate or
module with one job.

- One crate per job: `fnscope-model` (schema), `fnscope-parser` (discovery and
  body analysis), `fnscope-core` (scan and manifest I/O), `fnscope-enrich`
  (cache and backends), `fnscope-server` (HTTP and invoke), `fnscope-cli`
  (commands and watcher), plus `ui/`, `tests/fixtures/`, and `scripts/`.
- A crate owns its `Cargo.toml` and its public surface. Depend on the public
  API in `docs/contract/rust-api.md`; never reach into another crate's
  internals or duplicate its types.
- `crates/fnscope-parser/src/types.rs` and `src/lib.rs` are owned by the
  orchestrator. A job must not edit a file it does not own (AGENTS.md).
- Small focused files inside a crate: `discover.rs` discovers, `body.rs`
  analyses bodies. Do not merge unrelated concerns into one file.
- Shared logic lives in one place — model types in `fnscope-model`,
  normalization and hashing in `fnscope-core` — instead of being copied into
  every crate.
- Each crate is testable in isolation, with tests beside the code in
  `#[cfg(test)]`.
- No parallel structures: before writing, read the contract and find the
  owning crate. If nothing owns the change, report to the orchestrator
  instead of inventing a new module.

**Non-negotiable:** no monolithic crate, no responsibility that accumulates
in one module, no bypass of a crate boundary. Modularity is what lets the
build be split across subagents and lets one piece be replaced without
breaking the others.
