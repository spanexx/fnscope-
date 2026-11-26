# fnscope — locked Rust integration contract

Every agent MUST code exactly against this contract. Do not rename types, fields, or functions.
Serde uses `rename_all = "camelCase"` where noted; `type` fields are renamed explicitly.
All public structs derive `Clone, Debug, Serialize, Deserialize, PartialEq` unless stated otherwise.

## 1. `fnscope-model` — JSON schema (crate owns its own Cargo.toml + lib.rs)

```rust
pub enum Purity { Pure, SideEffect }          // serde: "pure" | "side-effect"
pub enum StepKind { Call, Branch, Transform, Return } // serde lowercase

pub struct FnGraph {
    pub tool: String,             // always "fnscope"
    pub schema_version: String,   // serde "schemaVersion", always "1.0.0"
    pub generated_at: String,     // serde "generatedAt", RFC3339 UTC
    pub root_dir: String,         // serde "rootDir"
    pub functions: Vec<FnRecord>,
}

pub struct FnRecord {
    pub id: String, pub name: String, pub module: String,
    pub file_path: String,        // serde "filePath"
    pub line: usize,
    pub ast_hash: String,         // serde "astHash", "sha256:<64 hex>"
    pub version: String,          // default "1.0.0"
    pub runtime: String,          // e.g. "node20 / edge", "universal"
    pub purity: Purity,
    pub timeout_ms: u64,          // serde "timeoutMs"
    pub permissions: Vec<String>,
    pub description: String,
    pub tags: Vec<String>,
    pub called_by: Vec<CallerRef>,// serde "calledBy"
    pub parameters: Vec<Parameter>,
    pub steps: Vec<Step>,
    pub returns: ReturnShape,
    pub errors: Vec<ErrorSpec>,
    pub source_code: String,      // serde "sourceCode"
}

pub struct CallerRef {
    pub fn_id: Option<String>,    // serde "fnId", null for external entrypoint
    pub name: String, pub module: String, pub call_site: String, // serde "callSite"
}
pub struct Parameter {
    pub name: String,
    #[serde(rename = "type")] pub type_: String,
    pub required: bool,
    pub default_value: Option<serde_json::Value>, // serde "defaultValue"
    pub description: String,
}
pub struct Step {
    pub id: String,               // "step-1", "step-2", ...
    pub kind: StepKind,
    pub calls_fn_id: Option<String>, // serde "callsFnId"
    pub title: String, pub code: String,
    pub inputs: Vec<String>, pub output: String, pub detail: String,
}
pub struct ReturnShape {
    #[serde(rename = "type")] pub type_: String,
    pub fields: Vec<ReturnField>,
}
pub struct ReturnField {
    pub name: String,
    #[serde(rename = "type")] pub type_: String,
}
pub struct ErrorSpec { pub code: String, pub status: Option<u16>, pub when: String }

// Enrichment cache (separate file .fnscope/enrich-cache.json)
pub struct EnrichCache { pub version: u32, pub entries: std::collections::BTreeMap<String, EnrichEntry> }
pub struct EnrichEntry {
    pub description: Option<String>,
    pub step_titles: Vec<String>, pub step_details: Vec<String>,
    pub defaults: std::collections::BTreeMap<String, serde_json::Value>,
    pub tags: Vec<String>,
}

// Invoke API
pub struct InvokeRequest { pub fn_id: String, pub args: serde_json::Value } // serde "fnId"
pub struct InvokeResponse {
    pub ok: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
    pub trace: std::collections::BTreeMap<String, TraceEntry>,
    pub duration_ms: f64,         // serde "durationMs"
}
pub struct TraceEntry { pub status: String, pub output: String }

impl FnGraph {
    pub fn validate(&self) -> Result<(), String>; // tool, schemaVersion, unique ids, step ids, "sha256:" prefix
}
```

## 2. `fnscope-parser` — shared types (orchestrator owns `src/types.rs` and `src/lib.rs`)

```rust
pub enum Lang { TypeScript, Tsx, JavaScript, Jsx }
pub enum FnKind { Declaration, Arrow, Method }

pub struct DiscoveredFn {
    pub name: String,             // bare name: "evaluateGrammarRole"
    pub qualified_name: String,   // "ClassName.method" or bare name
    pub kind: FnKind,
    pub line: usize,              // 1-based
    pub end_line: usize,
    pub source_code: String,      // exact source slice from first to last byte of node
    pub parameters: Vec<Parameter>,
    pub returns: ReturnShape,
    pub doc_description: Option<String>, // JSDoc or leading // comment text
    pub tags: Vec<String>,        // @tags from JSDoc
    pub is_async: bool,
    pub class_name: Option<String>,
}

pub struct BodyAnalysis {
    pub steps: Vec<Step>,
    pub errors: Vec<ErrorSpec>,
    pub purity: Purity,
    pub permissions: Vec<String>,
    pub callee_names: Vec<String>, // raw callee identifiers, in step order
    pub timeout_ms: u64,
    pub runtime: String,
}

// J02 owns src/discover.rs; J03 owns src/body.rs
pub fn discover::discover(lang: Lang, source: &str, relative_path: &str) -> Vec<DiscoveredFn>;
pub fn body::analyze(found: &DiscoveredFn, lang: Lang, source: &str, known_names: &[String]) -> BodyAnalysis;
```

`known_names` is the list of bare/qualified function names discovered in the whole file set; if a callee name matches one, that step's `callsFnId` must be the **name** (core later maps name -> id).

## 3. `fnscope-core` — scanner (J04 owns whole crate)

```rust
pub struct ScanOptions {
    pub target_dir: std::path::PathBuf,
    pub includes: Vec<String>,    // default ["**/*.ts","**/*.tsx","**/*.js","**/*.jsx"]
    pub excludes: Vec<String>,    // default ["**/node_modules/**","**/dist/**","**/*.test.ts","**/*.spec.ts", ...]
}
impl Default for ScanOptions { /* also target_dir = "." */ }

pub struct ScanReport {
    pub manifest_path: std::path::PathBuf,
    pub function_count: usize,
    pub file_count: usize,
    pub warnings: Vec<String>,
}

pub fn scan(options: &ScanOptions, out_dir: &std::path::Path, generated_at: String) -> anyhow::Result<(FnGraph, ScanReport)>;
pub fn write_manifest(graph: &FnGraph, out_dir: &std::path::Path) -> anyhow::Result<std::path::PathBuf>;
pub fn read_manifest(path: &std::path::Path) -> anyhow::Result<FnGraph>;
pub fn normalize_source(source: &str) -> String; // strip comments + all whitespace outside string/template literals
pub fn ast_hash(source: &str) -> String;         // "sha256:" + hex(sha256(normalize_source(source)))
pub fn make_id(relative_path: &str, qualified_name: &str) -> String; // kebab-case slug, stable, unique per file+fn
```

`scan` must: discover files (globset), parse, analyze bodies, build ids, resolve `callsFnId` via a name->id map, populate reverse `called_by` edges (external callers get `fn_id: None`), assign `ast_hash`, and return an un-enriched graph (deterministic fallbacks allowed for description/title/detail/defaultValue).

## 4. `fnscope-enrich` (J05 owns whole crate)

```rust
pub struct EnrichReport { pub cache_hits: usize, pub cache_misses: usize, pub api_calls: usize }
pub trait Enricher { fn enrich(&self, graph: &mut FnGraph) -> anyhow::Result<EnrichReport>; }
pub struct NoopEnricher;                     // deterministic fallbacks only, zero API calls
pub trait LlmBackend { fn complete(&self, prompt: &str) -> anyhow::Result<serde_json::Value>; }
pub struct MockBackend { pub replies: std::sync::Mutex<Vec<serde_json::Value>>, pub calls: std::sync::atomic::AtomicUsize }
impl LlmBackend for MockBackend { ... }
pub struct CachedEnricher { /* config, cache path, backend */ }
impl CachedEnricher {
    pub fn new(cache_path: std::path::PathBuf, backend: Box<dyn LlmBackend>, model: String) -> Self;
    pub fn from_env(cache_path: std::path::PathBuf) -> anyhow::Result<Self>; // ureq backend, env keys
}
impl Enricher for CachedEnricher { ... }     // cache hit => 0 api_calls; miss => 1 api_calls
pub fn load_cache(path: &std::path::Path) -> anyhow::Result<EnrichCache>;
pub fn write_cache(path: &std::path::Path, cache: &EnrichCache) -> anyhow::Result<()>;
```

Cache key = `fn.ast_hash`. A cache hit must overwrite description, step titles/details and parameter defaults from the entry.

## 5. `fnscope-server` (J06 owns whole crate)

```rust
pub enum InvokeMode { Simulate, Node }
pub struct ServerConfig { pub manifest_path: std::path::PathBuf, pub bind: std::net::SocketAddr, pub invoke_mode: InvokeMode }
pub fn router(config: ServerConfig) -> axum::Router;  // testable without binding
pub async fn serve(config: ServerConfig) -> anyhow::Result<()>;
pub const INDEX_HTML: &str = include_str!("../../../ui/dist/index.html");
pub const APP_JS: &str = include_str!("../../../ui/dist/app.js");
pub const APP_CSS: &str = include_str!("../../../ui/dist/app.css");
```

Routes:
- `GET /` -> `INDEX_HTML` (`text/html`)
- `GET /app.js` -> `APP_JS` (`text/javascript`)
- `GET /app.css` -> `APP_CSS` (`text/css`)
- `GET /api/fn-graph.json` -> manifest JSON (404 if missing, never panic)
- `POST /api/invoke` -> JSON `InvokeRequest` -> `InvokeResponse`
- `GET /api/health` -> `{"ok":true,"tool":"fnscope"}`

`Simulate` mode builds one `TraceEntry` per step with `status: "simulated"` and `output: step.output`, plus `data: { simulated: true, function, inputs, expectedReturnType }`.
`Node` mode writes a temp runner script, spawns `node`, imports the target module by absolute file URL, calls the named export (class methods: `Class.method`), returns JSON `{ok,data,error,trace}`; on any failure falls back to Simulate and adds an `error` field describing why.

## 6. `fnscope-cli` (J07 owns whole crate)

Binary name `fnscope`. Clap derive (the derived top-level struct is named `FnScopeCli`):
```
fnscope scan <TARGET_DIR> [--out DIR] [--enrich] [--model NAME] [--include GLOB]... [--exclude GLOB]...
fnscope [TARGET_DIR] | fnscope open [TARGET_DIR] [--port 4310] [--host 127.0.0.1] [--no-watch] [--invoke node|simulate]
```
Default out `.fnscope`, default port 4310. `open` scans first, then serves; watcher polls a directory fingerprint every 500ms and rescans on change. Print concise progress lines. Exit non-zero with a readable error on failure.

## 7. Directories owned by orchestrator (do not edit)

- `Cargo.toml` (workspace), `docs/contract/rust-api.md`, `ui/dist/index.html`, `ui/dist/app.js`, `ui/dist/app.css` (placeholders), `crates/fnscope-parser/src/types.rs`, `crates/fnscope-parser/src/lib.rs`.
