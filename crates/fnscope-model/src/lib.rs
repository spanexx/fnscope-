//! Shared JSON schema for fnscope manifests, enrichment caches and invoke messages.
//!
//! Every spelling in this crate is part of the locked contract in
//! `docs/contract/rust-api.md`; other crates depend on the exact serde names.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

/// Tool tag written into every manifest so readers can reject foreign documents.
const TOOL_NAME: &str = "fnscope";

/// Schema revision written into every manifest so readers can reject old shapes.
const SCHEMA_VERSION: &str = "1.0.0";

/// Supplies the documented revision when an older manifest leaves `version` out.
fn default_version() -> String {
    SCHEMA_VERSION.to_owned()
}

/// Reports whether a value is exactly `sha256:` followed by 64 hexadecimal digits.
fn is_valid_ast_hash(value: &str) -> bool {
    match value.strip_prefix("sha256:") {
        Some(digest) => digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
        None => false,
    }
}

/// Whether a function only computes from its inputs or also touches outside state.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Purity {
    /// Safe to replay during simulation because nothing outside the result changes.
    #[default]
    Pure,
    /// Needs a live environment (I/O, globals, clock) to produce a faithful result.
    SideEffect,
}

/// The role a body step plays, used to build readable traces.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StepKind {
    /// Reaches another discovered function.
    #[default]
    Call,
    /// Chooses between alternative paths.
    Branch,
    /// Computes a value without leaving the function.
    Transform,
    /// Produces the function's returned value.
    Return,
}

/// Root document of `.fnscope/manifest.json`, holding every discovered function.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FnGraph {
    /// Identifies fnscope as the producer so consumers can detect wrong files.
    pub tool: String,
    /// Pins the JSON shape so readers can reject incompatible revisions.
    #[serde(rename = "schemaVersion")]
    pub schema_version: String,
    /// RFC3339 UTC instant of the scan, kept for deterministic cache invalidation.
    #[serde(rename = "generatedAt")]
    pub generated_at: String,
    /// Directory the scan started from, so ids and paths can be resolved later.
    #[serde(rename = "rootDir")]
    pub root_dir: String,
    /// Discovered functions; an empty root legitimately produces an empty list.
    #[serde(default)]
    pub functions: Vec<FnRecord>,
}

impl Default for FnGraph {
    fn default() -> Self {
        Self {
            tool: TOOL_NAME.to_owned(),
            schema_version: SCHEMA_VERSION.to_owned(),
            generated_at: String::new(),
            root_dir: String::new(),
            functions: Vec::new(),
        }
    }
}

impl FnGraph {
    /// Checks the invariants every consumer relies on and aggregates all faults.
    pub fn validate(&self) -> Result<(), String> {
        let mut problems: Vec<String> = Vec::new();

        if self.tool != TOOL_NAME {
            problems.push(format!(
                "tool must be \"{TOOL_NAME}\" (got {:?})",
                self.tool
            ));
        }
        if self.schema_version != SCHEMA_VERSION {
            problems.push(format!(
                "schemaVersion must be \"{SCHEMA_VERSION}\" (got {:?})",
                self.schema_version
            ));
        }

        let mut seen_function_ids: HashSet<String> = HashSet::new();
        for (index, function) in self.functions.iter().enumerate() {
            if function.id.is_empty() {
                problems.push(format!("functions[{index}].id must not be empty"));
            } else if !seen_function_ids.insert(function.id.clone()) {
                problems.push(format!(
                    "functions[{index}].id is duplicated: {:?}",
                    function.id
                ));
            }

            if !is_valid_ast_hash(&function.ast_hash) {
                problems.push(format!(
                    "functions[{index}].astHash must be \"sha256:\" plus 64 hex chars (got {:?})",
                    function.ast_hash
                ));
            }
            if function.line == 0 {
                problems.push(format!(
                    "functions[{index}].line must be >= 1 (got {})",
                    function.line
                ));
            }
            if function.module.is_empty() {
                problems.push(format!("functions[{index}].module must not be empty"));
            }
            if function.file_path.is_empty() {
                problems.push(format!("functions[{index}].filePath must not be empty"));
            }

            let mut seen_step_ids: HashSet<String> = HashSet::new();
            for (step_index, step) in function.steps.iter().enumerate() {
                if step.id.is_empty() {
                    problems.push(format!(
                        "functions[{index}].steps[{step_index}].id must not be empty"
                    ));
                } else if !seen_step_ids.insert(step.id.clone()) {
                    problems.push(format!(
                        "functions[{index}].steps[{step_index}].id is duplicated within the function: {:?}",
                        step.id
                    ));
                }
            }
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "FnGraph validation failed with {} problem(s):\n  - {}",
                problems.len(),
                problems.join("\n  - ")
            ))
        }
    }
}

/// One discovered function with its call edges, body steps and source slice.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FnRecord {
    /// Stable file-and-name slug used as the graph-wide primary key.
    pub id: String,
    /// Bare exported name, used for name-to-id resolution during scanning.
    pub name: String,
    /// Owning module or source scope, shown in the UI and checked by validation.
    pub module: String,
    /// Source file path relative to the scan root.
    #[serde(rename = "filePath")]
    pub file_path: String,
    /// 1-based line of the declaration, so editors can jump to the function.
    pub line: usize,
    /// Digest of comment- and whitespace-normalized source for cache lookup.
    #[serde(rename = "astHash")]
    pub ast_hash: String,
    /// Function revision, defaulting to `1.0.0` for older manifests.
    #[serde(default = "default_version")]
    pub version: String,
    /// Runtime label such as `node20`, `edge` or `universal`.
    pub runtime: String,
    /// Whether simulation can safely replay this function.
    pub purity: Purity,
    /// Execution budget in milliseconds, surfaced as a warning when exceeded.
    #[serde(rename = "timeoutMs")]
    pub timeout_ms: u64,
    /// Capability names the function may need when invoked for real.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Human-readable purpose, enriched from JSDoc or a language model.
    pub description: String,
    /// Free-form JSDoc tags used for filtering in the UI.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Reverse call edges; external entrypoints carry a null `fnId`.
    #[serde(rename = "calledBy", default)]
    pub called_by: Vec<CallerRef>,
    /// Declared parameters in source order.
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    /// Ordered body steps used to build invocation traces.
    #[serde(default)]
    pub steps: Vec<Step>,
    /// Shape of the value returned to callers.
    pub returns: ReturnShape,
    /// Documented or statically detected failure modes.
    #[serde(default)]
    pub errors: Vec<ErrorSpec>,
    /// Exact source slice from the first to the last byte of the function node.
    #[serde(rename = "sourceCode")]
    pub source_code: String,
}

impl Default for FnRecord {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            module: String::new(),
            file_path: String::new(),
            line: 1,
            ast_hash: String::new(),
            version: SCHEMA_VERSION.to_owned(),
            runtime: "universal".to_owned(),
            purity: Purity::Pure,
            timeout_ms: 0,
            permissions: Vec::new(),
            description: String::new(),
            tags: Vec::new(),
            called_by: Vec::new(),
            parameters: Vec::new(),
            steps: Vec::new(),
            returns: ReturnShape::default(),
            errors: Vec::new(),
            source_code: String::new(),
        }
    }
}

/// An incoming call edge, pointing to a graph id or to an external entrypoint.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CallerRef {
    /// Graph id of the caller, or null when the caller lives outside the scan.
    #[serde(rename = "fnId")]
    pub fn_id: Option<String>,
    /// Caller name, kept readable even when no id could be resolved.
    pub name: String,
    /// Module containing the call site.
    pub module: String,
    /// `file:line` style location of the call, used by the UI.
    #[serde(rename = "callSite")]
    pub call_site: String,
}

/// A declared function parameter, including its optional enriched default.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Parameter {
    /// Parameter name as written in source.
    pub name: String,
    /// Type annotation text, or an inferred type from the parser.
    #[serde(rename = "type")]
    pub type_: String,
    /// Whether the caller must supply this parameter.
    pub required: bool,
    /// Known default expression cached from earlier scans.
    #[serde(rename = "defaultValue")]
    pub default_value: Option<serde_json::Value>,
    /// Purpose of the parameter, shown beside it in the UI.
    pub description: String,
}

/// One ordered body step, the unit used for traces and enrichment.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// Stable per-function key such as `step-1`, checked by validation.
    pub id: String,
    /// What the step does, driving trace status and UI icons.
    pub kind: StepKind,
    /// Resolved callee id for `Call` steps when the callee was discovered.
    #[serde(rename = "callsFnId")]
    pub calls_fn_id: Option<String>,
    /// Short human-readable summary of the step.
    pub title: String,
    /// Source expression that produced the step.
    pub code: String,
    /// Names of values read by the step.
    #[serde(default)]
    pub inputs: Vec<String>,
    /// Name or description of the value produced by the step.
    pub output: String,
    /// Longer explanation added by enrichment or deterministic fallbacks.
    pub detail: String,
}

/// Shape of the value a function returns, including object fields when known.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReturnShape {
    /// Type annotation text, defaulting to `void` when nothing is returned.
    #[serde(rename = "type")]
    pub type_: String,
    /// Named members of an object return type.
    #[serde(default)]
    pub fields: Vec<ReturnField>,
}

impl Default for ReturnShape {
    fn default() -> Self {
        Self {
            type_: "void".to_owned(),
            fields: Vec::new(),
        }
    }
}

/// A single named member of an object return type.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReturnField {
    /// Property name on the returned object.
    pub name: String,
    /// Type annotation text of the property.
    #[serde(rename = "type")]
    pub type_: String,
}

/// A documented or statically detected failure mode of a function.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorSpec {
    /// Stable error code used by callers to branch on failures.
    pub code: String,
    /// HTTP-like status hint when the error crosses an API boundary.
    pub status: Option<u16>,
    /// Condition that triggers the error.
    pub when: String,
}

/// Enrichment results cached on disk across scans, keyed by function hash.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnrichCache {
    /// Cache format revision, used to discard entries written by older tools.
    pub version: u32,
    /// Entries indexed by `FnRecord::ast_hash`.
    #[serde(default)]
    pub entries: BTreeMap<String, EnrichEntry>,
}

impl Default for EnrichCache {
    fn default() -> Self {
        Self {
            version: 1,
            entries: BTreeMap::new(),
        }
    }
}

/// Enriched text for a single cached function, reused when its hash is unchanged.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnrichEntry {
    /// Cached purpose text that replaces the deterministic fallback.
    pub description: Option<String>,
    /// Cached step titles aligned with the graph's step order.
    #[serde(rename = "stepTitles", default)]
    pub step_titles: Vec<String>,
    /// Cached step details aligned with the graph's step order.
    #[serde(rename = "stepDetails", default)]
    pub step_details: Vec<String>,
    /// Cached parameter defaults indexed by parameter name.
    #[serde(default)]
    pub defaults: BTreeMap<String, serde_json::Value>,
    /// Cached tags that replace the parsed JSDoc tags.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Request body of `POST /api/invoke`, naming the function and its arguments.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InvokeRequest {
    /// Id of the function to invoke.
    #[serde(rename = "fnId")]
    pub fn_id: String,
    /// JSON object of named arguments passed to the function.
    pub args: serde_json::Value,
}

/// Response body of `POST /api/invoke`, carrying data, trace and timing.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InvokeResponse {
    /// Whether invocation completed without an error.
    pub ok: bool,
    /// Returned value on success.
    pub data: Option<serde_json::Value>,
    /// Human-readable failure description when `ok` is false.
    pub error: Option<String>,
    /// Per-step trace indexed by step id.
    #[serde(default)]
    pub trace: BTreeMap<String, TraceEntry>,
    /// Wall-clock duration of the invocation in milliseconds.
    #[serde(rename = "durationMs")]
    pub duration_ms: f64,
}

/// One step's observed status and output inside an invocation trace.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TraceEntry {
    /// Step outcome, such as `simulated` or `ok`.
    pub status: String,
    /// Value or explanation produced by the step.
    pub output: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// Builds a fully populated graph so serialization covers every renamed field.
    fn sample_record() -> FnRecord {
        FnRecord {
            id: "src-grades-evaluateGrammarRole".to_owned(),
            name: "evaluateGrammarRole".to_owned(),
            module: "src/grades.ts".to_owned(),
            file_path: "src/grades.ts".to_owned(),
            line: 42,
            ast_hash: format!("sha256:{}", "a".repeat(64)),
            version: "1.0.0".to_owned(),
            runtime: "node20 / edge".to_owned(),
            purity: Purity::Pure,
            timeout_ms: 1500,
            permissions: vec!["fs:read".to_owned()],
            description: "Decides which grammar role applies.".to_owned(),
            tags: vec!["grammar".to_owned()],
            called_by: vec![CallerRef {
                fn_id: Some("src-main-run".to_owned()),
                name: "run".to_owned(),
                module: "src/main.ts".to_owned(),
                call_site: "src/main.ts:10".to_owned(),
            }],
            parameters: vec![Parameter {
                name: "input".to_owned(),
                type_: "string".to_owned(),
                required: true,
                default_value: Some(json!({ "kind": "text", "limit": 10 })),
                description: "Raw input text.".to_owned(),
            }],
            steps: vec![
                Step {
                    id: "step-1".to_owned(),
                    kind: StepKind::Call,
                    calls_fn_id: Some("src-parse-tokenize".to_owned()),
                    title: "Tokenize input".to_owned(),
                    code: "tokenize(input)".to_owned(),
                    inputs: vec!["input".to_owned()],
                    output: "tokens".to_owned(),
                    detail: "Splits text into tokens.".to_owned(),
                },
                Step {
                    id: "step-2".to_owned(),
                    kind: StepKind::Return,
                    calls_fn_id: None,
                    title: "Return verdict".to_owned(),
                    code: "return verdict".to_owned(),
                    inputs: vec!["tokens".to_owned()],
                    output: "verdict".to_owned(),
                    detail: "Returns the selected role.".to_owned(),
                },
            ],
            returns: ReturnShape {
                type_: "object".to_owned(),
                fields: vec![ReturnField {
                    name: "role".to_owned(),
                    type_: "string".to_owned(),
                }],
            },
            errors: vec![ErrorSpec {
                code: "E_PARSE".to_owned(),
                status: Some(400),
                when: "input cannot be tokenized".to_owned(),
            }],
            source_code: "export function evaluateGrammarRole(input) { /* ... */ }".to_owned(),
        }
    }

    /// Wraps the sample record in a graph with all root-level fields populated.
    fn sample_graph() -> FnGraph {
        FnGraph {
            tool: "fnscope".to_owned(),
            schema_version: "1.0.0".to_owned(),
            generated_at: "2026-11-25T12:00:00Z".to_owned(),
            root_dir: "project".to_owned(),
            functions: vec![sample_record()],
        }
    }

    /// Recursively asserts that no Rust snake_case field name leaked into JSON.
    fn assert_no_snake_case_keys(value: &Value) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    assert!(!key.contains('_'), "snake_case key leaked into JSON: {key}");
                    assert_no_snake_case_keys(child);
                }
            }
            Value::Array(items) => {
                for item in items {
                    assert_no_snake_case_keys(item);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn serialize_uses_exact_camel_case_keys() {
        let graph_json = serde_json::to_value(sample_graph()).expect("graph must serialize");
        let record_json = serde_json::to_value(sample_record()).expect("record must serialize");

        assert_eq!(graph_json["schemaVersion"], "1.0.0");
        assert_eq!(graph_json["generatedAt"], "2026-11-25T12:00:00Z");
        assert_eq!(graph_json["rootDir"], "project");
        assert_eq!(record_json["filePath"], "src/grades.ts");
        assert_eq!(record_json["astHash"], format!("sha256:{}", "a".repeat(64)));
        assert_eq!(record_json["timeoutMs"], 1500);
        assert_eq!(record_json["calledBy"][0]["fnId"], "src-main-run");
        assert_eq!(record_json["calledBy"][0]["callSite"], "src/main.ts:10");
        assert_eq!(record_json["parameters"][0]["type"], "string");
        assert_eq!(record_json["parameters"][0]["defaultValue"]["limit"], 10);
        assert_eq!(record_json["steps"][0]["callsFnId"], "src-parse-tokenize");
        assert_eq!(record_json["returns"]["fields"][0]["type"], "string");
        assert!(record_json["sourceCode"].is_string());

        assert_no_snake_case_keys(&graph_json);
        assert_no_snake_case_keys(&record_json);
    }

    #[test]
    fn enums_serialize_with_contract_spellings() {
        assert_eq!(serde_json::to_value(Purity::Pure).unwrap(), json!("pure"));
        assert_eq!(
            serde_json::to_value(Purity::SideEffect).unwrap(),
            json!("side-effect")
        );
        assert_eq!(serde_json::to_value(StepKind::Call).unwrap(), json!("call"));
        assert_eq!(
            serde_json::to_value(StepKind::Branch).unwrap(),
            json!("branch")
        );
        assert_eq!(
            serde_json::to_value(StepKind::Transform).unwrap(),
            json!("transform")
        );
        assert_eq!(
            serde_json::to_value(StepKind::Return).unwrap(),
            json!("return")
        );

        assert_eq!(
            serde_json::from_value::<Purity>(json!("side-effect")).unwrap(),
            Purity::SideEffect
        );
        assert_eq!(
            serde_json::from_value::<StepKind>(json!("transform")).unwrap(),
            StepKind::Transform
        );
    }

    #[test]
    fn graph_round_trips_through_json() {
        let graph = sample_graph();
        let json_text = serde_json::to_string(&graph).expect("graph must serialize");
        let decoded: FnGraph = serde_json::from_str(&json_text).expect("graph must deserialize");
        assert_eq!(decoded, graph);
    }

    #[test]
    fn missing_vector_fields_default_to_empty() {
        let graph: FnGraph = serde_json::from_value(json!({
            "tool": "fnscope",
            "schemaVersion": "1.0.0",
            "generatedAt": "",
            "rootDir": "project"
        }))
        .expect("older manifests may omit functions");
        assert!(graph.functions.is_empty());

        let ast_hash = format!("sha256:{}", "a".repeat(64));
        let record: FnRecord = serde_json::from_value(json!({
            "id": "f",
            "name": "f",
            "module": "m",
            "filePath": "f.ts",
            "line": 1,
            "astHash": ast_hash,
            "runtime": "universal",
            "purity": "pure",
            "timeoutMs": 0,
            "description": "",
            "sourceCode": "",
            "returns": { "type": "void" }
        }))
        .expect("older records may omit every vector field");
        assert_eq!(record.version, "1.0.0");
        assert!(record.permissions.is_empty());
        assert!(record.tags.is_empty());
        assert!(record.called_by.is_empty());
        assert!(record.parameters.is_empty());
        assert!(record.steps.is_empty());
        assert!(record.errors.is_empty());
        assert!(record.returns.fields.is_empty());
    }

    #[test]
    fn validate_accepts_a_well_formed_graph() {
        assert!(sample_graph().validate().is_ok());

        let mut empty = sample_graph();
        empty.functions.clear();
        assert!(empty.validate().is_ok(), "empty function lists are allowed");
    }

    #[test]
    fn validate_aggregates_every_problem() {
        let mut graph = sample_graph();
        graph.tool = "other-tool".to_owned();
        graph.schema_version = "2.0.0".to_owned();

        let duplicate_id = graph.functions[0].id.clone();
        let first_steps = graph.functions[0].steps.clone();
        let mut duplicate = sample_record();
        duplicate.id = duplicate_id;
        duplicate.ast_hash = "not-a-hash".to_owned();
        duplicate.line = 0;
        duplicate.module.clear();
        duplicate.file_path.clear();
        duplicate.steps = vec![
            Step {
                id: "step-1".to_owned(),
                ..first_steps[0].clone()
            },
            Step {
                id: "step-1".to_owned(),
                ..first_steps[1].clone()
            },
            Step {
                id: String::new(),
                ..first_steps[1].clone()
            },
        ];
        graph.functions.push(duplicate);

        let error = graph.validate().expect_err("faulty graph must be rejected");
        // Only faulted step indices appear: step-1 at index 0 is valid, so just
        // the duplicate at index 1 and the empty id at index 2 are reported.
        for fragment in [
            "tool",
            "fnscope",
            "schemaVersion",
            "1.0.0",
            "duplicated",
            "sha256",
            "64 hex",
            "line",
            "module",
            "filePath",
            "steps[1].id",
            "steps[2].id",
            "must not be empty",
        ] {
            assert!(
                error.contains(fragment),
                "aggregated error must mention {fragment:?}:\n{error}"
            );
        }
    }

    #[test]
    fn validate_rejects_malformed_hashes() {
        let bad_hashes = [
            String::new(),
            "sha256:".to_owned(),
            format!("sha256:{}", "z".repeat(64)),
            format!("sha256:{}", "a".repeat(63)),
            format!("md5:{}", "a".repeat(64)),
        ];
        for bad_hash in bad_hashes {
            let mut graph = sample_graph();
            graph.functions[0].ast_hash = bad_hash.clone();
            assert!(
                graph.validate().is_err(),
                "bad astHash must be rejected: {bad_hash:?}"
            );
        }

        let mut graph = sample_graph();
        graph.functions[0].ast_hash = format!("sha256:{}", "Ab09".repeat(16));
        assert!(graph.validate().is_ok(), "uppercase hex digits are valid");
    }

    #[test]
    fn enrichment_cache_round_trips() {
        let mut entries = BTreeMap::new();
        entries.insert(
            format!("sha256:{}", "b".repeat(64)),
            EnrichEntry {
                description: Some("Cached purpose.".to_owned()),
                step_titles: vec!["Tokenize".to_owned(), "Return".to_owned()],
                step_details: vec!["Split input.".to_owned(), "Produce result.".to_owned()],
                defaults: BTreeMap::from([("limit".to_owned(), json!(10))]),
                tags: vec!["cached".to_owned()],
            },
        );
        let cache = EnrichCache {
            version: 1,
            entries,
        };

        let cache_json = serde_json::to_value(&cache).expect("cache must serialize");
        assert!(cache_json.to_string().contains("stepTitles"));
        assert!(cache_json.to_string().contains("stepDetails"));
        assert_no_snake_case_keys(&cache_json);

        let decoded: EnrichCache =
            serde_json::from_value(cache_json).expect("cache must deserialize");
        assert_eq!(decoded, cache);
    }

    #[test]
    fn invoke_messages_use_fn_id_and_duration_ms() {
        let request = InvokeRequest {
            fn_id: "src-grades-evaluateGrammarRole".to_owned(),
            args: json!({ "input": "hello" }),
        };
        let request_json = serde_json::to_value(&request).expect("request must serialize");
        assert_eq!(request_json["fnId"], "src-grades-evaluateGrammarRole");
        assert!(request_json.get("fn_id").is_none());
        let decoded_request: InvokeRequest =
            serde_json::from_value(request_json).expect("request must deserialize");
        assert_eq!(decoded_request, request);

        let response = InvokeResponse {
            ok: true,
            data: Some(json!({ "role": "noun" })),
            error: None,
            trace: BTreeMap::from([(
                "step-1".to_owned(),
                TraceEntry {
                    status: "simulated".to_owned(),
                    output: "tokens".to_owned(),
                },
            )]),
            duration_ms: 12.5,
        };
        let response_json = serde_json::to_value(&response).expect("response must serialize");
        assert_eq!(response_json["durationMs"], 12.5);
        assert!(response_json.get("duration_ms").is_none());
        let decoded_response: InvokeResponse =
            serde_json::from_value(response_json).expect("response must deserialize");
        assert_eq!(decoded_response, response);
    }

    #[test]
    fn return_shape_default_is_void() {
        let shape = ReturnShape::default();
        assert_eq!(shape.type_, "void");
        assert!(shape.fields.is_empty());
    }
}
