//! Workspace scope registry: several independently scanned projects behind one local server.
//!
//! A scope is a manifest plus the roots needed to describe and invoke it. The scope id is a
//! whitelist key into this registry and is never joined into a filesystem path, so a request for
//! one scope cannot read another scope's manifest or source.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use axum::Router;
use fnscope_model::FnGraph;
use serde::{Deserialize, Serialize};

use crate::InvokeMode;

/// Scope id used by the implicit single-project server and by the compatibility routes.
pub const DEFAULT_SCOPE: &str = "default";

/// Longest accepted scope id; the id is a registry key, never a path component.
const MAX_SCOPE_ID_LEN: usize = 64;
/// Hard ceiling on a manifest read into memory before parsing.
pub(crate) const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
/// Hard ceiling on functions served from one manifest.
pub(crate) const MAX_FUNCTIONS: usize = 100_000;
/// Hard ceiling on distinct source files described from one manifest.
pub(crate) const MAX_FILES: usize = 100_000;
/// Most real Node invocations allowed to run at the same time.
pub(crate) const MAX_NODE_RUNS: usize = 4;
/// Body ceiling for invoke requests; axum refuses larger bodies before parsing them.
pub(crate) const MAX_INVOKE_BODY_BYTES: usize = 1024 * 1024;

/// One project scope the server knows how to read and invoke.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScopeConfig {
    /// Registry key; must match `[a-z0-9][a-z0-9-]*`.
    pub id: String,
    /// Human-readable project name shown by the scope switcher.
    #[serde(default)]
    pub label: String,
    /// Optional sentence explaining what the project is.
    #[serde(default)]
    pub note: String,
    /// Absolute (or CLI-resolved) project root used to contain Node module paths.
    #[serde(default)]
    pub scan_root: PathBuf,
    /// Relative root shown in the UI; absolute values are replaced with `.`.
    #[serde(default)]
    pub display_root: String,
    /// Manifest file read for this scope, re-read whenever its stamp changes.
    pub manifest_path: PathBuf,
    /// Preferred starting function; the first function is used when it is absent.
    #[serde(default)]
    pub default_function_id: Option<String>,
    /// Per-scope override of the workspace invoke capability.
    #[serde(default)]
    pub invoke_mode: Option<InvokeMode>,
}

impl ScopeConfig {
    /// Builds a scope with the required fields and neutral defaults.
    pub fn new(
        id: impl Into<String>,
        scan_root: impl Into<PathBuf>,
        manifest_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            id: id.into(),
            label: String::new(),
            note: String::new(),
            scan_root: scan_root.into(),
            display_root: String::new(),
            manifest_path: manifest_path.into(),
            default_function_id: None,
            invoke_mode: None,
        }
    }
}

/// Everything needed to serve a workspace: its scopes, default and listener.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConfig {
    /// Registry entries; invalid or duplicate ids are dropped by the router.
    #[serde(default)]
    pub scopes: Vec<ScopeConfig>,
    /// Scope served by `/api/fn-graph.json` and `/api/invoke`.
    pub default_scope: String,
    /// Socket address the HTTP listener binds to.
    pub bind: SocketAddr,
    /// Invocation strategy used when a scope does not override it.
    pub invoke_mode: InvokeMode,
}

impl WorkspaceConfig {
    /// Wraps one manifest in an implicit `default` scope, preserving single-project behavior.
    pub fn single(manifest_path: PathBuf, bind: SocketAddr, invoke_mode: InvokeMode) -> Self {
        Self {
            scopes: vec![ScopeConfig {
                id: DEFAULT_SCOPE.to_owned(),
                label: "Default".to_owned(),
                note: String::new(),
                scan_root: PathBuf::new(),
                display_root: ".".to_owned(),
                manifest_path,
                default_function_id: None,
                invoke_mode: Some(invoke_mode),
            }],
            default_scope: DEFAULT_SCOPE.to_owned(),
            bind,
            invoke_mode,
        }
    }
}

/// One entry of `GET /api/scopes`, using the relative display root only.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScopeDescriptor {
    /// Registry key the client sends back on scoped routes.
    pub id: String,
    /// Human-readable project name.
    pub label: String,
    /// Optional project note.
    pub note: String,
    /// Relative root shown in the UI; never an absolute path.
    pub root_dir: String,
    /// Manifest scan instant, or `null` when the manifest is unavailable.
    pub generated_at: Option<String>,
    /// Number of functions in the manifest.
    pub function_count: usize,
    /// Number of distinct source files in the manifest.
    pub file_count: usize,
    /// Per-language file counts, largest first.
    pub languages: Vec<LanguageCount>,
    /// Whether the manifest was read and validated.
    pub available: bool,
    /// Why the scope is unavailable, or `null` when it is healthy.
    pub error: Option<String>,
    /// Starting function chosen for this scope.
    pub default_function_id: Option<String>,
    /// Whether invocation replays steps or runs Node for this scope.
    pub invoke_mode: InvokeMode,
}

/// One language bucket in a scope descriptor.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LanguageCount {
    /// Display name such as `TypeScript`.
    pub name: String,
    /// Number of distinct files with that language.
    pub count: usize,
}

/// Response body of `GET /api/scopes`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScopeList {
    /// Descriptors in configured order.
    pub scopes: Vec<ScopeDescriptor>,
    /// Scope used by the compatibility routes.
    pub default_scope: String,
}

/// Reports whether an id is a safe registry key (`[a-z0-9][a-z0-9-]*`, at most 64 bytes).
pub fn is_valid_scope_id(id: &str) -> bool {
    if id.is_empty() || id.len() > MAX_SCOPE_ID_LEN {
        return false;
    }
    let mut bytes = id.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => return false,
    }
    bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Checks a workspace before binding, so a bad config fails at startup instead of per request.
pub fn validate_workspace_config(config: &WorkspaceConfig) -> Result<(), String> {
    if config.scopes.is_empty() {
        return Err("workspace must configure at least one scope".to_owned());
    }

    let mut seen: HashSet<&str> = HashSet::new();
    for scope in &config.scopes {
        if !is_valid_scope_id(&scope.id) {
            return Err(format!(
                "scope id {:?} must match [a-z0-9][a-z0-9-]*",
                scope.id
            ));
        }
        if !seen.insert(scope.id.as_str()) {
            return Err(format!("duplicate scope id: {}", scope.id));
        }
        if scope.manifest_path.as_os_str().is_empty() {
            return Err(format!("scope {} has no manifest path", scope.id));
        }
    }

    if !seen.contains(config.default_scope.as_str()) {
        return Err(format!(
            "default scope {:?} is not configured",
            config.default_scope
        ));
    }
    Ok(())
}

/// Cached validated graph plus the manifest stamp it was read from.
struct CacheEntry {
    stamp: ManifestStamp,
    graph: Arc<FnGraph>,
}

/// Identity of one manifest version; a rewrite or resize invalidates the cache entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ManifestStamp {
    modified: Option<SystemTime>,
    len: u64,
}

/// Why a scope could not be served; keeps error text safe to show a local user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ScopeError {
    /// The manifest file does not exist yet.
    Missing,
    /// The manifest is larger than the server accepts.
    TooLarge,
    /// The manifest could not be read at all.
    Unreadable(String),
    /// The manifest parsed but failed schema, validation or count limits.
    Invalid(String),
}

impl ScopeError {
    /// HTTP status used by graph routes; invoke keeps 200 with `ok:false`.
    pub(crate) fn status(&self) -> axum::http::StatusCode {
        match self {
            Self::Missing => axum::http::StatusCode::NOT_FOUND,
            Self::TooLarge | Self::Unreadable(_) | Self::Invalid(_) => {
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    /// User-facing explanation with no source text and no absolute path.
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Missing => "manifest not found — run fnscope scan first".to_owned(),
            Self::TooLarge => format!(
                "manifest exceeds the {} MiB limit",
                MAX_MANIFEST_BYTES / (1024 * 1024)
            ),
            Self::Unreadable(reason) => format!("cannot read manifest: {reason}"),
            Self::Invalid(reason) => reason.clone(),
        }
    }
}

/// The per-server scope registry and its validated-manifest cache.
pub(crate) struct ScopeRegistry {
    scopes: Vec<ScopeConfig>,
    default_scope: String,
    workspace_invoke_mode: InvokeMode,
    cache: Mutex<HashMap<String, CacheEntry>>,
}

impl ScopeRegistry {
    /// Drops invalid and duplicate ids so the router stays total on a hand-written config.
    pub(crate) fn new(config: WorkspaceConfig) -> Self {
        let mut seen = HashSet::new();
        let mut scopes = Vec::new();
        for scope in config.scopes {
            if is_valid_scope_id(&scope.id) && seen.insert(scope.id.clone()) {
                scopes.push(scope);
            }
        }
        let default_scope = if scopes.iter().any(|scope| scope.id == config.default_scope) {
            config.default_scope
        } else {
            scopes
                .first()
                .map(|scope| scope.id.clone())
                .unwrap_or_else(|| DEFAULT_SCOPE.to_owned())
        };

        Self {
            scopes,
            default_scope,
            workspace_invoke_mode: config.invoke_mode,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Scopes in configured order, used to build the list response.
    pub(crate) fn scopes(&self) -> &[ScopeConfig] {
        &self.scopes
    }

    /// Scope served by the compatibility routes.
    pub(crate) fn default_scope(&self) -> &str {
        &self.default_scope
    }

    /// Looks up a scope by its whitelisted id; never treats the id as a path.
    pub(crate) fn scope(&self, id: &str) -> Option<&ScopeConfig> {
        self.scopes.iter().find(|scope| scope.id == id)
    }

    /// Capability for one scope, falling back to the workspace-wide mode.
    pub(crate) fn invoke_mode(&self, scope: &ScopeConfig) -> InvokeMode {
        scope.invoke_mode.unwrap_or(self.workspace_invoke_mode)
    }

    /// Builds the safe public view of a scope, converting load failures to `available:false`.
    pub(crate) async fn descriptor(&self, scope: &ScopeConfig) -> ScopeDescriptor {
        let invoke_mode = self.invoke_mode(scope);
        let id = scope.id.clone();
        let label = if scope.label.trim().is_empty() {
            id.clone()
        } else {
            scope.label.clone()
        };
        let note = scope.note.clone();
        let root_dir = relative_display_root(&scope.display_root);

        match self.load(scope).await {
            Ok(graph) => {
                let (file_count, languages) = file_metrics(&graph);
                let default_function_id = scope
                    .default_function_id
                    .as_ref()
                    .filter(|wanted| {
                        graph
                            .functions
                            .iter()
                            .any(|function| &function.id == *wanted)
                    })
                    .cloned()
                    .or_else(|| graph.functions.first().map(|function| function.id.clone()));

                ScopeDescriptor {
                    id,
                    label,
                    note,
                    root_dir,
                    generated_at: Some(graph.generated_at.clone()),
                    function_count: graph.functions.len(),
                    file_count,
                    languages,
                    available: true,
                    error: None,
                    default_function_id,
                    invoke_mode,
                }
            }
            Err(error) => ScopeDescriptor {
                id,
                label,
                note,
                root_dir,
                generated_at: None,
                function_count: 0,
                file_count: 0,
                languages: Vec::new(),
                available: false,
                error: Some(error.message()),
                default_function_id: None,
                invoke_mode,
            },
        }
    }

    /// Returns the validated graph, re-reading only when the manifest stamp changed.
    pub(crate) async fn load(&self, scope: &ScopeConfig) -> Result<Arc<FnGraph>, ScopeError> {
        let before = manifest_stamp(&scope.manifest_path).await?;
        if before.len > MAX_MANIFEST_BYTES {
            return Err(ScopeError::TooLarge);
        }
        if let Some(graph) = self.cached(&scope.id, before) {
            return Ok(graph);
        }

        let raw = match tokio::fs::read_to_string(&scope.manifest_path).await {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ScopeError::Missing)
            }
            Err(error) => {
                return Err(ScopeError::Unreadable(error.to_string()));
            }
        };
        if raw.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(ScopeError::TooLarge);
        }

        let graph = tokio::task::spawn_blocking(move || parse_and_validate(&raw))
            .await
            .map_err(|error| ScopeError::Invalid(format!("manifest task failed: {error}")))??;
        let graph = Arc::new(graph);

        // Only cache when the file did not change while it was being read.
        if let Ok(after) = manifest_stamp(&scope.manifest_path).await {
            if after == before {
                let mut cache = self.lock_cache();
                cache.insert(
                    scope.id.clone(),
                    CacheEntry {
                        stamp: before,
                        graph: Arc::clone(&graph),
                    },
                );
            }
        }

        Ok(graph)
    }

    /// Clones a cached graph when its stamp still matches.
    fn cached(&self, id: &str, stamp: ManifestStamp) -> Option<Arc<FnGraph>> {
        let cache = self.lock_cache();
        cache
            .get(id)
            .filter(|entry| entry.stamp == stamp)
            .map(|entry| Arc::clone(&entry.graph))
    }

    /// Recovers the cache from a poisoned lock instead of failing every later request.
    fn lock_cache(&self) -> std::sync::MutexGuard<'_, HashMap<String, CacheEntry>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Reads a manifest's stamp, distinguishing a missing file from an unreadable one.
async fn manifest_stamp(path: &Path) -> Result<ManifestStamp, ScopeError> {
    match tokio::fs::metadata(path).await {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(ScopeError::Invalid(
                    "manifest path is not a regular file".to_owned(),
                ));
            }
            Ok(ManifestStamp {
                modified: metadata.modified().ok(),
                len: metadata.len(),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(ScopeError::Missing),
        Err(error) => Err(ScopeError::Unreadable(error.to_string())),
    }
}

/// Parses, validates and bounds one manifest; runs on a blocking thread.
fn parse_and_validate(raw: &str) -> Result<FnGraph, ScopeError> {
    let graph: FnGraph = serde_json::from_str(raw)
        .map_err(|error| ScopeError::Invalid(format!("invalid manifest JSON: {error}")))?;
    graph
        .validate()
        .map_err(|error| ScopeError::Invalid(format!("invalid manifest: {error}")))?;

    if graph.functions.len() > MAX_FUNCTIONS {
        return Err(ScopeError::Invalid(format!(
            "manifest has more than {MAX_FUNCTIONS} functions"
        )));
    }
    let mut files: HashSet<&str> = HashSet::new();
    for function in &graph.functions {
        files.insert(function.file_path.as_str());
    }
    if files.len() > MAX_FILES {
        return Err(ScopeError::Invalid(format!(
            "manifest has more than {MAX_FILES} distinct files"
        )));
    }
    Ok(graph)
}

/// Keeps only a relative display root; anything absolute or escaping becomes `.`.
fn relative_display_root(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return ".".to_owned();
    }
    let looks_absolute = trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed.get(1..2).is_some_and(|second| second == ":");
    if looks_absolute {
        return ".".to_owned();
    }
    let path = Path::new(trimmed);
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return ".".to_owned();
    }
    trimmed.replace('\\', "/")
}

/// Counts distinct files and groups them by language.
fn file_metrics(graph: &FnGraph) -> (usize, Vec<LanguageCount>) {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for function in &graph.functions {
        if !seen.insert(function.file_path.as_str()) {
            continue;
        }
        *counts
            .entry(language_name(&function.file_path))
            .or_insert(0) += 1;
    }

    let mut languages: Vec<LanguageCount> = counts
        .into_iter()
        .map(|(name, count)| LanguageCount {
            name: name.to_owned(),
            count,
        })
        .collect();
    languages.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.name.cmp(&right.name))
    });
    (seen.len(), languages)
}

/// Maps a file extension to the language label shown in the scope card.
fn language_name(file_path: &str) -> &'static str {
    let extension = Path::new(file_path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "ts" | "mts" | "cts" => "TypeScript",
        "tsx" => "TSX",
        "js" | "mjs" | "cjs" => "JavaScript",
        "jsx" => "JSX",
        "go" => "Go",
        "py" => "Python",
        "rs" => "Rust",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "cs" => "C#",
        "c" | "h" => "C",
        "cc" | "cpp" | "cxx" | "hpp" => "C++",
        "rb" => "Ruby",
        "php" => "PHP",
        "swift" => "Swift",
        "scala" => "Scala",
        "dart" => "Dart",
        "lua" => "Lua",
        "sh" | "bash" => "Shell",
        "vue" => "Vue",
        "svelte" => "Svelte",
        _ => "Other",
    }
}

/// Builds a router for a workspace; invalid scope entries are ignored rather than panicking.
pub fn workspace_router(config: WorkspaceConfig) -> Router {
    let state = Arc::new(crate::AppState::new(config));
    crate::build_router(state)
}

/// Validates the workspace, binds its listener and serves until stopped.
pub async fn serve_workspace(config: WorkspaceConfig) -> anyhow::Result<()> {
    validate_workspace_config(&config).map_err(anyhow::Error::msg)?;
    let bind = config.bind;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    println!("fnscope UI on http://{bind}");
    axum::serve(listener, workspace_router(config)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use axum::body::{to_bytes, Body};
    use axum::http::{header, Request, StatusCode};
    use axum::response::Response;
    use axum::Router;
    use fnscope_model::{FnRecord, ReturnShape, Step, StepKind};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use super::*;

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    /// Removes a uniquely named scratch directory even when a test fails.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "fnscope-scope-{}-{}-{}",
                std::process::id(),
                nanos,
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).expect("scratch directory must be creatable");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Builds one valid function record so graphs pass `FnGraph::validate`.
    fn record(id: &str, name: &str, file_path: &str) -> FnRecord {
        FnRecord {
            id: id.to_owned(),
            name: name.to_owned(),
            module: file_path.to_owned(),
            file_path: file_path.to_owned(),
            line: 1,
            ast_hash: format!("sha256:{}", "a".repeat(64)),
            description: format!("{name} description"),
            returns: ReturnShape {
                type_: "object".to_owned(),
                fields: Vec::new(),
            },
            steps: vec![
                Step {
                    id: "step-1".to_owned(),
                    kind: StepKind::Transform,
                    title: "read input".to_owned(),
                    code: "const value = args.x".to_owned(),
                    output: "value".to_owned(),
                    ..Default::default()
                },
                Step {
                    id: "step-2".to_owned(),
                    kind: StepKind::Return,
                    title: "return value".to_owned(),
                    code: "return value".to_owned(),
                    output: "result".to_owned(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    /// Builds a graph with a single named function.
    fn graph(generated_at: &str, functions: Vec<FnRecord>) -> FnGraph {
        FnGraph {
            tool: "fnscope".to_owned(),
            schema_version: "1.0.0".to_owned(),
            generated_at: generated_at.to_owned(),
            root_dir: ".".to_owned(),
            functions,
        }
    }

    /// Writes a manifest into the scope's own directory and returns its path.
    fn write_manifest(root: &Path, id: &str, graph: &FnGraph) -> PathBuf {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).expect("scope directory must be creatable");
        let path = dir.join("fn-graph.json");
        std::fs::write(&path, serde_json::to_vec(graph).expect("graph serializes"))
            .expect("manifest must be writable");
        path
    }

    /// Builds a scope config rooted at the scratch directory with a relative display root.
    fn scope(id: &str, root: &Path, manifest: PathBuf, mode: InvokeMode) -> ScopeConfig {
        ScopeConfig {
            id: id.to_owned(),
            label: format!("{id} project"),
            note: format!("{id} note"),
            scan_root: root.join(id),
            display_root: id.to_owned(),
            manifest_path: manifest,
            default_function_id: None,
            invoke_mode: Some(mode),
        }
    }

    fn workspace(scopes: Vec<ScopeConfig>, default_scope: &str) -> WorkspaceConfig {
        WorkspaceConfig {
            scopes,
            default_scope: default_scope.to_owned(),
            bind: "127.0.0.1:0".parse().expect("valid socket address"),
            invoke_mode: InvokeMode::Simulate,
        }
    }

    /// Sends a request through the router without opening a socket.
    async fn call(app: Router, request: Request<Body>) -> Response {
        app.oneshot(request).await.expect("router must not fail")
    }

    fn get(uri: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("valid request")
    }

    fn post_invoke(uri: &str, fn_id: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({ "fnId": fn_id, "args": { "x": 1 } }).to_string(),
            ))
            .expect("valid invoke request")
    }

    async fn body_json(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body must be readable");
        serde_json::from_slice(&bytes).expect("body must be JSON")
    }

    fn two_scopes() -> (TempDir, WorkspaceConfig) {
        let temp = TempDir::new();
        let alpha = write_manifest(
            temp.path(),
            "alpha",
            &graph(
                "2026-11-25T10:00:00Z",
                vec![record("shared-fn", "alphaFn", "src/alpha.ts")],
            ),
        );
        let beta = write_manifest(
            temp.path(),
            "beta",
            &graph(
                "2026-11-25T11:00:00Z",
                vec![record("shared-fn", "betaFn", "src/beta.ts")],
            ),
        );
        let config = workspace(
            vec![
                scope("alpha", temp.path(), alpha, InvokeMode::Simulate),
                scope("beta", temp.path(), beta, InvokeMode::Simulate),
            ],
            "alpha",
        );
        (temp, config)
    }

    #[test]
    fn scope_ids_are_a_strict_whitelist() {
        for valid in ["default", "a", "scope-1", "a0-", "0alpha"] {
            assert!(is_valid_scope_id(valid), "{valid:?} must be valid");
        }
        for invalid in [
            "",
            "Alpha",
            "a_b",
            "-alpha",
            "alpha.",
            "../escape",
            "a/b",
            "a b",
            "a\0b",
        ] {
            assert!(!is_valid_scope_id(invalid), "{invalid:?} must be rejected");
        }
        assert!(!is_valid_scope_id(&"a".repeat(65)));
        assert!(is_valid_scope_id(&"a".repeat(64)));
    }

    #[test]
    fn display_roots_never_expose_absolute_paths() {
        assert_eq!(relative_display_root("alpha"), "alpha");
        assert_eq!(relative_display_root("nested\\project"), "nested/project");
        assert_eq!(relative_display_root(""), ".");
        assert_eq!(relative_display_root("/etc/passwd"), ".");
        assert_eq!(relative_display_root("C:\\Users\\me"), ".");
        assert_eq!(relative_display_root("../outside"), ".");
    }

    #[tokio::test]
    async fn scope_list_uses_relative_roots_and_metadata() {
        let temp = TempDir::new();
        let manifest = write_manifest(
            temp.path(),
            "alpha",
            &graph(
                "2026-11-25T10:00:00Z",
                vec![
                    record("one-fn", "one", "src/one.ts"),
                    record("two-fn", "two", "src/two.ts"),
                    record("three-fn", "three", "src/three.tsx"),
                ],
            ),
        );
        let app = workspace_router(workspace(
            vec![scope("alpha", temp.path(), manifest, InvokeMode::Simulate)],
            "alpha",
        ));

        let response = call(app, get("/api/scopes")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;

        assert_eq!(body["defaultScope"], "alpha");
        let scopes = body["scopes"].as_array().expect("scope array");
        assert_eq!(scopes.len(), 1);
        let alpha = &scopes[0];
        assert_eq!(alpha["id"], "alpha");
        assert_eq!(alpha["label"], "alpha project");
        assert_eq!(alpha["note"], "alpha note");
        assert_eq!(alpha["rootDir"], "alpha");
        assert_eq!(alpha["generatedAt"], "2026-11-25T10:00:00Z");
        assert_eq!(alpha["functionCount"], 3);
        assert_eq!(alpha["fileCount"], 3);
        assert_eq!(alpha["available"], true);
        assert!(alpha["error"].is_null());
        assert_eq!(alpha["defaultFunctionId"], "one-fn");
        assert_eq!(alpha["invokeMode"], "simulate");
        assert_eq!(alpha["languages"][0]["name"], "TypeScript");
        assert_eq!(alpha["languages"][0]["count"], 2);
        assert_eq!(alpha["languages"][1]["name"], "TSX");
        assert!(
            !alpha["rootDir"]
                .as_str()
                .expect("root is text")
                .contains('\\'),
            "display root must be relative POSIX text"
        );
    }

    #[tokio::test]
    async fn scoped_graphs_isolate_a_shared_function_id() {
        let (_temp, config) = two_scopes();
        let app = workspace_router(config);

        let alpha =
            body_json(call(app.clone(), get("/api/scopes/alpha/fn-graph.json")).await).await;
        let beta = body_json(call(app.clone(), get("/api/scopes/beta/fn-graph.json")).await).await;
        assert_eq!(alpha["functions"][0]["name"], "alphaFn");
        assert_eq!(beta["functions"][0]["name"], "betaFn");
        assert_eq!(alpha["functions"][0]["id"], "shared-fn");
        assert_eq!(beta["functions"][0]["id"], "shared-fn");

        // The default compatibility route serves alpha, the configured default scope.
        let default_graph = body_json(call(app.clone(), get("/api/fn-graph.json")).await).await;
        assert_eq!(default_graph["functions"][0]["name"], "alphaFn");

        let alpha_invoke = body_json(
            call(
                app.clone(),
                post_invoke("/api/scopes/alpha/invoke", "shared-fn"),
            )
            .await,
        )
        .await;
        let beta_invoke = body_json(
            call(
                app.clone(),
                post_invoke("/api/scopes/beta/invoke", "shared-fn"),
            )
            .await,
        )
        .await;
        assert_eq!(alpha_invoke["ok"], true);
        assert_eq!(alpha_invoke["data"]["function"], "alphaFn");
        assert_eq!(beta_invoke["data"]["function"], "betaFn");

        let default_invoke =
            body_json(call(app.clone(), post_invoke("/api/invoke", "shared-fn")).await).await;
        assert_eq!(default_invoke["data"]["function"], "alphaFn");

        // An id that only exists in beta must not be found from alpha.
        let cross =
            body_json(call(app, post_invoke("/api/scopes/alpha/invoke", "beta-only")).await).await;
        assert_eq!(cross["ok"], false);
        assert!(cross["error"]
            .as_str()
            .expect("error text")
            .contains("unknown fnId"));
    }

    #[tokio::test]
    async fn unknown_and_corrupt_scopes_do_not_break_others() {
        let temp = TempDir::new();
        let alpha_manifest = write_manifest(
            temp.path(),
            "alpha",
            &graph(
                "2026-11-25T10:00:00Z",
                vec![record("alpha-fn", "alpha", "src/a.ts")],
            ),
        );
        let broken_dir = temp.path().join("broken");
        std::fs::create_dir_all(&broken_dir).expect("broken directory");
        let broken_manifest = broken_dir.join("fn-graph.json");
        std::fs::write(&broken_manifest, "{ not json").expect("corrupt manifest");

        let app = workspace_router(workspace(
            vec![
                scope("alpha", temp.path(), alpha_manifest, InvokeMode::Simulate),
                scope("broken", temp.path(), broken_manifest, InvokeMode::Simulate),
                scope(
                    "missing",
                    temp.path(),
                    temp.path().join("missing").join("fn-graph.json"),
                    InvokeMode::Simulate,
                ),
            ],
            "alpha",
        ));

        let list = body_json(call(app.clone(), get("/api/scopes")).await).await;
        let scopes = list["scopes"].as_array().expect("scope array");
        assert_eq!(scopes.len(), 3);
        assert_eq!(scopes[0]["available"], true);
        assert_eq!(scopes[0]["functionCount"], 1);
        assert_eq!(scopes[1]["available"], false);
        assert_eq!(scopes[1]["functionCount"], 0);
        assert!(scopes[1]["error"]
            .as_str()
            .expect("error text")
            .contains("invalid manifest"));
        assert_eq!(scopes[2]["available"], false);
        assert!(scopes[2]["error"]
            .as_str()
            .expect("error text")
            .contains("manifest not found"));

        assert_eq!(
            call(app.clone(), get("/api/scopes/alpha/fn-graph.json"))
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            call(app.clone(), get("/api/scopes/broken/fn-graph.json"))
                .await
                .status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            call(app.clone(), get("/api/scopes/missing/fn-graph.json"))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(app.clone(), get("/api/scopes/unknown/fn-graph.json"))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(app, get("/api/scopes/unknown/invoke")).await.status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
    }

    #[tokio::test]
    async fn unknown_scope_invoke_is_404() {
        let (_temp, config) = two_scopes();
        let app = workspace_router(config);
        let response = call(app, post_invoke("/api/scopes/nope/invoke", "shared-fn")).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = body_json(response).await;
        assert!(body["error"]
            .as_str()
            .expect("error text")
            .contains("unknown scope"));
    }

    #[tokio::test]
    async fn cache_invalidates_when_manifest_stamp_changes() {
        let temp = TempDir::new();
        let manifest = write_manifest(
            temp.path(),
            "alpha",
            &graph(
                "2026-11-25T10:00:00Z",
                vec![record("one-fn", "one", "src/one.ts")],
            ),
        );
        let app = workspace_router(workspace(
            vec![scope(
                "alpha",
                temp.path(),
                manifest.clone(),
                InvokeMode::Simulate,
            )],
            "alpha",
        ));

        let first = body_json(call(app.clone(), get("/api/scopes")).await).await;
        assert_eq!(first["scopes"][0]["generatedAt"], "2026-11-25T10:00:00Z");
        assert_eq!(first["scopes"][0]["functionCount"], 1);

        // A longer manifest changes the stamp even on filesystems with coarse mtimes.
        let replacement = graph(
            "2026-11-26T12:30:00Z",
            vec![
                record("one-fn", "one", "src/one.ts"),
                record("two-fn", "two", "src/two.ts"),
            ],
        );
        std::fs::write(
            &manifest,
            serde_json::to_vec_pretty(&replacement).expect("graph serializes"),
        )
        .expect("manifest must be rewritten");

        let second = body_json(call(app.clone(), get("/api/scopes")).await).await;
        assert_eq!(second["scopes"][0]["generatedAt"], "2026-11-26T12:30:00Z");
        assert_eq!(second["scopes"][0]["functionCount"], 2);

        let graph_body = body_json(call(app, get("/api/scopes/alpha/fn-graph.json")).await).await;
        let names: Vec<&str> = graph_body["functions"]
            .as_array()
            .expect("functions")
            .iter()
            .map(|function| function["name"].as_str().expect("name"))
            .collect();
        assert_eq!(names, ["one", "two"]);
    }

    #[tokio::test]
    async fn d8_validation_rejects_a_semantically_invalid_manifest() {
        let temp = TempDir::new();
        let mut invalid = graph(
            "2026-11-25T10:00:00Z",
            vec![record("bad-fn", "bad", "src/bad.ts")],
        );
        invalid.tool = "foreign-tool".to_owned();
        let manifest = write_manifest(temp.path(), "alpha", &invalid);

        let app = workspace_router(workspace(
            vec![scope("alpha", temp.path(), manifest, InvokeMode::Simulate)],
            "alpha",
        ));

        let list = body_json(call(app.clone(), get("/api/scopes")).await).await;
        assert_eq!(list["scopes"][0]["available"], false);
        let error = list["scopes"][0]["error"].as_str().expect("error text");
        assert!(
            error.contains("tool"),
            "error must name the validation gap: {error}"
        );

        for uri in ["/api/scopes/alpha/fn-graph.json", "/api/fn-graph.json"] {
            let response = call(app.clone(), get(uri)).await;
            assert_eq!(
                response.status(),
                StatusCode::INTERNAL_SERVER_ERROR,
                "route {uri} must reject an unvalidated graph"
            );
            let body = body_json(response).await;
            assert!(body["error"].as_str().expect("error text").contains("tool"));
        }

        let invoke = body_json(call(app, post_invoke("/api/invoke", "bad-fn")).await).await;
        assert_eq!(invoke["ok"], false);
        assert!(invoke["error"]
            .as_str()
            .expect("error text")
            .contains("tool"));
    }

    #[tokio::test]
    async fn traversal_shaped_scope_ids_are_never_routed() {
        let (_temp, config) = two_scopes();
        let app = workspace_router(config);

        for uri in [
            "/api/scopes/..%2Fescape/fn-graph.json",
            "/api/scopes/%2E%2E/fn-graph.json",
            "/api/scopes/Alpha/fn-graph.json",
            "/api/scopes/a_b/fn-graph.json",
        ] {
            let response = call(app.clone(), get(uri)).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "route {uri}");
        }

        // A config with an invalid id is dropped instead of becoming a path.
        let temp = TempDir::new();
        let manifest = write_manifest(
            temp.path(),
            "alpha",
            &graph(
                "2026-11-25T10:00:00Z",
                vec![record("a-fn", "a", "src/a.ts")],
            ),
        );
        let mut bad = scope("alpha", temp.path(), manifest.clone(), InvokeMode::Simulate);
        bad.id = "../escape".to_owned();
        let app = workspace_router(workspace(
            vec![
                bad,
                scope("alpha", temp.path(), manifest, InvokeMode::Simulate),
            ],
            "alpha",
        ));
        let list = body_json(call(app, get("/api/scopes")).await).await;
        let ids: Vec<&str> = list["scopes"]
            .as_array()
            .expect("scope array")
            .iter()
            .map(|scope| scope["id"].as_str().expect("id"))
            .collect();
        assert_eq!(ids, ["alpha"]);
    }

    #[tokio::test]
    async fn node_module_paths_are_contained_by_the_scope_root() {
        let temp = TempDir::new();
        let root = temp.path().join("alpha");
        std::fs::create_dir_all(&root).expect("scope root");
        std::fs::write(
            root.join("..").join("outside.mjs"),
            "export function escape() {}\n",
        )
        .expect("outside module");

        let mut escaping = graph(
            "2026-11-25T10:00:00Z",
            vec![record("escape-fn", "escape", "../outside.mjs")],
        );
        escaping.root_dir = root.to_string_lossy().into_owned();
        let manifest = write_manifest(temp.path(), "alpha", &escaping);

        let app = workspace_router(workspace(
            vec![scope("alpha", temp.path(), manifest, InvokeMode::Node)],
            "alpha",
        ));
        let body =
            body_json(call(app, post_invoke("/api/scopes/alpha/invoke", "escape-fn")).await).await;
        assert_eq!(
            body["ok"], true,
            "simulate fallback keeps the replay payload"
        );
        let error = body["error"].as_str().expect("fallback error");
        assert!(
            error.contains("escapes the scope root"),
            "traversal must be refused before spawning Node: {error}"
        );
    }

    #[tokio::test]
    async fn concurrent_requests_stay_isolated() {
        let (_temp, config) = two_scopes();
        let app = workspace_router(config);
        let mut tasks = Vec::new();
        for index in 0..24 {
            let app = app.clone();
            tasks.push(tokio::spawn(async move {
                if index % 2 == 0 {
                    let body =
                        body_json(call(app, get("/api/scopes/alpha/fn-graph.json")).await).await;
                    body["functions"][0]["name"].as_str().map(str::to_owned)
                } else {
                    let body =
                        body_json(call(app, get("/api/scopes/beta/fn-graph.json")).await).await;
                    body["functions"][0]["name"].as_str().map(str::to_owned)
                }
            }));
        }
        for (index, task) in tasks.into_iter().enumerate() {
            let name = task.await.expect("task must not panic").expect("name");
            let expected = if index % 2 == 0 { "alphaFn" } else { "betaFn" };
            assert_eq!(name, expected);
        }
    }

    #[tokio::test]
    async fn foreign_host_and_origin_are_rejected() {
        let (_temp, config) = two_scopes();
        let app = workspace_router(config);

        let foreign_host = Request::builder()
            .uri("/api/health")
            .header(header::HOST, "evil.example")
            .body(Body::empty())
            .expect("request");
        assert_eq!(
            call(app.clone(), foreign_host).await.status(),
            StatusCode::BAD_REQUEST
        );

        let local_host = Request::builder()
            .uri("/api/health")
            .header(header::HOST, "localhost:4310")
            .body(Body::empty())
            .expect("request");
        assert_eq!(call(app.clone(), local_host).await.status(), StatusCode::OK);

        let foreign_origin = Request::builder()
            .uri("/api/health")
            .header(header::ORIGIN, "http://evil.example")
            .body(Body::empty())
            .expect("request");
        assert_eq!(
            call(app.clone(), foreign_origin).await.status(),
            StatusCode::FORBIDDEN
        );

        let local_origin = Request::builder()
            .uri("/api/health")
            .header(header::ORIGIN, "http://localhost:4310")
            .body(Body::empty())
            .expect("request");
        assert_eq!(
            call(app.clone(), local_origin).await.status(),
            StatusCode::OK
        );

        let cross_site = Request::builder()
            .uri("/api/health")
            .header("sec-fetch-site", "cross-site")
            .body(Body::empty())
            .expect("request");
        assert_eq!(call(app, cross_site).await.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn embedded_assets_carry_a_content_security_policy() {
        let (_temp, config) = two_scopes();
        let app = workspace_router(config);
        for uri in ["/", "/app.js", "/app.css"] {
            let response = call(app.clone(), get(uri)).await;
            assert_eq!(response.status(), StatusCode::OK, "route {uri}");
            let policy = response
                .headers()
                .get("content-security-policy")
                .expect("CSP header")
                .to_str()
                .expect("ASCII policy");
            assert!(
                policy.contains("default-src 'self'"),
                "route {uri}: {policy}"
            );
            assert!(
                policy.contains("frame-ancestors 'none'"),
                "route {uri}: {policy}"
            );
        }
    }

    #[tokio::test]
    async fn invoke_body_is_size_limited() {
        let (_temp, config) = two_scopes();
        let app = workspace_router(config);
        let oversized = "x".repeat(MAX_INVOKE_BODY_BYTES + 16);
        let request = Request::builder()
            .method("POST")
            .uri("/api/scopes/alpha/invoke")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(format!(
                "{{\"fnId\":\"shared-fn\",\"args\":\"{oversized}\"}}"
            )))
            .expect("request");
        let response = call(app, request).await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn strict_workspace_validation_names_the_problem() {
        let empty = workspace(Vec::new(), "default");
        assert!(validate_workspace_config(&empty).is_err());

        let mut duplicate = workspace(
            vec![
                ScopeConfig::new("alpha", ".", "a.json"),
                ScopeConfig::new("alpha", ".", "b.json"),
            ],
            "alpha",
        );
        assert!(validate_workspace_config(&duplicate).is_err());

        duplicate.scopes[1].id = "beta".to_owned();
        duplicate.default_scope = "gamma".to_owned();
        assert!(validate_workspace_config(&duplicate).is_err());

        duplicate.default_scope = "beta".to_owned();
        assert!(validate_workspace_config(&duplicate).is_ok());
    }

    #[test]
    fn language_labels_cover_the_scan_adapters() {
        assert_eq!(language_name("src/a.ts"), "TypeScript");
        assert_eq!(language_name("src/a.tsx"), "TSX");
        assert_eq!(language_name("src/a.js"), "JavaScript");
        assert_eq!(language_name("src/a.go"), "Go");
        assert_eq!(language_name("src/a.py"), "Python");
        assert_eq!(language_name("src/a.rs"), "Rust");
        assert_eq!(language_name("src/a.kts"), "Kotlin");
        assert_eq!(language_name("src/a.unknown"), "Other");
    }
}
