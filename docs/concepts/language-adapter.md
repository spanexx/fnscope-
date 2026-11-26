# Concept — Language adapter

## The problem this concept solves

Iteration 16 of `.reports/goal-build-fnscope.md` expanded the goal to
"polyglot language support with a pluggable Tree-sitter/language-backend
architecture." The contract update says: "adding a language requires ZERO
changes to core graph builder, schema, enricher or UI." Acceptance evidence
(2)–(5) requires a real scan of `crates/` with non-zero functions, a mixed
polyglot fixture in one manifest, unit plus integration tests for every
backend, and "existing TS/JS gates remain green."

Iteration 15 shows the failure mode this avoids: scanning Rust source returned
"scanned 0 functions in 0 files" because "the scanner only handled TS/JS", and
the response was to add `tree-sitter-rust`, add `Lang::Rust` to shared types,
and dispatch three jobs (J15 Rust parser, J16 core `.rs` discovery, J17 Rust
fixture, J18 docs). That approach puts language knowledge inside core and
shared types. Iteration 16 replaces it with a plugin boundary.

**Rule.** A new backend is a plugin, not a branch in the graph builder.

## What a language adapter owns

An adapter is the only place that knows the syntax of one language. It owns:

- **Extensions and discovery patterns** — which file suffixes and node kinds
  count as a function in that language.
- **Discovery** — producing the bare and qualified names, `FnKind`, 1-based
  `line` and `end_line`, and the exact `source_code` slice. This maps to the
  `DiscoveredFn` shape in `docs/contract/rust-api.md`.
- **Signature and parameters** — names, declared types, `required`,
  `defaultValue`, description, and the return shape including unpacked fields.
- **Execution steps** — ordered `Step` records with `kind`, `title`, `code`,
  `inputs`, `output` and `detail`.
- **Throws and panics** — the language-specific error form (thrown objects,
  `throw new`, `Result`/`panic`, exceptions) mapped to `ErrorSpec` with `code`,
  `status` and `when`.
- **Type index contributions** — aliases, interfaces, structs and enums that
  return fields resolve through, including cross-file references.
- **Callee names** — raw identifiers in step order (`callee_names`), which core
  later resolves to `callsFnId`.
- **Purity and permissions** — the language-specific markers that map to
  `Purity` and the `permissions` list, plus `runtime` and `timeout_ms`.

An adapter produces contract-shaped data. It does not assign final ids, build
the graph, or write the manifest.

## What core owns

`fnscope-core` stays language-neutral. It owns:

- **File walk** — globset discovery with `ScanOptions.includes` / `excludes`,
  including the `target/` and `node_modules` style exclusions.
- **Ids** — `make_id(relative_path, qualified_name)`, stable and unique per
  file plus function.
- **Hash** — `ast_hash(source)` from `normalize_source`.
- **Graph edges** — the name-to-id map that turns `callsFnId` into a resolved
  reference, and the reverse `calledBy` edges with `fn_id: None` for external
  callers.
- **Manifest** — assembling `FnGraph`, `write_manifest`, `read_manifest`, and
  validation of `tool`, `schemaVersion`, unique ids and step ids.

The schema (`fnscope-model`), the enricher (`fnscope-enrich`) and the UI are
also language-neutral. They consume `FnGraph` and must not grow language
branches.

## The invariant

Adding a language changes only the adapter registry: register the new adapter
and add its adapter crate, its fixtures and its tests. A diff that touches
`fnscope-core`'s graph builder, the schema, the enricher, or `ui/` has broken
the invariant and must stop for review. Record the mismatch in `docs/drift.md`
before proceeding, following AGENTS.md.

## How to add a language (recipe)

Implement the adapter trait in a new crate that owns extensions, discovery,
signature and parameter extraction, step construction, error extraction, type
index entries, callee names, and purity/permissions mapping; return exactly the
contract types (`DiscoveredFn`, `BodyAnalysis`, `Step`, `ErrorSpec`,
`ReturnShape`) so core stays unaware of the syntax; register the adapter in the
single adapter registry with its file extensions; add one unit test per
extractor category and one integration test that scans a fixture of that
language into a valid `FnGraph` with resolved `calledBy` edges; add a mixed
polyglot fixture that produces one manifest containing both the new language
and the existing TS/JS functions; then run `cargo test --workspace` and confirm
the existing TS/JS gates are unchanged. If any of this requires editing core,
the schema, the enricher or the UI, stop and raise it as an architecture drift
instead of adding a branch.

## Anti-pattern: per-language `if` branches in core

Do not write code shaped like "if the file ends in `.rs`, do Rust discovery;
else if `.py`, do Python discovery; else do TypeScript" inside the scanner.
That is the iteration 15 pattern scaled up: each new language adds another
branch to the file walk, another branch to discovery, another branch to the
body analysis and another test matrix in core. It makes core the owner of every
language, breaks disjoint file ownership, and means a change for one language
can regress another. The same warning applies to `Lang` as a growing enum with
`match` statements scattered across crates: a registry of adapters is the
seam, not a wider enum.

## Current contract status (the locked seam)

The adapter contract is `docs/contract/adapter-api.md` (merged in a6f5cde). It
records `LanguageAdapter` and `Registry` in
`crates/fnscope-parser/src/adapter.rs`, the compatibility delegates `discover`,
`analyze`, `collect_types` and `resolve_return_fields`, and the single core
injection seam `fnscope_core::scan::scan_with_registry`. Adding a language now
means implementing the trait, registering it and passing the registry to
`scan_with_registry` — no edit to the graph builder, schema, enricher or UI.

`docs/contract/rust-api.md` remains the locked schema contract. `Lang` is still
public in `crates/fnscope-parser/src/types.rs:7-15`, but only for compatibility
with the original entry points; core no longer imports or matches on it. The
old free functions are thin delegates to the built-in registry, so this concept
and the adapter contract describe the same seam, not two competing designs.
