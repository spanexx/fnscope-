//! Local HTTP server that serves the embedded explorer UI and the JSON/invoke APIs.
//!
//! The manifest is re-read on every request so a file watcher's regenerated graph
//! becomes visible without restarting the server. All handlers are total: a missing
//! or corrupt manifest produces a JSON error response instead of a panic.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(test)]
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use fnscope_model::{FnGraph, InvokeRequest, InvokeResponse, TraceEntry};
use serde_json::{json, Value};

/// Explorer shell embedded at build time; the UI job replaces the placeholder file.
pub const INDEX_HTML: &str = include_str!("../../../ui/dist/index.html");
/// Bundled application script embedded at build time.
pub const APP_JS: &str = include_str!("../../../ui/dist/app.js");
/// Compiled stylesheet embedded at build time.
pub const APP_CSS: &str = include_str!("../../../ui/dist/app.css");

/// Chooses how `/api/invoke` answers: deterministic replay or a real Node process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Debug)]
struct AppState {
    manifest_path: PathBuf,
    invoke_mode: InvokeMode,
}

/// Builds the complete route table with state attached, so tests need no socket.
pub fn router(config: ServerConfig) -> Router {
    let state = Arc::new(AppState {
        manifest_path: config.manifest_path,
        invoke_mode: config.invoke_mode,
    });

    Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/app.css", get(app_css))
        .route("/api/health", get(health))
        .route("/api/fn-graph.json", get(fn_graph))
        .route("/api/invoke", post(invoke))
        .with_state(state)
}

/// Serves the single-page application shell.
async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// Serves the embedded JavaScript bundle as JavaScript.
async fn app_js() -> Response {
    asset(APP_JS, "text/javascript")
}

/// Serves the embedded stylesheet as CSS.
async fn app_css() -> Response {
    asset(APP_CSS, "text/css")
}

/// Wraps a static asset with its narrow, explicit content type.
fn asset(body: &'static str, content_type: &'static str) -> Response {
    (StatusCode::OK, [("content-type", content_type)], body).into_response()
}

/// Reports liveness for the CLI and for external health checks.
async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "tool": "fnscope" }))
}

/// Re-reads the manifest on every request so watcher updates appear immediately.
async fn fn_graph(State(state): State<Arc<AppState>>) -> Response {
    let raw = match tokio::fs::read_to_string(&state.manifest_path).await {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return error_response(
                StatusCode::NOT_FOUND,
                "manifest not found — run fnscope scan first".to_owned(),
            );
        }
        Err(err) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("cannot read manifest: {err}"),
            );
        }
    };

    // Deserializing into FnGraph validates the schema before the text is handed to the UI.
    match serde_json::from_str::<FnGraph>(&raw) {
        Ok(_) => (StatusCode::OK, [("content-type", "application/json")], raw).into_response(),
        Err(err) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("invalid manifest JSON: {err}"),
        ),
    }
}

/// Emits a JSON error body without ever panicking.
fn error_response(status: StatusCode, message: String) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

/// Loads the manifest for an invocation, reporting failures as readable strings.
async fn load_graph(path: &Path) -> Result<FnGraph, String> {
    let raw = tokio::fs::read_to_string(path)
        .await
        .map_err(|err| format!("cannot read manifest: {err}"))?;
    serde_json::from_str::<FnGraph>(&raw).map_err(|err| format!("invalid manifest JSON: {err}"))
}

/// Dispatches an invocation to the simulator or to the Node runner.
async fn invoke(
    State(state): State<Arc<AppState>>,
    Json(request): Json<InvokeRequest>,
) -> Json<InvokeResponse> {
    let started = Instant::now();

    let graph = match load_graph(&state.manifest_path).await {
        Ok(graph) => graph,
        Err(reason) => {
            return Json(InvokeResponse {
                ok: false,
                error: Some(reason),
                duration_ms: started.elapsed().as_secs_f64() * 1000.0,
                ..Default::default()
            });
        }
    };

    let simulated = simulate(&graph, &request);
    let mut response = match state.invoke_mode {
        InvokeMode::Simulate => simulated.clone(),
        InvokeMode::Node => {
            let node_graph = graph.clone();
            let node_request = request.clone();
            let scratch_base = state
                .manifest_path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .map(|parent| parent.to_path_buf())
                .unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir())
                });
            let scratch_base_for_task = scratch_base.clone();
            match tokio::task::spawn_blocking(move || {
                run_node(&node_graph, &node_request, &scratch_base_for_task)
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
    Json(response)
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

/// Executes the target export in a Node child process and parses its JSON result.
fn run_node(
    graph: &FnGraph,
    request: &InvokeRequest,
    scratch_base: &Path,
) -> Result<InvokeResponse, String> {
    let function = graph
        .functions
        .iter()
        .find(|function| function.id == request.fn_id)
        .ok_or_else(|| format!("unknown fnId: {}", request.fn_id))?;

    let root = PathBuf::from(&graph.root_dir);
    let absolute_root = if root.is_absolute() {
        root
    } else {
        std::env::current_dir()
            .map_err(|err| format!("cannot read current directory: {err}"))?
            .join(root)
    };
    let module_path = absolute_root.join(&function.file_path);

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
    let child = Command::new(&node)
        .arg("--experimental-strip-types")
        .arg(&runner_path)
        .arg(&args_path)
        .arg(&module_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("node executable '{shown}' could not start: {err}"))?;

    // The wait is bounded with a channel rather than a platform kill timeout: `Child`
    // is not `Sync` and there is no portable way to terminate a whole process group,
    // so a timed-out child is left to finish on its own while the temp dir is removed.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    let output = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|err| format!("node '{shown}' did not finish within 5s ({err})"))?
        .map_err(|err| format!("node '{shown}' failed while waiting: {err}"))?;

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
        for needle in ["<div id=\"root\">", "/app.js", "/app.css"] {
            assert!(body.contains(needle), "index must contain {needle}");
        }
    }

    #[tokio::test]
    async fn asset_routes_serve_embedded_constants() {
        for (uri, expected, expected_type) in [
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
