//! Optional LLM enrichment for a parsed `FnGraph`, with a content-addressed
//! cache keyed by each function's `astHash`.
//!
//! The structural scan is the source of truth. Enrichment only adds readable
//! text and sandbox defaults: a cache hit performs **zero** network calls and
//! an all-hit run performs **zero** cache writes.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use fnscope_model::{EnrichCache, EnrichEntry, FnGraph, FnRecord, StepKind};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Default chat-completions endpoint; override with `FNSCOPE_LLM_BASE_URL`.
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1/chat/completions";
/// Default fast model; override with `FNSCOPE_LLM_MODEL`.
const DEFAULT_MODEL: &str = "gpt-4o-mini";
/// Upper bound on a single enrichment request, in seconds.
const REQUEST_TIMEOUT_SECS: u64 = 30;
/// Environment keys checked in order when locating an API key.
const API_KEY_VARS: [&str; 3] = ["OPENAI_API_KEY", "GEMINI_API_KEY", "ANTHROPIC_API_KEY"];

/// Counters describing one enrichment pass; `api_calls` is the proof of zero
/// network work on an all-cache-hit run.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnrichReport {
    /// Functions served entirely from the on-disk cache.
    pub cache_hits: usize,
    /// Functions whose `astHash` was absent from the cache.
    pub cache_misses: usize,
    /// Backend completions actually requested during this pass.
    pub api_calls: usize,
}

/// Adds human-readable text and sandbox defaults to an in-memory graph.
pub trait Enricher {
    /// Enriches `graph` in place and reports cache and API usage.
    fn enrich(&self, graph: &mut FnGraph) -> Result<EnrichReport>;
}

/// A completion provider; kept object-safe so callers can inject a mock.
pub trait LlmBackend {
    /// Returns the raw JSON reply for a single compact prompt.
    fn complete(&self, prompt: &str) -> Result<Value>;
}

/// Fills only missing fields with values inferred deterministically from the
/// declared types, never consulting a model.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NoopEnricher;

impl Enricher for NoopEnricher {
    fn enrich(&self, graph: &mut FnGraph) -> Result<EnrichReport> {
        for function in graph.functions.iter_mut() {
            apply_deterministic_blanks(function);
        }
        Ok(EnrichReport {
            cache_hits: 0,
            cache_misses: 0,
            api_calls: 0,
        })
    }
}

/// Test backend that hands out queued replies and counts every call.
#[derive(Debug)]
pub struct MockBackend {
    /// Replies consumed in FIFO order; an empty queue yields a valid blank object.
    pub replies: Mutex<Vec<Value>>,
    /// Number of `complete` invocations observed so far.
    pub calls: AtomicUsize,
}

impl MockBackend {
    /// Creates a backend that will return `replies` in the given order.
    pub fn new(replies: Vec<Value>) -> Self {
        Self {
            replies: Mutex::new(replies),
            calls: AtomicUsize::new(0),
        }
    }

    /// Reports how many completions reached this backend.
    pub fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl LlmBackend for MockBackend {
    fn complete(&self, _prompt: &str) -> Result<Value> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut replies = self
            .replies
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if replies.is_empty() {
            Ok(json!({
                "description": "",
                "stepTitles": [],
                "stepDetails": [],
                "defaults": {},
                "tags": []
            }))
        } else {
            Ok(replies.remove(0))
        }
    }
}

/// HTTP completion provider used by `CachedEnricher::from_env`.
struct UreqBackend {
    /// Shared, timeout-bounded client with rustls defaults.
    agent: ureq::Agent,
    /// Chat-completions endpoint supplied through the environment.
    base_url: String,
    /// Bearer token selected from the supported provider variables.
    api_key: String,
    /// Model name sent with each completion request.
    model: String,
}

/// Builds the exact JSON body sent by the HTTP backend, including the model.
fn chat_payload(model: &str, prompt: &str) -> Value {
    json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": "Reply with strict JSON only, no prose and no markdown fences."
            },
            { "role": "user", "content": prompt }
        ],
        "temperature": 0
    })
}

impl LlmBackend for UreqBackend {
    fn complete(&self, prompt: &str) -> Result<Value> {
        let payload = chat_payload(&self.model, prompt);
        let authorization = format!("Bearer {}", self.api_key);
        let mut response = self
            .agent
            .post(self.base_url.as_str())
            .header("Authorization", authorization.as_str())
            .header("Content-Type", "application/json")
            .send_json(&payload)
            .map_err(|err| anyhow!("LLM request to {} failed: {err}", self.base_url))?;
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|err| anyhow!("Could not read LLM response body: {err}"))?;
        serde_json::from_str(&body)
            .with_context(|| format!("LLM reply was not JSON: {:.200}", body.trim()))
    }
}

/// Enriches via an injected backend, caching every result under `astHash`.
pub struct CachedEnricher {
    /// Where the content-addressed cache is read from and written to.
    cache_path: PathBuf,
    /// Completion provider; a cache hit means it is never touched.
    backend: Box<dyn LlmBackend>,
    /// Model label forwarded to the HTTP backend and used in error context.
    model: String,
}

impl CachedEnricher {
    /// Builds an enricher from an explicit backend, useful for offline tests.
    pub fn new(cache_path: PathBuf, backend: Box<dyn LlmBackend>, model: String) -> Self {
        Self {
            cache_path,
            backend,
            model,
        }
    }

    /// Builds an enricher whose backend posts to an OpenAI-compatible endpoint.
    ///
    /// The endpoint comes from `FNSCOPE_LLM_BASE_URL`, the model from
    /// `FNSCOPE_LLM_MODEL`, and the key from the first non-empty provider
    /// variable in `API_KEY_VARS`. Requests are bounded to 30 seconds.
    pub fn from_env(cache_path: PathBuf) -> Result<Self> {
        let model =
            std::env::var("FNSCOPE_LLM_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        Self::from_env_with_model(cache_path, model)
    }

    /// Builds the HTTP-backed enricher from env keys with an explicit model name,
    /// so the CLI's --model flag is honoured.
    pub fn from_env_with_model(cache_path: PathBuf, model: String) -> Result<Self> {
        let base_url =
            std::env::var("FNSCOPE_LLM_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned());
        let api_key = API_KEY_VARS
            .iter()
            .find_map(|name| {
                std::env::var(name)
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
            .ok_or_else(|| {
                anyhow!(
                    "No LLM API key configured. Set one of {}, GEMINI_API_KEY, or ANTHROPIC_API_KEY first.",
                    API_KEY_VARS[0]
                )
            })?;
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(REQUEST_TIMEOUT_SECS)))
            .build();
        let agent = ureq::Agent::new_with_config(config);
        let backend = UreqBackend {
            agent,
            base_url,
            api_key,
            model: model.clone(),
        };
        Ok(Self {
            cache_path,
            backend: Box::new(backend),
            model,
        })
    }
}

impl Enricher for CachedEnricher {
    fn enrich(&self, graph: &mut FnGraph) -> Result<EnrichReport> {
        let mut cache = load_cache(&self.cache_path)?;
        let mut report = EnrichReport {
            cache_hits: 0,
            cache_misses: 0,
            api_calls: 0,
        };

        for function in graph.functions.iter_mut() {
            // Clone the entry so the immutable borrow of `function` ends before
            // the cached values are merged into it.
            if let Some(entry) = cache.entries.get(&function.ast_hash).cloned() {
                apply_entry(function, &entry);
                report.cache_hits += 1;
                continue;
            }

            let prompt = build_prompt(function);
            let reply = self
                .backend
                .complete(&prompt)
                .map_err(|err| anyhow!("model {} completion failed: {err}", self.model))?;
            report.api_calls += 1;
            report.cache_misses += 1;

            let (entry, malformed) = parse_reply(&reply, function);
            if malformed {
                // A bad model answer must never be retried forever: cache the
                // deterministic values produced for this exact function.
                apply_entry_blanks(function, &entry);
            } else {
                apply_entry(function, &entry);
            }
            cache.entries.insert(function.ast_hash.clone(), entry);
        }

        if report.cache_misses > 0 {
            cache.version = cache.version.saturating_add(1);
            write_cache(&self.cache_path, &cache)?;
        }
        Ok(report)
    }
}

/// Reads an enrichment cache, treating a missing file as a fresh version-1 cache.
pub fn load_cache(path: &Path) -> Result<EnrichCache> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .with_context(|| format!("Malformed enrichment cache at {}", path.display())),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(EnrichCache::default()),
        Err(err) => Err(err)
            .with_context(|| format!("Could not read enrichment cache at {}", path.display())),
    }
}

/// Writes an enrichment cache as pretty JSON via a temporary file and rename.
pub fn write_cache(path: &Path, cache: &EnrichCache) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Could not create cache directory {}", parent.display())
            })?;
        }
    }
    let text =
        serde_json::to_string_pretty(cache).context("Could not serialize enrichment cache")?;
    let mut temporary = path.to_path_buf();
    temporary.set_extension("json.tmp");
    fs::write(&temporary, &text)
        .with_context(|| format!("Could not write temporary cache {}", temporary.display()))?;

    // Windows rename fails when the destination exists, so remove it first.
    if path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("Could not replace old cache {}", path.display()))?;
    }
    if let Err(err) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(err).with_context(|| {
            format!(
                "Could not move temporary cache {} to {}",
                temporary.display(),
                path.display()
            )
        });
    }
    Ok(())
}

/// Converts a step kind into its lowercase contract spelling.
fn step_kind_label(kind: StepKind) -> &'static str {
    match kind {
        StepKind::Call => "call",
        StepKind::Branch => "branch",
        StepKind::Transform => "transform",
        StepKind::Return => "return",
    }
}

/// Uppercases the first character of a lowercase label for human-readable text.
fn capitalize(label: &str) -> String {
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Infers a neutral sandbox default from a declared TypeScript-like type string.
fn default_for_type(type_: &str, name: &str) -> Value {
    let normalized = type_.trim().to_ascii_lowercase();
    if normalized.ends_with("[]")
        || normalized.contains("[]")
        || normalized.starts_with("array")
        || normalized.starts_with('[')
    {
        return json!([]);
    }
    if normalized.starts_with("record")
        || normalized.starts_with("object")
        || normalized.starts_with('{')
    {
        return json!({});
    }
    if normalized.starts_with("string") {
        return Value::String(format!("sample-{name}"));
    }
    if matches!(
        normalized.as_str(),
        "number" | "integer" | "int" | "float" | "double"
    ) {
        return json!(0);
    }
    if normalized == "boolean" || normalized == "bool" {
        return json!(false);
    }
    Value::Null
}

/// Builds the complete deterministic entry used by `NoopEnricher` and bad replies.
fn deterministic_entry(function: &FnRecord) -> EnrichEntry {
    let step_titles = function
        .steps
        .iter()
        .map(|step| format!("{} step", capitalize(step_kind_label(step.kind))))
        .collect();
    let step_details = function
        .steps
        .iter()
        .map(|step| {
            format!(
                "Deterministic {} step in {}.",
                step_kind_label(step.kind),
                function.name
            )
        })
        .collect();
    let defaults = function
        .parameters
        .iter()
        .map(|parameter| {
            (
                parameter.name.clone(),
                default_for_type(&parameter.type_, &parameter.name),
            )
        })
        .collect::<BTreeMap<_, _>>();
    EnrichEntry {
        description: Some(format!("Automatically described {}.", function.name)),
        step_titles,
        step_details,
        defaults,
        tags: Vec::new(),
    }
}

/// Replaces populated fields from an entry, leaving only truly empty ones alone.
fn apply_entry(function: &mut FnRecord, entry: &EnrichEntry) {
    if let Some(description) = &entry.description {
        if !description.is_empty() {
            function.description = description.clone();
        }
    }
    for (index, step) in function.steps.iter_mut().enumerate() {
        if let Some(title) = entry.step_titles.get(index) {
            if !title.is_empty() {
                step.title = title.clone();
            }
        }
        if let Some(detail) = entry.step_details.get(index) {
            if !detail.is_empty() {
                step.detail = detail.clone();
            }
        }
    }
    for parameter in function.parameters.iter_mut() {
        if parameter.default_value.is_none() {
            if let Some(value) = entry.defaults.get(&parameter.name) {
                parameter.default_value = Some(value.clone());
            }
        }
    }
    if !entry.tags.is_empty() {
        function.tags = entry.tags.clone();
    }
}

/// Fills only blank fields, preserving any deterministic values already present.
fn apply_entry_blanks(function: &mut FnRecord, entry: &EnrichEntry) {
    if function.description.is_empty() {
        if let Some(description) = &entry.description {
            if !description.is_empty() {
                function.description = description.clone();
            }
        }
    }
    for (index, step) in function.steps.iter_mut().enumerate() {
        if step.title.is_empty() {
            if let Some(title) = entry.step_titles.get(index) {
                if !title.is_empty() {
                    step.title = title.clone();
                }
            }
        }
        if step.detail.is_empty() {
            if let Some(detail) = entry.step_details.get(index) {
                if !detail.is_empty() {
                    step.detail = detail.clone();
                }
            }
        }
    }
    for parameter in function.parameters.iter_mut() {
        if parameter.default_value.is_none() {
            if let Some(value) = entry.defaults.get(&parameter.name) {
                parameter.default_value = Some(value.clone());
            }
        }
    }
}

/// Applies the deterministic fallback to a single function in place.
fn apply_deterministic_blanks(function: &mut FnRecord) {
    let entry = deterministic_entry(function);
    apply_entry_blanks(function, &entry);
}

/// Builds a compact skeleton prompt; full source code is never transmitted.
fn build_prompt(function: &FnRecord) -> String {
    let mut prompt = String::new();
    prompt.push_str(
        "Enrich this function skeleton. Reply with STRICT JSON only (no prose, no fences): \
         {\"description\":\"...\",\"stepTitles\":[\"...\"],\"stepDetails\":[\"...\"],\
         \"defaults\":{\"name\":value},\"tags\":[\"...\"]}.\n",
    );
    prompt.push_str(&format!("name: {}\n", function.name));
    prompt.push_str(&format!("returns: {}\n", function.returns.type_));
    prompt.push_str("parameters (name: type):\n");
    for parameter in &function.parameters {
        prompt.push_str(&format!("  {}: {}\n", parameter.name, parameter.type_));
    }
    prompt.push_str("steps (id | kind | code | output):\n");
    for step in &function.steps {
        prompt.push_str(&format!(
            "  {} | {} | {} | {}\n",
            step.id,
            step_kind_label(step.kind),
            step.code,
            step.output
        ));
    }
    prompt
}

/// Removes optional fenced-code markers around a model's embedded JSON text.
fn strip_fences(text: &str) -> &str {
    let trimmed = text.trim();
    let opened = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```JSON"))
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    opened.strip_suffix("```").unwrap_or(opened).trim()
}

/// Extracts a JSON object from a raw object, an OpenAI envelope, or fenced text.
fn extract_object(reply: &Value) -> Option<Value> {
    if let Some(content) = reply
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
    {
        let parsed = serde_json::from_str::<Value>(strip_fences(content)).ok();
        if let Some(value) = parsed {
            if value.is_object() {
                return Some(value);
            }
        }
    }
    match reply {
        Value::Object(_) => Some(reply.clone()),
        Value::String(text) => serde_json::from_str::<Value>(strip_fences(text))
            .ok()
            .filter(|value| value.is_object()),
        _ => None,
    }
}

/// Whether an entry carries no usable enrichment at all.
fn entry_is_empty(entry: &EnrichEntry) -> bool {
    entry.description.as_deref().unwrap_or("").is_empty()
        && entry.step_titles.iter().all(String::is_empty)
        && entry.step_details.iter().all(String::is_empty)
        && entry.defaults.is_empty()
        && entry.tags.is_empty()
}

/// Maps a parsed JSON object into an entry whose arrays align with step indices.
fn entry_from_object(object: &Value, function: &FnRecord) -> EnrichEntry {
    let description = object
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut step_titles: Vec<String> = object
        .get("stepTitles")
        .or_else(|| object.get("step_titles"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let mut step_details: Vec<String> = object
        .get("stepDetails")
        .or_else(|| object.get("step_details"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    step_titles.resize(function.steps.len(), String::new());
    step_details.resize(function.steps.len(), String::new());
    let defaults = object
        .get("defaults")
        .and_then(Value::as_object)
        .map(|members| {
            members
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let tags = object
        .get("tags")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    EnrichEntry {
        description,
        step_titles,
        step_details,
        defaults,
        tags,
    }
}

/// Parses a backend reply, reporting `true` when deterministic values replaced it.
fn parse_reply(reply: &Value, function: &FnRecord) -> (EnrichEntry, bool) {
    if let Some(object) = extract_object(reply) {
        let entry = entry_from_object(&object, function);
        if !entry_is_empty(&entry) {
            return (entry, false);
        }
    }
    (deterministic_entry(function), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnscope_model::{Purity, ReturnShape};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Isolated temp directory with a collision-free name and eager cleanup.
    struct TempDir {
        /// Absolute path of this test's private directory.
        path: PathBuf,
    }

    impl TempDir {
        /// Creates a fresh unique directory under the system temp folder.
        fn new(label: &str) -> Self {
            let sequence = TEST_SEQ.fetch_add(1, Ordering::SeqCst);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock is after the epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "fnscope-enrich-{}-{}-{nanos}-{sequence}",
                label,
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("test temp dir must be creatable");
            Self { path }
        }

        /// Returns the cache file path inside this directory.
        fn cache_path(&self) -> PathBuf {
            self.path.join("enrich-cache.json")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    /// Guarantees unique temp paths even when tests run in the same nanosecond.
    static TEST_SEQ: AtomicUsize = AtomicUsize::new(0);

    /// Single-function graph with blank text and unset defaults for enrichment.
    fn sample_graph() -> FnGraph {
        FnGraph {
            tool: "fnscope".to_owned(),
            schema_version: "1.0.0".to_owned(),
            generated_at: "2026-11-25T12:00:00Z".to_owned(),
            root_dir: "src".to_owned(),
            functions: vec![FnRecord {
                id: "src-evaluate-grammar-role".to_owned(),
                name: "evaluateGrammarRole".to_owned(),
                module: "src/roles.ts".to_owned(),
                file_path: "src/roles.ts".to_owned(),
                line: 10,
                ast_hash: sample_hash(),
                version: "1.0.0".to_owned(),
                runtime: "node20".to_owned(),
                purity: Purity::Pure,
                timeout_ms: 100,
                permissions: Vec::new(),
                description: String::new(),
                tags: Vec::new(),
                called_by: Vec::new(),
                parameters: vec![
                    fnscope_model::Parameter {
                        name: "sentence".to_owned(),
                        type_: "string".to_owned(),
                        required: true,
                        default_value: None,
                        description: String::new(),
                    },
                    fnscope_model::Parameter {
                        name: "limit".to_owned(),
                        type_: "number".to_owned(),
                        required: false,
                        default_value: None,
                        description: String::new(),
                    },
                    fnscope_model::Parameter {
                        name: "strict".to_owned(),
                        type_: "boolean".to_owned(),
                        required: false,
                        default_value: None,
                        description: String::new(),
                    },
                    fnscope_model::Parameter {
                        name: "tokens".to_owned(),
                        type_: "string[]".to_owned(),
                        required: false,
                        default_value: None,
                        description: String::new(),
                    },
                    fnscope_model::Parameter {
                        name: "options".to_owned(),
                        type_: "Record<string, unknown>".to_owned(),
                        required: false,
                        default_value: None,
                        description: String::new(),
                    },
                ],
                steps: vec![
                    fnscope_model::Step {
                        id: "step-1".to_owned(),
                        kind: StepKind::Call,
                        calls_fn_id: None,
                        title: String::new(),
                        code: "tokenize(input.sentence)".to_owned(),
                        inputs: Vec::new(),
                        output: "tokens: Token[]".to_owned(),
                        detail: String::new(),
                    },
                    fnscope_model::Step {
                        id: "step-2".to_owned(),
                        kind: StepKind::Return,
                        calls_fn_id: None,
                        title: "Return result".to_owned(),
                        code: "return result".to_owned(),
                        inputs: Vec::new(),
                        output: "RoleEvaluationResult".to_owned(),
                        detail: String::new(),
                    },
                ],
                returns: ReturnShape {
                    type_: "RoleEvaluationResult".to_owned(),
                    fields: Vec::new(),
                },
                errors: Vec::new(),
                source_code: "export function evaluateGrammarRole(...) { /* ... */ }".to_owned(),
            }],
        }
    }

    /// First function hash used by the fixtures.
    fn sample_hash() -> String {
        format!("sha256:{}", "a".repeat(64))
    }

    /// Well-formed model reply with all four enrichment fields.
    fn valid_reply() -> Value {
        json!({
            "description": "Evaluates the grammar role.",
            "stepTitles": ["Tokenize the sentence", "Return the verdict"],
            "stepDetails": ["Split into tokens.", "Produce the role payload."],
            "defaults": { "sentence": "sample sentence", "limit": 5 },
            "tags": ["nlp"]
        })
    }

    #[test]
    fn first_run_calls_backend_once_and_writes_cache() {
        let directory = TempDir::new("first-run");
        let path = directory.cache_path();
        let backend = MockBackend::new(vec![valid_reply()]);
        let enricher =
            CachedEnricher::new(path.clone(), Box::new(backend), "test-model".to_owned());

        let mut graph = sample_graph();
        let report = enricher
            .enrich(&mut graph)
            .expect("enrichment must succeed");

        assert_eq!(report.api_calls, 1);
        assert_eq!(report.cache_hits, 0);
        assert_eq!(report.cache_misses, 1);
        assert_eq!(
            graph.functions[0].description,
            "Evaluates the grammar role."
        );
        assert_eq!(graph.functions[0].steps[0].title, "Tokenize the sentence");
        assert_eq!(
            graph.functions[0].steps[1].detail,
            "Produce the role payload."
        );
        assert_eq!(
            graph.functions[0].parameters[0].default_value,
            Some(json!("sample sentence"))
        );
        assert_eq!(graph.functions[0].tags, vec!["nlp".to_owned()]);

        assert!(path.exists(), "cache file must be written after a miss");
        let cache = load_cache(&path).expect("cache must load");
        assert!(cache.entries.contains_key(&sample_hash()));
        assert_eq!(cache.version, 2);
    }

    #[test]
    fn cache_hit_preserves_values_with_zero_api_calls_and_zero_writes() {
        let directory = TempDir::new("cache-hit");
        let path = directory.cache_path();
        {
            let backend = MockBackend::new(vec![valid_reply()]);
            let enricher =
                CachedEnricher::new(path.clone(), Box::new(backend), "test-model".to_owned());
            enricher
                .enrich(&mut sample_graph())
                .expect("seeding enrichment must succeed");
        }
        // Store a uniquely compact copy so that any rewrite (which is pretty)
        // would be detectable byte-for-byte after the all-hit run.
        let seeded = load_cache(&path).expect("seed cache must load");
        let compact = serde_json::to_vec(&seeded).expect("cache must serialize");
        fs::write(&path, &compact).expect("sentinel write must succeed");

        let backend = MockBackend::new(Vec::new());
        let enricher =
            CachedEnricher::new(path.clone(), Box::new(backend), "test-model".to_owned());
        let mut graph = sample_graph();
        let report = enricher
            .enrich(&mut graph)
            .expect("cached run must succeed");

        assert_eq!(report.api_calls, 0);
        assert_eq!(report.cache_hits, 1);
        assert_eq!(report.cache_misses, 0);
        assert_eq!(
            graph.functions[0].description,
            "Evaluates the grammar role."
        );
        assert_eq!(
            graph.functions[0].parameters[0].default_value,
            Some(json!("sample sentence"))
        );
        // An all-hit run must not rewrite the cache at all.
        assert_eq!(
            fs::read(&path).expect("cache must be readable"),
            compact,
            "an all-hit run must not rewrite the cache"
        );
    }

    #[test]
    fn changed_ast_hash_is_a_new_miss_and_adds_a_cache_entry() {
        let directory = TempDir::new("changed-hash");
        let path = directory.cache_path();
        {
            let backend = MockBackend::new(vec![valid_reply()]);
            let enricher =
                CachedEnricher::new(path.clone(), Box::new(backend), "test-model".to_owned());
            enricher
                .enrich(&mut sample_graph())
                .expect("seeding enrichment must succeed");
        }

        let mut graph = sample_graph();
        graph.functions[0].ast_hash = format!("sha256:{}", "b".repeat(64));
        let backend = MockBackend::new(vec![valid_reply()]);
        let enricher =
            CachedEnricher::new(path.clone(), Box::new(backend), "test-model".to_owned());
        let report = enricher
            .enrich(&mut graph)
            .expect("re-enrichment must succeed");

        assert_eq!(report.api_calls, 1);
        assert_eq!(report.cache_misses, 1);
        assert_eq!(report.cache_hits, 0);
        let cache = load_cache(&path).expect("cache must load");
        assert_eq!(cache.entries.len(), 2);
        assert!(cache.entries.contains_key(&sample_hash()));
        assert!(cache.entries.contains_key(&graph.functions[0].ast_hash));
    }

    #[test]
    fn openai_envelope_with_fenced_json_parses() {
        let directory = TempDir::new("envelope");
        let reply = json!({
            "choices": [{
                "message": {
                    "content": "```json\n{\"description\":\"Wrapped description.\",\"stepTitles\":[\"A\",\"B\"],\"stepDetails\":[\"a\",\"b\"],\"defaults\":{\"sentence\":\"wrapped\"},\"tags\":[\"wrapped\"]}\n```"
                }
            }]
        });
        let backend = MockBackend::new(vec![reply]);
        let enricher =
            CachedEnricher::new(directory.cache_path(), Box::new(backend), "m".to_owned());
        let mut graph = sample_graph();
        let report = enricher
            .enrich(&mut graph)
            .expect("enrichment must succeed");

        assert_eq!(report.api_calls, 1);
        assert_eq!(graph.functions[0].description, "Wrapped description.");
        assert_eq!(graph.functions[0].steps[0].title, "A");
        assert_eq!(graph.functions[0].steps[1].detail, "b");
        assert_eq!(
            graph.functions[0].parameters[0].default_value,
            Some(json!("wrapped"))
        );
        assert_eq!(graph.functions[0].tags, vec!["wrapped".to_owned()]);
    }

    #[test]
    fn noop_enricher_fills_only_blanks() {
        let mut graph = sample_graph();
        graph.functions[0].description = "Existing description".to_owned();
        graph.functions[0].steps[0].title = "Existing title".to_owned();
        graph.functions[0].steps[1].title.clear();
        graph.functions[0].parameters[0].default_value = Some(json!("keep me"));

        let report = NoopEnricher
            .enrich(&mut graph)
            .expect("noop enrichment must succeed");

        assert_eq!(
            report,
            EnrichReport {
                cache_hits: 0,
                cache_misses: 0,
                api_calls: 0
            }
        );
        assert_eq!(graph.functions[0].description, "Existing description");
        assert_eq!(graph.functions[0].steps[0].title, "Existing title");
        assert_eq!(
            graph.functions[0].parameters[0].default_value,
            Some(json!("keep me"))
        );

        let function = &graph.functions[0];
        assert!(!function.steps[1].title.is_empty());
        assert!(!function.steps[0].detail.is_empty());
        assert_eq!(function.parameters[1].default_value, Some(json!(0)));
        assert_eq!(function.parameters[2].default_value, Some(json!(false)));
        assert_eq!(function.parameters[3].default_value, Some(json!([])));
        assert_eq!(function.parameters[4].default_value, Some(json!({})));
    }

    #[test]
    fn malformed_reply_still_yields_deterministic_values_and_a_cache_entry() {
        let directory = TempDir::new("malformed");
        let backend = MockBackend::new(vec![json!("this is not json at all")]);
        let enricher =
            CachedEnricher::new(directory.cache_path(), Box::new(backend), "m".to_owned());
        let mut graph = sample_graph();
        let report = enricher
            .enrich(&mut graph)
            .expect("enrichment must succeed");

        assert_eq!(report.api_calls, 1);
        assert_eq!(report.cache_misses, 1);
        assert!(!graph.functions[0].description.is_empty());
        assert_eq!(
            graph.functions[0].parameters[0].default_value,
            Some(json!("sample-sentence"))
        );
        let cache = load_cache(&directory.cache_path()).expect("cache must load");
        assert_eq!(cache.entries.len(), 1);
        assert!(cache.entries[&sample_hash()].description.is_some());
    }

    #[test]
    fn missing_cache_file_loads_as_empty_version_one() {
        let directory = TempDir::new("missing-cache");
        let path = directory.path.join("no-such-cache.json");
        let cache = load_cache(&path).expect("missing cache must not be an error");
        assert_eq!(cache.version, 1);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn malformed_cache_file_reports_a_readable_error() {
        let directory = TempDir::new("bad-cache");
        let path = directory.cache_path();
        fs::write(&path, "{this is not json").expect("fixture write must succeed");
        let error = load_cache(&path).expect_err("malformed cache must be rejected");
        assert!(
            error.to_string().contains("Malformed enrichment cache"),
            "unexpected error: {error}"
        );
    }

    /// Restores an environment variable after a test overrides it.
    struct EnvGuard {
        /// Variable whose original state is remembered.
        key: &'static str,
        /// Original value, or `None` when the variable was absent.
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        /// Sets `key` to `value`, remembering the prior state for teardown.
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn from_env_with_model_uses_explicit_model_in_http_payload() {
        let directory = TempDir::new("explicit-model");
        let _key = EnvGuard::set("OPENAI_API_KEY", "test-key-for-model-flag");
        let _env_model = EnvGuard::set("FNSCOPE_LLM_MODEL", "env-model-must-be-ignored");

        let enricher = CachedEnricher::from_env_with_model(
            directory.cache_path(),
            "cli-explicit-model".to_owned(),
        )
        .expect("an available API key must let the constructor succeed");

        assert_eq!(enricher.model, "cli-explicit-model");
        let payload = chat_payload(&enricher.model, "compact prompt");
        assert_eq!(payload["model"], "cli-explicit-model");
        assert_eq!(payload["messages"][1]["content"], "compact prompt");
    }
}
