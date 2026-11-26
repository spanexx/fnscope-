# fnscope-adapter-registry — PRD-TRD (reconstructed)

RECONSTRUCTED 2026-11-26. This document was written after J21 (registry
refactor) and J22 (independent review) shipped, and after D-4 was closed and
the refactor committed as `a6f5cde`. It is a reconstruction from the locked
spec, the ledger, the review and the shipped files — not a pre-impl artifact.
No date is backdated and no quote is invented. Status: SHIPPED.

## Why

The base product scans TS/JS into a schema-valid function graph manifest.
Iteration 15 added Rust by putting `Lang::Rust` and a `.rs` branch into core,
which made `fnscope scan crates/` work but made core the owner of a grammar.
Iteration 16 expanded the goal:

> "fnscope scans Rust, TS/JS, Python, Go and the standard language set with
> accurate function graphs and execution steps; adding a language requires
> ZERO changes to core graph builder, schema, enricher or UI"
>
> — `.reports/goal-build-fnscope.md:181`

With more languages queued, the enum-plus-match pattern would scale badly:
every new language would add a branch to the file walk, discovery, body
analysis and core's test matrix, and a change for one language could regress
another (`docs/concepts/language-adapter.md`, "Anti-pattern"). J21 replaced
that with one seam: a `LanguageAdapter` trait plus a `Registry`, with core
doing a single extension lookup and no language knowledge. J22 audited it;
its first PASS was false, D-4 recorded the gap, and the re-review plus the
real gate closed it (ledger iterations 22-24, `:230-252`; `.reports/reviews/J21.md`;
`docs/drift.md:45-49`).

The feature under reconstruction is therefore the **seam**, not any one
language: it is what makes every later adapter an isolated, reviewable change.

## Behavioral spec

### Registry surface

- `LanguageAdapter: Send + Sync` has exactly six methods: `id()`,
  `extensions()`, `discover()`, `analyze()`, `collect_types()` and
  `resolve_return_fields()`, each returning contract shapes from
  `fnscope-model` / `crates/fnscope-parser/src/types.rs`
  (`docs/contract/adapter-api.md:12-20`; `adapter.rs:23-48`).
- `Registry::new()` is empty. `Registry::with_builtin_adapters()` registers
  the built-ins; `Registry::register` appends; `registry()` returns the
  built-in registry for callers that do not inject one (`adapter.rs:54-96`).
- J21 shipped seven registrations: four `TypeScriptAdapter` instances (one per
  `Lang::{TypeScript, Tsx, JavaScript, Jsx}`, preserving the original grammar
  byte-for-byte), plus `RustAdapter`, and — as recorded later in the same
  contract — Python and Go (`docs/contract/adapter-api.md:34-38`, `:125-137`).
  The current `adapter.rs:62-78` registers fifteen adapters; the additional
  languages were added by later jobs through the same seam.
- `for_extension` trims a leading dot, lowercases the suffix and returns the
  **first** registered adapter that claims it; registration order is stable
  and later registrations never override an earlier claim (`adapter.rs:81-91`).
  An unclaimed suffix returns `None` (`adapter.rs:129-134`: `zig`, empty string).
  Bare suffixes such as `"py"`, `"GO"` and `".ts"` all resolve.

### Compatibility surface

The free functions `discover`, `analyze`, `collect_types` and
`resolve_return_fields` keep their original signatures and are thin delegates
to built-in adapters (`docs/contract/adapter-api.md:44-58`;
`crates/fnscope-parser/src/discover.rs:32-62`, `body.rs:19-30`).
`resolve_return_fields` has no language parameter and delegates to a shared,
grammar-free resolver in `crates/fnscope-parser/src/resolve.rs:14-127`.
`Lang` stays public in `types.rs` only for these older entry points; core no
longer imports or matches on it.

### Core surface

- `scan(options, out_dir, generated_at)` keeps its signature and builds
  `registry()` before delegating to `scan_with_registry`
  (`crates/fnscope-core/src/scan.rs:217-225`).
- `scan_with_registry(options, out_dir, generated_at, &registry)` is the single
  injection seam (`:227-236`). Per discovered file, production code does one
  `registry.for_extension(extension)` lookup (`:262-268`) and calls
  `collect_types`, `discover`, `resolve_return_fields` and `analyze` on the
  selected adapter (`:271`, `:272`, and the pass using the adapter below).
  There is no `Lang` import, no per-language `if`/`match`, so ids, hashes,
  name-to-id maps, `calledBy` edges and manifest assembly stay language-neutral.
- An injected adapter is observable: `FakeAdapter` claims `.zig`; the built-in
  registry scans that file to zero functions, while `scan_with_registry` with
  `Registry::new()` plus `register(Box::new(FakeAdapter))` yields one function
  named `zeta` in `main.zig` (`scan.rs:623-711`).

### Invariant

Adding a language changes only the new adapter, its registry entry, its
fixtures and its tests. A diff touching `fnscope-core`'s graph builder,
`fnscope-model`, `fnscope-enrich`, `ui/`, `docs/contract/rust-api.md` or the
schema version is a drift stop, written to `docs/drift.md` before any next
edit (L9, `docs/ethics/lessons.md:187-200`). Registration itself is a separate
orchestrator-delegated job per batch so two worktrees never write `adapter.rs`
or a manifest together (ledger iteration 24, `:251`).

## Simulation contract

- `simulate.sh` in this directory is the canonical **post-impl** simulation.
  It is a source-level simulator: it reads `adapter.rs`, the parser adapter
  modules and `fnscope-core/src/scan.rs`, builds a suffix-to-adapter table from
  the declared `extensions()` / `id()` bodies and evaluates the scenarios
  below. It runs no `cargo`, starts no server, writes no files and resolves the
  repo root from its own path (no hardcoded drive or user directory).
  Scenarios, each with one `[PASS]`/`[FAIL]` line:
  1. **S1 — registry resolves built-ins.** Every documented built-in suffix
     (`ts`, `tsx`, `js`, `jsx`, `rs`, `py`, `go`, `java`, `c`, `cpp`, `cc`,
     `cs`, `kt`, `php`, `rb`, `swift`) maps to its expected adapter id, and
     each adapter type is registered in `with_builtin_adapters`.
  2. **S2 — unregistered suffix is `None`.** No production `extensions()`
     claims `zig`, `for_extension` returns `Option` via `find_map`, and the
     unit test asserting `for_extension("zig").is_none()` exists.
  3. **S3 — fake adapter injected into core.** The core test declares
     `FakeAdapter`, claims `.zig`, registers it into `Registry::new()` and
     calls `scan_with_registry`; the built-in scan is empty and the injected
     scan yields one function named `zeta`.
  4. **S4 — no language branch in core.** The production part of `scan.rs`
     (before `#[cfg(test)]`) contains no standalone `Lang` token, no
     per-language `if`/`match`, exactly one `for_extension(` call, and `scan`
     delegates to `scan_with_registry`.
  5. **S5 — LF-only deliverables.** All five files in this pack contain zero
     CR bytes, checked in the same way as `tr -cd '\r' < file | wc -c` == 0.
  The script exits 0 only when all scenarios pass and prints `ADAPTER_SIM_OK`;
  any failure prints `ADAPTER_SIM_FAIL` and exits 1. This is a simulation of
  the seam's recorded behavior, **not** a compile or test gate — the real gate
  is the orchestrator's `ADAPTER_GATE_STATUS=0` at ledger iteration 24.
- `archive/simulate-pre.reconstructed.sh` is a **labelled design-time
  placeholder**. No pre-impl simulation artifact exists in the evidence for
  J21/J22, so it asserts nothing and prints a placeholder marker. It is not a
  verdict and must not be quoted as one.
- Byte checks use the single-quoted WSL form in a script file, never
  `sed 's/\r$//'` (L11, `docs/ethics/lessons.md:238-243`).

## Technical design

- `crates/fnscope-parser/src/adapter.rs` owns the trait, the `Registry` (a
  `Vec<Box<dyn LanguageAdapter>>`), `for_extension`'s trim/lowercase/first-claim
  lookup and the built-in `registry()` helper. Public items carry one-line
  `///` comments explaining why (`adapter.rs:17-96`).
- One `TypeScriptAdapter` instance per suffix keeps the pre-refactor grammar
  choice (`Lang::Tsx` -> `.tsx`, `Lang::JavaScript` -> `.js`, `Lang::Jsx` ->
  `.jsx`, default -> `.ts`) without changing `discover`/`analyze` signatures.
  This is deviation 1 in `docs/contract/adapter-api.md:64-68`.
- `Lang` remains in `types.rs` (deviation 2) only so the compatibility wrappers
  and locked contract section 2 keep compiling; core's dependence is what was
  removed. The shared resolver in `src/resolve.rs` (deviation 3) manipulates
  declared type-name text only and is safe for adapters and the language-free
  wrapper alike.
- `scan_with_registry` (deviation 4) is the only new core function. `scan`,
  `ScanOptions`, `ScanReport`, `write_manifest` and `read_manifest` are
  unchanged; ids, `ast_hash`, graph edges and manifest validation are untouched
  (`docs/contract/adapter-api.md:70-82`, `:100-123`).
- Review and hygiene were part of the design: J21 wrote code in its own
  worktree with no cargo/commit; J22 audited source only with a checklist that
  later gained semantic-sanity item 12 (L11); files were re-merged with `cp`
  only and verified with `tr -cd '\r' < file | wc -c` == 0; the false PASS was
  kept and D-4 closed with real gate evidence (`.reports/reviews/J21.md`;
  `docs/drift.md:45-49`).
- Delivery: `a6f5cde refactor: pluggable language adapter registry` at ledger
  iteration 24 (`:248`); `ADAPTER_GATE_STATUS=0` with 101 tests (parser 44).

## Non-goals

- No schema change: no new field, no rename, no serde rename, no new schema
  version. `DiscoveredFn`, `BodyAnalysis`, `Step`, `ErrorSpec`, `ReturnShape`
  and `FnGraph` keep their locked shapes (`docs/contract/rust-api.md`).
- No language branch in core, no global mutable registry, no widening of
  `Lang` into a universal enum.
- No edit to the enricher or UI, and no claim that an adapter executes a
  non-TS/JS language through `invoke`.
- No agent-run cargo, no agent merge, no agent commit; format/syntax checks
  are not compile evidence (L11, `docs/ethics/lessons.md:226-252`).
- No backdating, no invented user quote, and no claim that the reconstructed
  simulation replaces the orchestrator's recorded gate.
- No pre-impl simulation claim: the archived script is a labelled placeholder.

## Sources

`.reports/goal-build-fnscope.md` iterations 15-16 and 20-24 (`:170-188`,
`:214-252`); `.reports/reviews/J21.md` (original review and `## Re-review
2026-11-26`); `docs/contract/adapter-api.md`;
`docs/concepts/language-adapter.md`; `docs/ethics/lessons.md` L1/L9/L10/L11;
`docs/drift.md` D-4; `crates/fnscope-parser/src/adapter.rs`;
`crates/fnscope-core/src/scan.rs`; `docs/features/fnscope-base/IMPL.md:241-245`.
