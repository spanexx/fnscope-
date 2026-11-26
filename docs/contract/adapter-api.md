# fnscope — language-adapter API (J21)

This document records the exact trait and registry implemented by
`fnscope-parser`, and the single injection seam added to `fnscope-core`.
It implements the concept locked in `docs/concepts/language-adapter.md`.
`docs/contract/rust-api.md` remains the locked contract: no schema field,
`DiscoveredFn`, `BodyAnalysis`, `Step`, `ErrorSpec` or `ReturnShape` was
renamed or reshaped.

## 1. Trait and registry (`crates/fnscope-parser/src/adapter.rs`)

```rust
pub trait LanguageAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn extensions(&self) -> &'static [&'static str];
    fn discover(&self, source: &str, relative_path: &str) -> Vec<DiscoveredFn>;
    fn analyze(&self, found: &DiscoveredFn, source: &str, known_names: &[String]) -> BodyAnalysis;
    fn collect_types(&self, source: &str) -> BTreeMap<String, ReturnShape>;
    fn resolve_return_fields(&self, found: &mut DiscoveredFn, types: &BTreeMap<String, ReturnShape>);
}

pub struct Registry { /* private: Vec<Box<dyn LanguageAdapter>> */ }

impl Registry {
    pub fn new() -> Self;                              // empty registry
    pub fn with_builtin_adapters() -> Self;            // TS, TSX, JS, JSX, Rust, Python, Go
    pub fn register(&mut self, adapter: Box<dyn LanguageAdapter>);
    pub fn for_extension(&self, ext: &str) -> Option<&dyn LanguageAdapter>;
}

pub fn registry() -> Registry; // built-in registry for callers that do not inject one
```

`for_extension` trims a leading dot, lowercases the suffix, and returns the
first registered adapter that claims it. Registration order is stable.

The seven built-in registrations are four `TypeScriptAdapter` instances
(one per `Lang::{TypeScript, Tsx, JavaScript, Jsx}`), one `RustAdapter`, one
`PythonAdapter` (`.py`) and one `GoAdapter` (`.go`). Each `TypeScriptAdapter`
claims exactly one suffix so the original grammar choice (TypeScript vs TSX vs
JavaScript) is preserved byte-for-byte.

`Lang` remains defined in `crates/fnscope-parser/src/types.rs` only so older
callers keep compiling. It is an internal detail of the adapters:
`fnscope-core` no longer imports or matches on it.

## 2. Compatibility surface (unchanged public functions)

The following free functions remain public with their original signatures and
are thin delegates to the built-in registry:

```rust
pub fn discover(lang: Lang, source: &str, relative_path: &str) -> Vec<DiscoveredFn>;
pub fn analyze(found: &DiscoveredFn, lang: Lang, source: &str, known_names: &[String]) -> BodyAnalysis;
pub fn collect_types(lang: Lang, source: &str) -> BTreeMap<String, ReturnShape>;
pub fn resolve_return_fields(found: &mut DiscoveredFn, types: &BTreeMap<String, ReturnShape>);
```

`resolve_return_fields` has no language parameter, so it delegates to a
built-in adapter whose implementation calls the shared grammar-free resolver
in `crates/fnscope-parser/src/resolve.rs`.

## 3. Adjustments to the requested sketch, with reasons

1. **One TS adapter instance per suffix (four TS-family instances among seven built-ins).**
   `discover`/`analyze`/`collect_types` receive no path, so a single instance
   could not choose the TSX grammar for `.tsx` and the JavaScript grammar for
   `.jsx`. One instance per suffix preserves the pre-refactor grammar and
   output exactly.
2. **`Lang` kept public in `types.rs`.** Removing it would break the
   compatibility wrappers and the locked contract section 2. Core's dependence
   on it is what was removed.
3. **Shared return-field resolver in `src/resolve.rs`.** The original free
   function resolved both `Promise<T>` (TS) and `Result<T, E>` / `Option<T>` /
   `Vec<T>` (Rust) names. The module only manipulates declared type-name text,
   owns no tree-sitter grammar and no extension table, so it is safe for both
   adapters and for the language-free compatibility wrapper to share.
4. **Core injection seam `scan_with_registry`.** Signatures of `scan`,
   `ScanOptions`, `ScanReport`, `write_manifest` and `read_manifest` are
   unchanged. `scan` now builds `registry()` and calls
   `scan_with_registry(options, out_dir, generated_at, &registry)`.
   A new public function was needed so a caller (or a unit test) can inject a
   registry without global mutable state — the proof that adding a language
   requires no core edit.

## 4. Core change (one language-dispatch point)

`fnscope-core/src/scan.rs` no longer has `lang_for_path`, no `Lang`, and no
per-language `if`/`match`. Each discovered file does exactly one lookup:

```rust
let extension = relative.rsplit_once('.').map(|(_, ext)| ext).unwrap_or_default();
let Some(adapter) = registry.for_extension(extension) else { continue; };
types.extend(adapter.collect_types(&source));
let found = adapter.discover(&source, &relative);
```

Pass two calls `adapter.resolve_return_fields(...)` and
`adapter.analyze(...)`. Ids, hashes, name-to-id maps, `called_by` edges,
manifest assembly, `write_manifest` and `read_manifest` are untouched.

## 5. How to add a language

1. Create a new crate (as was done for the now built-in `python` and `golang`
   adapters, e.g. `fnscope-adapter-ruby` for the next language) that depends on
   `fnscope-parser` and `fnscope-model`. Implement `LanguageAdapter` for a
   struct that owns the language's extensions, discovery, signature/parameter
   extraction, steps, errors, type index entries, callee names and
   purity/permissions. Return the contract types unchanged.
2. Register the adapter. Python and Go joined TS/TSX/JS/JSX and Rust through the
   same call in `Registry::with_builtin_adapters`; a future language becomes a
   built-in by adding its `register(Box::new(MyAdapter::new()))` line there.
   Callers that do not want a built-in call `Registry::new()`,
   `register(Box::new(MyAdapter))` and pass the registry to
   `fnscope_core::scan::scan_with_registry`.
3. Add unit tests for each extractor category beside the adapter code, and an
   integration test that scans a fixture of the new language into a valid
   `FnGraph` with resolved `called_by` edges. Add or extend a mixed polyglot
   fixture so one manifest contains the new language and the existing TS/JS,
   Rust, Python and Go functions.
4. Run `cargo test --workspace` and confirm the existing TS/JS and other
   language gates are unchanged.

No edit to `fnscope-core`'s graph builder, `fnscope-model`, `fnscope-enrich`,
`ui/`, `docs/contract/rust-api.md` or `docs/drift.md` is part of this recipe.
If any step appears to require one, stop and raise it as architecture drift
instead of adding a branch.

## 6. Evidence recorded by J21

- Registry resolves `ts`, `tsx`, `js`, `jsx`, `rs` and returns `None` for an
  unregistered suffix (`src/adapter.rs` unit tests).
- A fake adapter claiming `.zig` is selected by core through
  `scan_with_registry` and produces one function, while the built-in registry
  produces none for the same file (`crates/fnscope-core/src/scan.rs` test).
- Representative TS and Rust snippets keep their pre-refactor discovery and
  body-analysis facts (`src/typescript/mod.rs`, `src/rust/mod.rs` tests), and
  every original TS/JS and Rust test module is retained verbatim in
  `src/discover.rs` and `src/body.rs`.

This list is J21's original evidence; Python and Go were added later by
following section 5, with their own tests and fixtures.

Gates were not run by J21. Verdicts belong to the orchestrator.
