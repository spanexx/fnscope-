//! Local HTTP server that serves the embedded explorer UI and the JSON/invoke APIs.
//!
//! The manifest is re-read on every request so a file watcher's regenerated graph
//! becomes visible without restarting the server. All handlers are total: a missing
//! or corrupt manifest produces a JSON error response instead of a panic.

use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[cfg(test)]
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path as AxumPath, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use fnscope_model::{FnGraph, FnRecord, InvokeRequest, InvokeResponse, TraceEntry};
use serde_json::{json, Value};

mod workspace;

pub use workspace::{
    is_valid_scope_id, serve_workspace, validate_workspace_config, workspace_router, LanguageCount,
    ScopeConfig, ScopeDescriptor, ScopeList, WorkspaceConfig, DEFAULT_SCOPE,
};

/// Policy for the embedded shell and bundles; the UI is fully local and loads no remote asset.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; \
     style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; \
     connect-src 'self'; worker-src 'none'; base-uri 'none'; frame-ancestors 'none'; \
     form-action 'none'";

/// Explorer shell embedded at build time; the UI job replaces the placeholder file.
pub const INDEX_HTML: &str = include_str!("../../../ui/dist/index.html");
/// Pre-paint palette bootstrap; external so the CSP can stay inline-free.
pub const THEME_JS: &str = include_str!("../../../ui/dist/theme.js");
/// Bundled application script embedded at build time.
pub const APP_JS: &str = include_str!("../../../ui/dist/app.js");
/// Compiled stylesheet embedded at build time.
pub const APP_CSS: &str = include_str!("../../../ui/dist/app.css");

/// Chooses how `/api/invoke` answers: deterministic replay or a real Node process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvokeMode {
    /// Build a trace from the manifest's steps without executing any code.
    Simulate,
    /// Execute the named export in Node and report the observed result.
    Node,
}

/// Everything the orchestrating CLI hands to the server.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// Manifest path, re-read on every graph and invoke request.
    pub manifest_path: PathBuf,
    /// Socket address the HTTP listener binds to.
    pub bind: SocketAddr,
    /// Invocation strategy used by `/api/invoke`.
    pub invoke_mode: InvokeMode,
}

/// Handler state shared by all routes; cheaply cloned behind an `Arc`.
pub(crate) struct AppState {
    /// Scope registry plus its validated-manifest cache.
    registry: workspace::ScopeRegistry,
    /// Address the listener was configured with, used by the Host/Origin guard.
    bind: SocketAddr,
    /// Caps how many real Node processes one server may run concurrently.
    node_slots: Arc<tokio::sync::Semaphore>,
}

impl AppState {
    /// Builds state for a workspace, keeping the configured listener address for host checks.
    pub(crate) fn new(config: workspace::WorkspaceConfig) -> Self {
        let bind = config.bind;
        Self {
            registry: workspace::ScopeRegistry::new(config),
            bind,
            node_slots: Arc::new(tokio::sync::Semaphore::new(workspace::MAX_NODE_RUNS)),
        }
    }
}

/// Builds the complete route table with state attached, so tests need no socket.
pub fn router(config: ServerConfig) -> Router {
    let workspace =
        workspace::WorkspaceConfig::single(config.manifest_path, config.bind, config.invoke_mode);
    workspace::workspace_router(workspace)
}

/// Builds the shared route table for a workspace-backed state.
pub(crate) fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/theme.js", get(theme_js))
        .route("/app.js", get(app_js))
        .route("/app.css", get(app_css))
        .route("/api/health", get(health))
        .route("/api/scopes", get(scopes))
        .route("/api/scopes/{id}/fn-graph.json", get(scoped_fn_graph))
        .route("/api/scopes/{id}/invoke", post(scoped_invoke))
        .route("/api/fn-graph.json", get(fn_graph))
        .route("/api/invoke", post(invoke))
        .layer(DefaultBodyLimit::max(workspace::MAX_INVOKE_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            security_guard,
        ))
        .with_state(state)
}

/// Rejects foreign `Host`/`Origin` values so a browser cannot be rebound onto this server.
async fn security_guard(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(host) = request.headers().get(header::HOST) {
        let host = host.to_str().unwrap_or("");
        if !authority_allowed(host, state.bind) {
            return error_response(
                StatusCode::BAD_REQUEST,
                "host must name the local machine".to_owned(),
            );
        }
    }

    if let Some(origin) = request.headers().get(header::ORIGIN) {
        let origin = origin.to_str().unwrap_or("");
        if !origin_allowed(origin, state.bind) {
            return error_response(
                StatusCode::FORBIDDEN,
                "cross-origin requests are not allowed".to_owned(),
            );
        }
    }

    if let Some(site) = request.headers().get("sec-fetch-site") {
        if site.to_str().unwrap_or("") == "cross-site" {
            return error_response(
                StatusCode::FORBIDDEN,
                "cross-site requests are not allowed".to_owned(),
            );
        }
    }

    next.run(request).await
}

/// Reports whether a `Host`/authority names this machine.
fn authority_allowed(authority: &str, bind: SocketAddr) -> bool {
    let host = authority_host(authority);
    if is_loopback_host(host) {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(ip) => bind.ip().is_unspecified() || ip == bind.ip(),
        Err(_) => false,
    }
}

/// Extracts the host part of `host:port` or `[ipv6]:port`.
fn authority_host(authority: &str) -> &str {
    let authority = authority.trim();
    if let Some(rest) = authority.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(authority);
    }
    match authority.rsplit_once(':') {
        Some((host, port))
            if !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            host
        }
        _ => authority,
    }
}

/// Reports whether a host string is a loopback name or address.
fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

/// Reports whether an `Origin` header points back at this local server.
fn origin_allowed(origin: &str, bind: SocketAddr) -> bool {
    let rest = origin
        .trim()
        .strip_prefix("http://")
        .or_else(|| origin.trim().strip_prefix("https://"));
    match rest {
        Some(rest) => {
            let authority = rest.split('/').next().unwrap_or("");
            authority_allowed(authority, bind)
        }
        None => false,
    }
}

/// Serves the single-page application shell with a strict, local-only policy.
async fn index() -> Response {
    asset(INDEX_HTML, "text/html")
}

/// Serves the embedded JavaScript bundle as JavaScript.
async fn app_js() -> Response {
    asset(APP_JS, "text/javascript")
}

/// Serves the pre-paint palette bootstrap as JavaScript.
async fn theme_js() -> Response {
    asset(THEME_JS, "text/javascript")
}

/// Serves the embedded stylesheet as CSS.
async fn app_css() -> Response {
    asset(APP_CSS, "text/css")
}

/// Wraps a static asset with its narrow content type and the embedded-asset CSP.
fn asset(body: &'static str, content_type: &'static str) -> Response {
    (
        StatusCode::OK,
        [
            ("content-type", content_type),
            ("content-security-policy", CONTENT_SECURITY_POLICY),
        ],
        body,
    )
        .into_response()
}

/// Reports liveness for the CLI and for external health checks.
async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "tool": "fnscope" }))
}

/// Lists every configured scope with its availability and relative root.
async fn scopes(State(state): State<Arc<AppState>>) -> Json<workspace::ScopeList> {
    let mut descriptors = Vec::with_capacity(state.registry.scopes().len());
    for index in 0..state.registry.scopes().len() {
        let scope = &state.registry.scopes()[index];
        descriptors.push(state.registry.descriptor(scope).await);
    }
    Json(workspace::ScopeList {
        scopes: descriptors,
        default_scope: state.registry.default_scope().to_owned(),
    })
}

/// Compatibility route: serves the workspace's default scope graph.
async fn fn_graph(State(state): State<Arc<AppState>>) -> Response {
    let default_scope = state.registry.default_scope().to_owned();
    graph_for_scope(&state, &default_scope).await
}

/// Additive route: serves one whitelisted scope's validated graph.
async fn scoped_fn_graph(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !is_valid_scope_id(&id) {
        return unknown_scope(&id);
    }
    graph_for_scope(&state, &id).await
}

/// Loads and serializes a graph, validating it before any text reaches the UI (D-8).
async fn graph_for_scope(state: &AppState, scope_id: &str) -> Response {
    let Some(scope) = state.registry.scope(scope_id) else {
        return unknown_scope(scope_id);
    };
    match state.registry.load(scope).await {
        Ok(graph) => match serde_json::to_string(graph.as_ref()) {
            Ok(body) => {
                (StatusCode::OK, [("content-type", "application/json")], body).into_response()
            }
            Err(error) => error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("cannot encode manifest: {error}"),
            ),
        },
        Err(error) => error_response(error.status(), error.message()),
    }
}

/// Emits a JSON error body without ever panicking.
fn error_response(status: StatusCode, message: String) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

/// 404 for a scope id that is not in the registry; the id is never resolved as a path.
fn unknown_scope(id: &str) -> Response {
    error_response(StatusCode::NOT_FOUND, format!("unknown scope: {id}"))
}

/// Compatibility route: invokes against the workspace's default scope.
async fn invoke(
    State(state): State<Arc<AppState>>,
    Json(request): Json<InvokeRequest>,
) -> Response {
    let default_scope = state.registry.default_scope().to_owned();
    invoke_in_scope(&state, &default_scope, request).await
}

/// Additive route: invokes only inside one whitelisted scope.
async fn scoped_invoke(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(request): Json<InvokeRequest>,
) -> Response {
    if !is_valid_scope_id(&id) {
        return unknown_scope(&id);
    }
    invoke_in_scope(&state, &id, request).await
}

/// Dispatches an invocation to the simulator or to a bounded Node process.
async fn invoke_in_scope(state: &AppState, scope_id: &str, request: InvokeRequest) -> Response {
    let started = Instant::now();

    let Some(scope) = state.registry.scope(scope_id) else {
        return unknown_scope(scope_id);
    };
    let graph = match state.registry.load(scope).await {
        Ok(graph) => graph,
        Err(error) => {
            return Json(InvokeResponse {
                ok: false,
                error: Some(error.message()),
                duration_ms: started.elapsed().as_secs_f64() * 1000.0,
                ..Default::default()
            })
            .into_response();
        }
    };

    let simulated = simulate(&graph, &request);
    let mut response = match state.registry.invoke_mode(scope) {
        InvokeMode::Simulate => simulated.clone(),
        InvokeMode::Node => {
            // The permit is moved into the blocking task so the slot is released only
            // after the Node process tree has been waited for or timed out.
            let permit = match Arc::clone(&state.node_slots).try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    return Json(fallback(
                        simulated,
                        "too many concurrent Node invocations; try again",
                    ))
                    .into_response();
                }
            };
            let node_graph = Arc::clone(&graph);
            let node_request = request.clone();
            let scratch_base = scope
                .manifest_path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .map(Path::to_path_buf)
                .unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir())
                });
            let scan_root = scope.scan_root.clone();
            match tokio::task::spawn_blocking(move || {
                let _permit = permit;
                run_node(&node_graph, &node_request, &scratch_base, &scan_root)
            })
            .await
            {
                Ok(Ok(result)) => result,
                Ok(Err(reason)) => fallback(simulated.clone(), &reason),
                Err(join_error) => {
                    fallback(simulated.clone(), &format!("task panicked: {join_error}"))
                }
            }
        }
    };

    // Node's runner reports no per-step trace, so reuse the deterministic one for the UI.
    if response.trace.is_empty() {
        response.trace = simulated.trace.clone();
    }

    response.duration_ms = started.elapsed().as_secs_f64() * 1000.0;
    Json(response).into_response()
}

/// Builds a deterministic trace from the manifest's steps without executing code.
fn simulate(graph: &FnGraph, request: &InvokeRequest) -> InvokeResponse {
    let mut response = InvokeResponse::default();

    match graph
        .functions
        .iter()
        .find(|function| function.id == request.fn_id)
    {
        Some(function) => {
            for step in &function.steps {
                response.trace.insert(
                    step.id.clone(),
                    TraceEntry {
                        status: "simulated".to_owned(),
                        output: step.output.clone(),
                    },
                );
            }
            response.ok = true;
            response.data = Some(json!({
                "simulated": true,
                "function": &function.name,
                "inputs": &request.args,
                "expectedReturnType": &function.returns.type_,
            }));
        }
        None => {
            response.ok = false;
            response.error = Some(format!("unknown fnId: {}", request.fn_id));
        }
    }

    response
}

/// Keeps the simulated payload but records why real execution was unavailable.
fn fallback(mut simulated: InvokeResponse, reason: &str) -> InvokeResponse {
    let label = node_executable().to_string_lossy().into_owned();
    simulated.error = Some(format!("node '{label}' invocation failed: {reason}"));
    simulated
}

/// Prefers an explicit Node override so a local tool can target another Node without hardcoding a path.
fn node_executable() -> std::ffi::OsString {
    match std::env::var_os("FNSCOPE_NODE") {
        Some(value) if !value.is_empty() => value,
        _ => std::ffi::OsString::from("node"),
    }
}

/// Encodes text as a quoted JavaScript string literal for the generated runner.
fn js_string(value: &str) -> String {
    Value::String(value.to_owned()).to_string()
}

/// Removes its scratch directory on drop so failed Node runs never leak files.
struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    /// Creates scratch files beside the manifest so a Windows Node launched from WSL can open the paths handed to it via argv.
    fn create(base: &Path) -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = base.join(format!("fnscope-invoke-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        // A still-running child may hold a handle on Windows; best-effort is sufficient.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// JavaScript runner template; only the export name is embedded, paths arrive via argv.
const RUNNER_TEMPLATE: &str = r#"
import { pathToFileURL } from 'node:url';
import { readFileSync } from 'node:fs';

const MODULE_PATH = process.argv[3];
const FN_NAME = __FN_NAME__;
const ARGS_PATH = process.argv[2];
const PARAM_NAMES = __PARAM_NAMES__;

// Keep stdout reserved for the result envelope; all diagnostics go to stderr.
console.log = (...parts) => console.error(...parts);
console.info = (...parts) => console.error(...parts);

function emit(payload) {
  process.stdout.write(JSON.stringify(payload));
}

try {
  const args = JSON.parse(readFileSync(ARGS_PATH, 'utf8'));
  const imported = await import(pathToFileURL(MODULE_PATH).href);
  let fn;
  let receiver;
  if (FN_NAME.includes('.')) {
    const dot = FN_NAME.indexOf('.');
    const className = FN_NAME.slice(0, dot);
    const methodName = FN_NAME.slice(dot + 1);
    const ctor = imported[className] ?? imported.default?.[className];
    if (typeof ctor !== 'function') throw new Error('class export not found: ' + className);
    fn = ctor.prototype?.[methodName];
    if (typeof fn !== 'function') throw new Error('method not found: ' + FN_NAME);
    receiver = new ctor();
  } else {
    fn = imported[FN_NAME] ?? imported.default?.[FN_NAME];
    if (typeof fn !== 'function') throw new Error('export not found: ' + FN_NAME);
  }
  const values = PARAM_NAMES.map(name => (args && typeof args === 'object') ? args[name] : undefined);
  let result;
  if (PARAM_NAMES.length === 1) {
    const single = values[0] !== undefined ? values[0] : args;
    result = await fn.call(receiver, single);
  } else if (PARAM_NAMES.length > 1 && values.every(v => v !== undefined)) {
    result = await fn.apply(receiver, values);
  } else {
    result = await fn.call(receiver, args);
  }
  emit({ ok: true, data: result === undefined ? null : result, error: null, trace: {} });
} catch (error) {
  const message = error && error.message ? String(error.message) : String(error);
  emit({ ok: false, data: null, error: message, trace: {} });
}
"#;

/// Runs a command with captured output and a hard deadline, killing its process tree on timeout.
///
/// The caller must not remove scratch files until this returns: a timeout path waits for the
/// direct child to be reaped and signals the whole group before reporting the failure.
fn run_bounded(mut command: Command, timeout: Duration) -> Result<Output, String> {
    configure_process_group(&mut command);
    let mut child = command
        .spawn()
        .map_err(|err| format!("could not start: {err}"))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (stdout_tx, stdout_rx) = mpsc::channel();
    let (stderr_tx, stderr_rx) = mpsc::channel();
    let stdout_reader = std::thread::spawn(move || {
        let _ = stdout_tx.send(read_pipe(stdout));
    });
    let stderr_reader = std::thread::spawn(move || {
        let _ = stderr_tx.send(read_pipe(stderr));
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(err) => {
                terminate_process_tree(&mut child);
                return Err(format!("failed while waiting: {err}"));
            }
        }

        if Instant::now() >= deadline {
            terminate_process_tree(&mut child);
            // Reader threads unblock when the killed tree closes its pipe handles; they are
            // detached here so a stuck descendant cannot hold the request open.
            return Err(format!(
                "did not finish within {}s; its process tree was killed",
                timeout.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    // A descendant that inherited a pipe can outlive the direct child, so the output wait
    // shares the same deadline instead of joining reader threads without a bound.
    let stdout = receive_pipe(&stdout_rx, deadline, &mut child, timeout)?;
    let stderr = receive_pipe(&stderr_rx, deadline, &mut child, timeout)?;
    let _ = stdout_reader.join();
    let _ = stderr_reader.join();

    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// Receives one captured pipe before the shared deadline, killing the tree if it stalls.
fn receive_pipe(
    receiver: &mpsc::Receiver<Vec<u8>>,
    deadline: Instant,
    child: &mut Child,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let now = Instant::now();
    if now < deadline {
        match receiver.recv_timeout(deadline - now) {
            Ok(data) => return Ok(data),
            Err(_) => {}
        }
    }
    terminate_process_tree(child);
    Err(format!(
        "did not close its output within {}s; its process tree was killed",
        timeout.as_secs()
    ))
}

/// Reads one captured pipe to end without ever panicking in a helper thread.
fn read_pipe<R: Read + Send + 'static>(pipe: Option<R>) -> Vec<u8> {
    let mut buffer = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut buffer);
    }
    buffer
}

/// Gives the child its own process group so a timeout can signal every descendant.
#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    command.process_group(0);
}

/// Non-Unix platforms kill the tree through the platform helper instead.
#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

/// Kills the child and its descendants, then waits for the direct child to be reaped.
fn terminate_process_tree(child: &mut Child) {
    let pid = child.id();

    #[cfg(unix)]
    {
        // The leading dash targets the group created in `configure_process_group`.
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    // Even if the group helper is unavailable, stop and reap the direct child.
    let _ = child.kill();
    let _ = child.wait();

    #[cfg(unix)]
    wait_for_process_group(pid, Duration::from_secs(1));
}

/// Waits briefly for the killed group to disappear so scratch files are not removed early.
#[cfg(unix)]
fn wait_for_process_group(pid: u32, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let alive = Command::new("kill")
            .args(["-0", "--", &format!("-{pid}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !alive || Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Resolves a function's source file and refuses anything outside the scope root.
///
/// Canonicalization follows symlinks, so a manifest cannot point Node at a file outside the
/// project even if it contains absolute paths, `..` segments or links.
fn resolve_module_path(
    graph: &FnGraph,
    function: &FnRecord,
    scan_root: &Path,
) -> Result<PathBuf, String> {
    let file_path = Path::new(&function.file_path);
    if file_path.is_absolute() {
        return Err("module path must be relative to the scope root".to_owned());
    }

    let root = if scan_root.as_os_str().is_empty() {
        let recorded = PathBuf::from(&graph.root_dir);
        if recorded.is_absolute() {
            recorded
        } else {
            std::env::current_dir()
                .map_err(|err| format!("cannot read current directory: {err}"))?
                .join(recorded)
        }
    } else {
        scan_root.to_path_buf()
    };
    let root =
        std::fs::canonicalize(&root).map_err(|err| format!("cannot resolve scope root: {err}"))?;
    let candidate = root.join(file_path);
    let module =
        std::fs::canonicalize(&candidate).map_err(|err| format!("cannot resolve module: {err}"))?;
    if !module.starts_with(&root) {
        return Err("module path escapes the scope root".to_owned());
    }
    if !module.is_file() {
        return Err("module path is not a file".to_owned());
    }
    Ok(module)
}

/// Executes the target export in a Node child process and parses its JSON result.
fn run_node(
    graph: &FnGraph,
    request: &InvokeRequest,
    scratch_base: &Path,
    scan_root: &Path,
) -> Result<InvokeResponse, String> {
    let function = graph
        .functions
        .iter()
        .find(|function| function.id == request.fn_id)
        .ok_or_else(|| format!("unknown fnId: {}", request.fn_id))?;

    let module_path = resolve_module_path(graph, function, scan_root)?;

    let temp = TempDirGuard::create(scratch_base)
        .map_err(|err| format!("cannot create temp dir: {err}"))?;
    let args_path = temp.path.join("args.json");
    let runner_path = temp.path.join("runner.mjs");

    let args_bytes =
        serde_json::to_vec(&request.args).map_err(|err| format!("cannot encode args: {err}"))?;
    std::fs::write(&args_path, args_bytes)
        .map_err(|err| format!("cannot write args.json: {err}"))?;

    let param_names: Vec<String> = function.parameters.iter().map(|p| p.name.clone()).collect();
    let param_names_json = serde_json::to_string(&param_names).unwrap_or_else(|_| "[]".to_owned());
    let runner = RUNNER_TEMPLATE
        .replace("__FN_NAME__", &js_string(&function.name))
        .replace("__PARAM_NAMES__", &param_names_json);
    std::fs::write(&runner_path, runner)
        .map_err(|err| format!("cannot write runner.mjs: {err}"))?;

    let node = node_executable();
    let shown = node.to_string_lossy().into_owned();
    let mut command = Command::new(&node);
    command
        .arg("--experimental-strip-types")
        .arg(&runner_path)
        .arg(&args_path)
        .arg(&module_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // `run_bounded` only returns after the child is reaped, so the scratch directory
    // created above is never removed while the process tree can still read it.
    let output = run_bounded(command, Duration::from_secs(5))
        .map_err(|reason| format!("node '{shown}' {reason}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(format!(
            "node '{shown}' exited with {} ({})",
            output.status,
            stderr.trim()
        ));
    }

    // The runner emits only {ok,data,error,trace}, so supply the duration the
    // response schema requires; the handler replaces it with the real measurement.
    let mut envelope: Value = serde_json::from_str(&stdout).map_err(|err| {
        format!(
            "node '{shown}' returned invalid JSON ({err}): {}",
            stdout.trim()
        )
    })?;
    if envelope.get("durationMs").is_none() {
        envelope["durationMs"] = json!(0.0);
    }
    let mut response: InvokeResponse = serde_json::from_value(envelope)
        .map_err(|err| format!("node '{shown}' returned an invalid envelope: {err}"))?;

    // Even a graceful failure from the runner must name the executable for diagnosis.
    if !response.ok {
        let detail = response
            .error
            .take()
            .unwrap_or_else(|| "unknown error".to_owned());
        response.error = Some(format!("node '{shown}': {detail}"));
    }

    Ok(response)
}

/// Binds the listener and serves the router until the process is stopped.
pub async fn serve(config: ServerConfig) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    println!("fnscope UI on http://{}", config.bind);
    axum::serve(listener, router(config)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::{header, Request};
    use fnscope_model::{FnRecord, Parameter, Purity, ReturnShape, Step, StepKind};
    use serde_json::json;
    use tower::ServiceExt;

    /// Sends a request through the router without opening a socket.
    async fn call(app: Router, request: Request<Body>) -> Response {
        app.oneshot(request).await.expect("router must not fail")
    }

    /// Builds a config whose manifest path need not exist.
    fn config(manifest_path: PathBuf, mode: InvokeMode) -> ServerConfig {
        ServerConfig {
            manifest_path,
            bind: "127.0.0.1:0".parse().expect("valid socket address"),
            invoke_mode: mode,
        }
    }

    /// Returns a process-unique timestamp for scratch file names.
    fn stamp() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    }

    /// Reads a response body as UTF-8 text.
    async fn body_text(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body must be readable");
        String::from_utf8(bytes.to_vec()).expect("body must be UTF-8")
    }

    /// Sends a POST /api/invoke request with a JSON body.
    fn invoke_request(fn_id: &str, args: Value) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/api/invoke")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({ "fnId": fn_id, "args": args }).to_string(),
            ))
            .expect("valid invoke request")
    }

    /// Creates a two-step manifest in a unique temp file.
    fn write_sample_manifest() -> PathBuf {
        let graph = FnGraph {
            tool: "fnscope".to_owned(),
            schema_version: "1.0.0".to_owned(),
            generated_at: "2026-11-25T12:00:00Z".to_owned(),
            root_dir: ".".to_owned(),
            functions: vec![FnRecord {
                id: "echo-fn".to_owned(),
                name: "echo".to_owned(),
                module: "echo.js".to_owned(),
                file_path: "echo.js".to_owned(),
                line: 1,
                ast_hash: format!("sha256:{}", "a".repeat(64)),
                runtime: "node20".to_owned(),
                purity: Purity::Pure,
                timeout_ms: 1000,
                description: String::new(),
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
            }],
        };

        let path = std::env::temp_dir().join(format!(
            "fnscope-manifest-{}-{}.json",
            std::process::id(),
            stamp()
        ));
        std::fs::write(&path, serde_json::to_vec(&graph).expect("graph serializes"))
            .expect("temp manifest must be writable");
        path
    }

    #[tokio::test]
    async fn index_route_serves_embedded_shell() {
        let app = router(config(PathBuf::from("absent.json"), InvokeMode::Simulate));
        let response = call(
            app,
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("valid request"),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("content type present")
            .to_str()
            .expect("ascii content type");
        assert!(
            content_type.starts_with("text/html"),
            "unexpected content type: {content_type}"
        );
        let body = body_text(response).await;
        for needle in ["<div id=\"root\">", "/theme.js", "/app.js", "/app.css"] {
            assert!(body.contains(needle), "index must contain {needle}");
        }
    }

    #[tokio::test]
    async fn asset_routes_serve_embedded_constants() {
        for (uri, expected, expected_type) in [
            ("/theme.js", THEME_JS, "text/javascript"),
            ("/app.js", APP_JS, "text/javascript"),
            ("/app.css", APP_CSS, "text/css"),
        ] {
            let app = router(config(PathBuf::from("absent.json"), InvokeMode::Simulate));
            let response = call(
                app,
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await;

            assert_eq!(response.status(), StatusCode::OK, "route {uri}");
            let content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .expect("content type present")
                .to_str()
                .expect("ascii content type");
            assert_eq!(content_type, expected_type, "route {uri}");
            assert_eq!(body_text(response).await, expected, "route {uri}");
        }
    }

    #[tokio::test]
    async fn health_reports_ok_and_tool() {
        let app = router(config(PathBuf::from("absent.json"), InvokeMode::Simulate));
        let response = call(
            app,
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .expect("valid request"),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = serde_json::from_str(&body_text(response).await).expect("valid JSON");
        assert_eq!(body["ok"], true);
        assert_eq!(body["tool"], "fnscope");
    }

    #[tokio::test]
    async fn fn_graph_returns_404_for_missing_manifest() {
        let missing = std::env::temp_dir().join(format!("fnscope-absent-{}.json", stamp()));
        let app = router(config(missing, InvokeMode::Simulate));
        let response = call(
            app,
            Request::builder()
                .uri("/api/fn-graph.json")
                .body(Body::empty())
                .expect("valid request"),
        )
        .await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body: Value = serde_json::from_str(&body_text(response).await).expect("valid JSON");
        assert!(body["error"]
            .as_str()
            .expect("error is text")
            .contains("manifest not found"));
    }

    #[tokio::test]
    async fn fn_graph_returns_manifest_functions() {
        let path = write_sample_manifest();
        let app = router(config(path.clone(), InvokeMode::Simulate));
        let response = call(
            app,
            Request::builder()
                .uri("/api/fn-graph.json")
                .body(Body::empty())
                .expect("valid request"),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = serde_json::from_str(&body_text(response).await).expect("valid JSON");
        assert_eq!(
            body["functions"].as_array().expect("function array").len(),
            1
        );
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn simulate_invocation_builds_a_trace() {
        let path = write_sample_manifest();
        let app = router(config(path.clone(), InvokeMode::Simulate));
        let response = call(app, invoke_request("echo-fn", json!({ "x": 7 }))).await;

        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = serde_json::from_str(&body_text(response).await).expect("valid JSON");
        assert_eq!(body["ok"], true);
        assert_eq!(body["trace"]["step-1"]["status"], "simulated");
        assert_eq!(body["trace"]["step-1"]["output"], "value");
        assert_eq!(body["trace"]["step-2"]["status"], "simulated");
        assert_eq!(body["trace"]["step-2"]["output"], "result");
        assert_eq!(body["data"]["simulated"], true);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn unknown_fn_id_returns_an_error() {
        let path = write_sample_manifest();
        let app = router(config(path.clone(), InvokeMode::Simulate));
        let response = call(app, invoke_request("no-such-fn", json!({ "x": 7 }))).await;

        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = serde_json::from_str(&body_text(response).await).expect("valid JSON");
        assert_eq!(body["ok"], false);
        assert!(body["error"]
            .as_str()
            .expect("error is text")
            .contains("no-such-fn"));
        let _ = std::fs::remove_file(&path);
    }

    /// Returns the major version reported by a Node executable, if it answers.
    fn node_major_version(node: &std::ffi::OsStr) -> Option<u32> {
        let output = Command::new(node).arg("--version").output().ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        text.trim()
            .trim_start_matches('v')
            .split('.')
            .next()?
            .parse()
            .ok()
    }

    /// Reports whether the default Node or `FNSCOPE_NODE` is new enough for these tests.
    fn node_is_available() -> bool {
        let major = node_major_version(std::ffi::OsStr::new("node")).unwrap_or(0);
        if major >= 22 {
            return true;
        }
        match std::env::var_os("FNSCOPE_NODE") {
            Some(value) if !value.is_empty() => {
                if node_major_version(&value).unwrap_or(0) >= 22 {
                    true
                } else {
                    eprintln!("SKIP node-mode test: FNSCOPE_NODE points to Node < 22");
                    false
                }
            }
            _ => {
                eprintln!("SKIP node-mode test: Node < 22 and FNSCOPE_NODE unset");
                false
            }
        }
    }

    /// Reports whether a pid is still present, used to prove a group kill reached descendants.
    #[cfg(target_os = "linux")]
    fn process_exists(pid: i32) -> bool {
        Path::new(&format!("/proc/{pid}")).exists()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn timed_out_process_tree_is_killed() {
        let dir =
            std::env::temp_dir().join(format!("fnscope-kill-{}-{}", std::process::id(), stamp()));
        std::fs::create_dir_all(&dir).expect("scratch dir must be creatable");
        let shell_pid_path = dir.join("shell.pid");
        let child_pid_path = dir.join("child.pid");

        let mut command = Command::new("sh");
        command.arg("-c").arg(format!(
            "echo $$ > '{}'; sleep 30 & echo $! > '{}'; wait",
            shell_pid_path.display(),
            child_pid_path.display()
        ));

        let started = Instant::now();
        let error = run_bounded(command, Duration::from_millis(700))
            .expect_err("a 30-second tree must hit the 700ms deadline");
        assert!(
            error.contains("did not finish") && error.contains("process tree was killed"),
            "unexpected timeout error: {error}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the timeout path must stay bounded"
        );

        let shell_pid: i32 = std::fs::read_to_string(&shell_pid_path)
            .expect("shell pid file")
            .trim()
            .parse()
            .expect("numeric shell pid");
        let child_pid: i32 = std::fs::read_to_string(&child_pid_path)
            .expect("child pid file")
            .trim()
            .parse()
            .expect("numeric child pid");

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && (process_exists(shell_pid) || process_exists(child_pid))
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !process_exists(shell_pid),
            "group leader {shell_pid} survived the timeout"
        );
        assert!(
            !process_exists(child_pid),
            "background child {child_pid} survived the group kill"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn node_invocation_runs_a_real_export() {
        if !node_is_available() {
            return;
        }

        let dir = std::env::current_dir()
            .expect("cwd")
            .join("target")
            .join(format!("fnscope-node-{}-{}", std::process::id(), stamp()));
        std::fs::create_dir_all(dir.parent().expect("target parent")).expect("target must exist");
        std::fs::create_dir_all(&dir).expect("scratch dir must be creatable");
        let module_path = dir.join("echo.mjs");
        std::fs::write(
            &module_path,
            "export function echo(args) { return { got: args.x }; }\n",
        )
        .expect("module must be writable");

        let graph = FnGraph {
            tool: "fnscope".to_owned(),
            schema_version: "1.0.0".to_owned(),
            generated_at: "2026-11-25T12:00:00Z".to_owned(),
            root_dir: dir.to_string_lossy().into_owned(),
            functions: vec![FnRecord {
                id: "echo-fn".to_owned(),
                name: "echo".to_owned(),
                module: "echo.mjs".to_owned(),
                file_path: "echo.mjs".to_owned(),
                line: 1,
                ast_hash: format!("sha256:{}", "a".repeat(64)),
                runtime: "node24".to_owned(),
                purity: Purity::Pure,
                timeout_ms: 1000,
                description: String::new(),
                returns: ReturnShape {
                    type_: "object".to_owned(),
                    fields: Vec::new(),
                },
                steps: vec![Step {
                    id: "step-1".to_owned(),
                    kind: StepKind::Transform,
                    title: "echo input".to_owned(),
                    code: "return { got: args.x }".to_owned(),
                    output: "result".to_owned(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };
        let manifest_path = dir.join("fn-graph.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec(&graph).expect("graph serializes"),
        )
        .expect("manifest must be writable");

        let app = router(config(manifest_path.clone(), InvokeMode::Node));
        let response = call(app, invoke_request("echo-fn", json!({ "x": 42 }))).await;

        assert_eq!(response.status(), StatusCode::OK);
        let text = body_text(response).await;
        let body: Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(body["ok"], true, "response: {text}");
        assert_eq!(body["data"]["got"].as_i64(), Some(42), "response: {text}");
        assert!(
            !body["trace"].as_object().expect("trace object").is_empty(),
            "response: {text}"
        );

        let _ = std::fs::remove_file(&manifest_path);
        let _ = std::fs::remove_file(&module_path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn node_invocation_passes_a_scalar_parameter_by_name() {
        if !node_is_available() {
            return;
        }

        let dir = std::env::current_dir()
            .expect("cwd")
            .join("target")
            .join(format!(
                "fnscope-node-scalar-{}-{}",
                std::process::id(),
                stamp()
            ));
        std::fs::create_dir_all(dir.parent().expect("target parent")).expect("target must exist");
        std::fs::create_dir_all(&dir).expect("scratch dir must be creatable");
        let module_path = dir.join("shout.mjs");
        std::fs::write(
            &module_path,
            "export function shout(text) { return String(text).toUpperCase() }\n",
        )
        .expect("module must be writable");

        let graph = FnGraph {
            tool: "fnscope".to_owned(),
            schema_version: "1.0.0".to_owned(),
            generated_at: "2026-11-25T12:00:00Z".to_owned(),
            root_dir: dir.to_string_lossy().into_owned(),
            functions: vec![FnRecord {
                id: "shout-fn".to_owned(),
                name: "shout".to_owned(),
                module: "shout.mjs".to_owned(),
                file_path: "shout.mjs".to_owned(),
                parameters: vec![Parameter {
                    name: "text".to_owned(),
                    type_: "string".to_owned(),
                    required: true,
                    default_value: None,
                    description: String::new(),
                }],
                ..Default::default()
            }],
        };
        let manifest_path = dir.join("fn-graph.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec(&graph).expect("graph serializes"),
        )
        .expect("manifest must be writable");

        let app = router(config(manifest_path.clone(), InvokeMode::Node));
        let response = call(app, invoke_request("shout-fn", json!({ "text": "hi" }))).await;

        assert_eq!(response.status(), StatusCode::OK);
        let text = body_text(response).await;
        let body: Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(body["ok"], true, "response: {text}");
        assert_eq!(body["data"].as_str(), Some("HI"), "response: {text}");

        let _ = std::fs::remove_file(&manifest_path);
        let _ = std::fs::remove_file(&module_path);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
