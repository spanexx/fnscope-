//! Swift adapter.
//!
//! All Swift grammar knowledge lives in this module. The shared type-name
//! resolver in `crate::resolve` is grammar-free, so this adapter reuses it for
//! `resolve_return_fields` like the other language adapters do.

mod body;
mod discover;

use std::collections::BTreeMap;

use fnscope_model::ReturnShape;

use crate::adapter::LanguageAdapter;
use crate::resolve::resolve_fields;
use crate::types::{BodyAnalysis, DiscoveredFn};

/// Stateless adapter for `.swift` sources.
pub struct SwiftAdapter;

impl SwiftAdapter {
    /// Construct an adapter; registration needs no configuration or global state.
    pub fn new() -> Self {
        Self
    }
}

impl Default for SwiftAdapter {
    /// Return the stateless adapter so registration can use `Box::default()`.
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageAdapter for SwiftAdapter {
    fn id(&self) -> &'static str {
        "swift"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["swift"]
    }

    fn discover(&self, source: &str, relative_path: &str) -> Vec<DiscoveredFn> {
        discover::discover(source, relative_path)
    }

    fn analyze(&self, found: &DiscoveredFn, source: &str, known_names: &[String]) -> BodyAnalysis {
        body::analyze(found, source, known_names)
    }

    fn collect_types(&self, source: &str) -> BTreeMap<String, ReturnShape> {
        discover::collect_types(source)
    }

    fn resolve_return_fields(
        &self,
        found: &mut DiscoveredFn,
        types: &BTreeMap<String, ReturnShape>,
    ) {
        resolve_fields(found, types);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FnKind;
    use fnscope_model::{Purity, StepKind};
    use serde_json::json;

    const FREE: &str = r#"
/// Greets one person.
func greet(name: String) -> String {
    return "Hello, \(name)"
}
"#;

    /// A top-level function keeps its bare name, type and doc description.
    #[test]
    fn discovers_a_free_function() {
        let found = SwiftAdapter::new().discover(FREE, "greet.swift");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "greet");
        assert_eq!(found[0].qualified_name, "greet");
        assert_eq!(found[0].kind, FnKind::Declaration);
        assert_eq!(found[0].class_name, None);
        assert_eq!(found[0].returns.type_, "String");
        assert_eq!(
            found[0].doc_description.as_deref(),
            Some("Greets one person.")
        );
        assert_eq!(found[0].parameters.len(), 1);
        assert_eq!(found[0].parameters[0].name, "name");
        assert_eq!(found[0].parameters[0].type_, "String");
    }

    /// Struct methods and initializers are qualified by their owning type.
    #[test]
    fn discovers_struct_methods_and_initializers() {
        let source = r#"
struct Calculator {
    var multiplier: Int

    /// Multiplies by the stored factor.
    func multiply(_ value: Int) -> Int {
        return value * multiplier
    }

    init(multiplier: Int) {
        self.multiplier = multiplier
    }
}
"#;
        let found = SwiftAdapter::new().discover(source, "calculator.swift");
        assert_eq!(found.len(), 2);
        let method = found
            .iter()
            .find(|fn_| fn_.qualified_name == "Calculator.multiply")
            .expect("method must be discovered");
        assert_eq!(method.kind, FnKind::Method);
        assert_eq!(method.class_name.as_deref(), Some("Calculator"));
        assert_eq!(method.returns.type_, "Int");
        let initializer = found
            .iter()
            .find(|fn_| fn_.qualified_name == "Calculator.init")
            .expect("initializer must be discovered");
        assert_eq!(initializer.kind, FnKind::Method);
        assert_eq!(initializer.class_name.as_deref(), Some("Calculator"));
    }

    /// External labels stay in the parameter description; defaults become JSON.
    #[test]
    fn reads_parameter_labels_and_defaults() {
        let source = "func configure(label name: String, retries: Int = 3, verbose: Bool = true) {\n    print(name)\n}";
        let found = SwiftAdapter::new().discover(source, "configure.swift");
        assert_eq!(found.len(), 1);
        let params = &found[0].parameters;
        assert_eq!(params.len(), 3);
        assert_eq!(params[0].name, "name");
        assert!(params[0].description.contains("external label: label"));
        assert_eq!(params[1].name, "retries");
        assert!(!params[1].required);
        assert_eq!(params[1].default_value, Some(json!(3)));
        assert_eq!(params[2].name, "verbose");
        assert!(!params[2].required);
        assert_eq!(params[2].default_value, Some(json!(true)));
    }

    /// Stored properties and aliases populate the type index and return fields.
    #[test]
    fn indexes_types_and_resolves_alias_fields() {
        let source = r#"
struct Point {
    var x: Double
    var y: Double
}
typealias Coordinate = Point
func origin() -> Coordinate {
    return Coordinate(x: 0, y: 0)
}
"#;
        let adapter = SwiftAdapter::new();
        let types = adapter.collect_types(source);
        let point = types.get("Point").expect("Point must be indexed");
        assert_eq!(point.fields.len(), 2);
        assert_eq!(point.fields[0].name, "x");
        assert_eq!(point.fields[0].type_, "Double");
        assert_eq!(point.fields[1].name, "y");
        let alias = types.get("Coordinate").expect("alias must be indexed");
        assert_eq!(alias.fields.len(), 2);

        let found = adapter.discover(source, "geometry.swift");
        let origin = found
            .iter()
            .find(|fn_| fn_.name == "origin")
            .expect("origin must be discovered");
        assert_eq!(origin.returns.type_, "Coordinate");
        assert_eq!(origin.returns.fields.len(), 2);

        let mut bare = origin.clone();
        bare.returns.fields.clear();
        adapter.resolve_return_fields(&mut bare, &types);
        assert_eq!(bare.returns.fields.len(), 2);
    }

    /// Body steps are top-level statements in source order with fixed kinds.
    #[test]
    fn builds_call_branch_transform_and_return_steps() {
        let source = r#"
func load(id: Int) -> Int {
    let value = normalize(id)
    if value < 1 {
        return 0
    }
    let doubled = value * 2
    return doubled
}
"#;
        let adapter = SwiftAdapter::new();
        let found = adapter.discover(source, "load.swift");
        let analysis = adapter.analyze(&found[0], source, &[]);
        let kinds: Vec<StepKind> = analysis.steps.iter().map(|step| step.kind).collect();
        assert_eq!(
            kinds,
            vec![
                StepKind::Call,
                StepKind::Branch,
                StepKind::Transform,
                StepKind::Return
            ]
        );
        assert!(analysis.callee_names.iter().any(|name| name == "normalize"));
        assert_eq!(analysis.steps[0].title, "Call normalize()");
        assert_eq!(analysis.steps[3].output, "Int");
    }

    /// Guard and `if` throws map to codes, statuses and enclosing conditions.
    #[test]
    fn extracts_guarded_and_explicit_throws() {
        let source = r#"
func fetch(code: Int) throws -> Int {
    guard code > 0 else {
        throw FunctionError(code: "EMPTY_TEXT")
    }
    if code == 7 {
        throw ValidationError.tooShort
    }
    return code
}
"#;
        let adapter = SwiftAdapter::new();
        let found = adapter.discover(source, "fetch.swift");
        let analysis = adapter.analyze(&found[0], source, &[]);
        assert_eq!(analysis.errors.len(), 2);
        assert_eq!(analysis.errors[0].code, "EMPTY_TEXT");
        assert_eq!(analysis.errors[0].status, Some(400));
        assert_eq!(analysis.errors[0].when, "code > 0");
        assert_eq!(analysis.errors[1].code, "tooShort");
        assert_eq!(analysis.errors[1].when, "code == 7");
    }

    /// Console, filesystem, network and environment markers become permissions.
    #[test]
    fn classifies_side_effects_and_budgets() {
        let source = r#"
func report(key: String) {
    print(key)
    let token = ProcessInfo.processInfo.environment["API_KEY"]
    let data = Data(contentsOf: URL(string: "https://example.com")!)
    let manager = FileManager.default
    let session = URLSession.shared
    os_log("done")
}
"#;
        let adapter = SwiftAdapter::new();
        let found = adapter.discover(source, "report.swift");
        let analysis = adapter.analyze(&found[0], source, &[]);
        assert_eq!(analysis.purity, Purity::SideEffect);
        for permission in [
            "console:out",
            "fs:read",
            "net:outbound",
            "secrets:read:API_KEY",
        ] {
            assert!(
                analysis.permissions.iter().any(|p| p == permission),
                "missing permission {permission}"
            );
        }
        assert_eq!(analysis.timeout_ms, 5000);
        assert_eq!(analysis.runtime, "node20");
    }

    /// Discovery, analysis, identity and defaults are byte-for-byte repeatable.
    #[test]
    fn repeated_runs_are_deterministic() {
        let adapter = SwiftAdapter::new();
        let first = adapter.discover(FREE, "greet.swift");
        let second = SwiftAdapter::new().discover(FREE, "greet.swift");
        assert_eq!(first, second);
        let first_analysis = adapter.analyze(&first[0], FREE, &[]);
        let second_analysis = SwiftAdapter::default().analyze(&second[0], FREE, &[]);
        assert_eq!(first_analysis, second_analysis);
        assert_eq!(SwiftAdapter::default().id(), "swift");
        assert_eq!(SwiftAdapter::new().extensions(), &["swift"]);
        assert_eq!(first_analysis.timeout_ms, 150);
        assert_eq!(first_analysis.runtime, "universal");
        assert_eq!(first_analysis.purity, Purity::Pure);
    }

    /// A function declared inside another body is local and is never discovered.
    #[test]
    fn skips_nested_local_functions() {
        let source = "func outer() {\n  func inner() {}\n  inner()\n}\n";
        let found = SwiftAdapter::new().discover(source, "outer.swift");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "outer");
    }

    /// A project-wide type index merges across files so cross-file returns resolve.
    #[test]
    fn resolves_return_fields_from_a_merged_cross_file_index() {
        let adapter = SwiftAdapter::new();
        let first = adapter.collect_types("struct Local {\n  var id: Int\n}\n");
        let second =
            adapter.collect_types("struct Remote {\n  var name: String\n  var flag: Bool\n}\n");
        let mut combined = first;
        combined.extend(second);
        assert_eq!(combined.len(), 2);

        let source = "func find() -> Remote {\n  return Remote(name: \"x\", flag: true)\n}\n";
        let found = adapter.discover(source, "find.swift");
        assert_eq!(found.len(), 1);
        let mut record = found[0].clone();
        // Remote lives in a different source, so per-file discovery cannot resolve it.
        assert!(record.returns.fields.is_empty());
        adapter.resolve_return_fields(&mut record, &combined);
        assert_eq!(record.returns.fields.len(), 2);
        assert_eq!(record.returns.fields[0].name, "name");
        assert_eq!(record.returns.fields[0].type_, "String");
        assert_eq!(record.returns.fields[1].name, "flag");
        assert_eq!(record.returns.fields[1].type_, "Bool");
    }
}
