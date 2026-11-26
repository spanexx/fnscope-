//! End-to-end checks against the committed sample fixture.

use std::path::{Path, PathBuf};

use fnscope_core::{scan, ScanOptions};
use fnscope_model::{FnGraph, FnRecord, Purity};

/// Absolute path of the fixture, derived from the crate location only.
fn fixture_source_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/sample_app/src")
}

/// Scans the fixture at a fixed instant so comparisons stay deterministic.
fn scan_fixture() -> FnGraph {
    let options = ScanOptions {
        target_dir: fixture_source_dir(),
        ..ScanOptions::default()
    };
    scan(
        &options,
        &PathBuf::from(".fnscope"),
        "2026-11-25T00:00:00Z".to_owned(),
    )
    .expect("the sample fixture must scan successfully")
    .0
}

/// Finds a record by its bare source name, which is stable across id schemes.
fn by_name<'a>(graph: &'a FnGraph, name: &str) -> &'a FnRecord {
    graph
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("fixture should contain a function named {name:?}"))
}

#[test]
fn scan_discovers_functions_and_honors_default_excludes() {
    let graph = scan_fixture();
    assert!(
        graph.functions.len() >= 8,
        "expected at least 8 functions, got {}",
        graph.functions.len()
    );
    for function in &graph.functions {
        assert!(!function.file_path.ends_with(".test.ts"));
        assert!(!function.file_path.ends_with(".test.tsx"));
        assert!(!function.file_path.ends_with(".spec.ts"));
        assert!(!function.file_path.ends_with(".spec.tsx"));
    }
    let evaluation = by_name(&graph, "evaluateGrammarRole");
    assert!(evaluation.id.contains("evaluate-grammar-role"));
}

#[test]
fn scan_builds_a_bidirectional_call_graph() {
    let graph = scan_fixture();
    let evaluation = by_name(&graph, "evaluateGrammarRole");
    let grading = by_name(&graph, "gradeSentenceExercise");
    assert!(
        evaluation
            .called_by
            .iter()
            .any(|caller| caller.fn_id.as_deref() == Some(grading.id.as_str())),
        "evaluateGrammarRole must list gradeSentenceExercise as an internal caller"
    );

    let ids: Vec<&str> = graph
        .functions
        .iter()
        .map(|function| function.id.as_str())
        .collect();
    for function in &graph.functions {
        for step in &function.steps {
            if let Some(callee) = &step.calls_fn_id {
                assert!(
                    ids.contains(&callee.as_str()),
                    "{} calls unresolved id {callee:?}",
                    function.id
                );
            }
        }
    }
}

#[test]
fn scan_carries_documented_error_statuses() {
    let graph = scan_fixture();
    assert!(
        graph
            .functions
            .iter()
            .flat_map(|function| function.errors.iter())
            .any(|error| error.status == Some(422)),
        "the fixture must produce at least one 422 error"
    );
}

#[test]
fn scan_propagates_side_effects_and_capabilities() {
    let graph = scan_fixture();
    let delivery = by_name(&graph, "dispatchWebhookEvent");
    assert_eq!(delivery.purity, Purity::SideEffect);
    assert!(
        delivery
            .permissions
            .iter()
            .any(|permission| permission == "net:outbound"),
        "network delivery must require net:outbound: {:?}",
        delivery.permissions
    );
    assert!(
        delivery
            .permissions
            .iter()
            .any(|permission| permission.starts_with("secrets:read:")),
        "reading the signing secret must require a secrets:read permission: {:?}",
        delivery.permissions
    );
}

#[test]
fn scan_resolves_return_fields_from_project_wide_types() {
    let graph = scan_fixture();
    let evaluation = by_name(&graph, "evaluateGrammarRole");
    assert!(
        !evaluation.returns.fields.is_empty(),
        "RoleEvaluationResult is declared in types.ts, not roles.ts"
    );

    let role_id = evaluation
        .returns
        .fields
        .iter()
        .find(|field| field.name == "roleId")
        .expect("roleId must resolve from the shared interface");
    // types.ts declares `roleId: 'doer' | 'engine' | ...`, not plain `string`.
    assert!(
        role_id.type_.contains("doer"),
        "roleId must keep its declared literal-union type, got {:?}",
        role_id.type_
    );

    let label = evaluation
        .returns
        .fields
        .iter()
        .find(|field| field.name == "label")
        .expect("label must resolve from the shared interface");
    assert_eq!(label.type_, "string");
}

#[test]
fn scan_is_byte_deterministic() {
    let first = serde_json::to_string(&scan_fixture()).expect("first graph must serialize");
    let second = serde_json::to_string(&scan_fixture()).expect("second graph must serialize");
    assert_eq!(first, second);
}

/// Absolute path of the Rust fixture, derived from the crate location only.
fn rust_fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/rust_app")
}

/// Scans the Rust fixture at a fixed instant so comparisons stay deterministic.
fn scan_rust_fixture() -> FnGraph {
    let options = ScanOptions {
        target_dir: rust_fixture_dir(),
        ..ScanOptions::default()
    };
    scan(
        &options,
        &PathBuf::from(".fnscope"),
        "2026-11-25T00:00:00Z".to_owned(),
    )
    .expect("the Rust fixture must scan successfully")
    .0
}

#[test]
fn scan_discovers_rust_functions_methods_and_call_edges() {
    let graph = scan_rust_fixture();

    assert!(
        graph.functions.len() >= 3,
        "expected at least 3 Rust functions, got {}",
        graph.functions.len()
    );

    // The locked model stores only the bare `name`. A dotted qualified name
    // `Type.method` is observable verbatim when the parser reports it that way,
    // or as the kebab-case id `type-method` that core derives from it.
    let has_impl_method = graph.functions.iter().any(|function| {
        let last_segment = function
            .name
            .split(|ch| ch == '.' || ch == ':')
            .last()
            .unwrap_or("");
        function.name.contains('.')
            || function.name.contains("::")
            || (function.id.contains('-')
                && !last_segment.is_empty()
                && function.id.ends_with(&format!("-{last_segment}")))
    });
    assert!(
        has_impl_method,
        "at least one impl method with a dotted qualified name must be discovered"
    );

    // A reverse edge whose fnId names another discovered function proves that
    // core resolved a call between two Rust functions (not an external entry).
    let has_rust_call_edge = graph.functions.iter().any(|callee| {
        callee.called_by.iter().any(|caller| {
            caller.fn_id.as_deref().is_some_and(|id| {
                id != callee.id && graph.functions.iter().any(|function| function.id == id)
            })
        })
    });
    assert!(
        has_rust_call_edge,
        "a called_by edge between two Rust functions is required"
    );

    for function in &graph.functions {
        let path = function.file_path.replace('\\', "/");
        assert!(
            !path.split('/').any(|part| part == "target"),
            "Cargo build output must be excluded, got {}",
            function.file_path
        );
    }
}
