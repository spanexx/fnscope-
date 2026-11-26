//! C# language adapter.
//!
//! All C# grammar knowledge lives in this module and its two children. The
//! shared type-name resolver in `crate::resolve` is grammar-free, so it is
//! reused here exactly as the Python and Go adapters reuse it. This job does
//! not register the adapter globally; registration is a separate, reviewed
//! change.

mod body;
mod discover;

use std::collections::BTreeMap;

use fnscope_model::ReturnShape;

use crate::adapter::LanguageAdapter;
use crate::resolve::resolve_fields;
use crate::types::{BodyAnalysis, DiscoveredFn};

/// Stateless adapter for `.cs` sources, held by a caller-built registry.
pub struct CSharpAdapter;

impl CSharpAdapter {
    /// Construct the stateless adapter; all grammar state is local to each call.
    pub fn new() -> Self {
        Self
    }
}

impl Default for CSharpAdapter {
    /// Return the same stateless value as [`CSharpAdapter::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageAdapter for CSharpAdapter {
    fn id(&self) -> &'static str {
        "csharp"
    }

    fn extensions(&self) -> &'static [&'static str] {
        // `Registry::for_extension` trims a leading dot, so this is `cs`, not `.cs`.
        &["cs"]
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

    /// Identity is contract-stable before any global registration exists.
    #[test]
    fn csharp_adapter_reports_contract_identity() {
        let adapter = CSharpAdapter::new();
        assert_eq!(adapter.id(), "csharp");
        assert_eq!(adapter.extensions(), &["cs"]);
        assert_eq!(CSharpAdapter::default().id(), "csharp");
        assert!(adapter.discover("using System;\n", "App.cs").is_empty());
    }

    /// A namespace-level class method is qualified and documents itself.
    #[test]
    fn discovers_namespace_class_method() {
        let source = concat!(
            "namespace Demo {\n",
            "  /// <summary>Normalize the supplied text.</summary>\n",
            "  // normalize trims the value.\n",
            "  public class Text {\n",
            "    public static string Normalize(string text) { return text.Trim(); }\n",
            "  }\n",
            "}\n",
        );
        let found = CSharpAdapter::new().discover(source, "App.cs");
        assert_eq!(found.len(), 1);
        let method = &found[0];
        assert_eq!(method.name, "Normalize");
        assert_eq!(method.qualified_name, "Text.Normalize");
        assert_eq!(method.kind, FnKind::Method);
        assert_eq!(method.class_name.as_deref(), Some("Text"));
        assert_eq!(method.returns.type_, "string");
        assert_eq!(
            method.doc_description.as_deref(),
            Some("Normalize the supplied text. normalize trims the value.")
        );
        assert_eq!(method.line, 5);
    }

    /// Constructors are discovered as `Class.Class` with a `void` result shape.
    #[test]
    fn discovers_constructor() {
        let source = concat!(
            "class Counter {\n",
            "  public Counter(int seed) { this.seed = seed; }\n",
            "}\n",
        );
        let found = CSharpAdapter::new().discover(source, "Counter.cs");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Counter");
        assert_eq!(found[0].qualified_name, "Counter.Counter");
        assert_eq!(found[0].class_name.as_deref(), Some("Counter"));
        assert_eq!(found[0].returns.type_, "void");
        assert_eq!(found[0].parameters.len(), 1);
    }

    /// A namespace-level struct method is qualified with the struct name.
    #[test]
    fn discovers_struct_method() {
        let source = "namespace Demo { public struct Point { public double Distance(double x) { return x; } } }";
        let found = CSharpAdapter::new().discover(source, "Point.cs");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].qualified_name, "Point.Distance");
        assert_eq!(found[0].class_name.as_deref(), Some("Point"));
        assert_eq!(found[0].kind, FnKind::Method);
    }

    /// Record primary-constructor parameters are discovered as a callable.
    #[test]
    fn discovers_record_primary_constructor() {
        let source = "namespace Demo { public record Ping(string Code, int Retry = 3); }\n";
        let found = CSharpAdapter::new().discover(source, "Ping.cs");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].qualified_name, "Ping.Ping");
        assert_eq!(found[0].parameters.len(), 2);
        assert_eq!(found[0].parameters[0].type_, "string");
        assert_eq!(found[0].parameters[1].name, "Retry");
        assert!(!found[0].parameters[1].required);
    }

    /// Parameter type annotations, optional defaults and source slices survive.
    #[test]
    fn discovers_parameter_types_and_defaults() {
        let source = concat!(
            "class Calc {\n",
            "  // scale multiplies by a configurable factor.\n",
            "  public static double Scale(double value, double factor = 2.5, string tag = \"x\") { return value * factor; }\n",
            "  public static async System.Threading.Tasks.Task<int> LoadAsync(int id) { return id; }\n",
            "}\n",
        );
        let found = CSharpAdapter::new().discover(source, "Calc.cs");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].parameters.len(), 3);
        assert_eq!(found[0].parameters[0].type_, "double");
        assert!(found[0].parameters[0].required);
        assert_eq!(found[0].parameters[1].name, "factor");
        assert_eq!(found[0].parameters[1].type_, "double");
        assert_eq!(
            found[0].parameters[1].default_value,
            Some(serde_json::Value::String("2.5".to_owned()))
        );
        assert_eq!(found[0].parameters[2].type_, "string");
        assert_eq!(
            found[0].parameters[2].default_value,
            Some(serde_json::Value::String("\"x\"".to_owned()))
        );
        assert!(found[0].source_code.contains("Scale("));
        assert!(found[1].is_async);
        assert_eq!(found[1].returns.type_, "System.Threading.Tasks.Task<int>");
    }

    /// A first-level local function is discoverable; one nested inside it is not.
    #[test]
    fn discovers_first_level_local_function_and_skips_deeper_one() {
        let source = concat!(
            "class Runner {\n",
            "  public static string Run(string value) {\n",
            "    string Local(string a) {\n",
            "      string Inner(string b) { return b; }\n",
            "      return Inner(a);\n",
            "    }\n",
            "    return Local(value);\n",
            "  }\n",
            "}\n",
        );
        let found = CSharpAdapter::new().discover(source, "Runner.cs");
        let names: Vec<&str> = found.iter().map(|f| f.qualified_name.as_str()).collect();
        assert_eq!(names, vec!["Runner.Run", "Runner.Run.Local"]);
        assert_eq!(found[1].name, "Local");
        assert_eq!(found[1].kind, FnKind::Declaration);
        assert_eq!(found[1].class_name.as_deref(), Some("Runner"));
        assert_eq!(found[1].returns.type_, "string");
    }

    /// Property and field declarations feed the project-wide type index.
    #[test]
    fn collects_class_properties_and_fields() {
        let source = concat!(
            "namespace Demo {\n",
            "  public class RoleResult {\n",
            "    public string Role { get; set; }\n",
            "    public double Score;\n",
            "    public int Count, Total;\n",
            "  }\n",
            "}\n",
        );
        let types = CSharpAdapter::new().collect_types(source);
        assert_eq!(types.len(), 1);
        let shape = &types["RoleResult"];
        assert_eq!(shape.type_, "RoleResult");
        let fields: Vec<(&str, &str)> = shape
            .fields
            .iter()
            .map(|field| (field.name.as_str(), field.type_.as_str()))
            .collect();
        assert_eq!(
            fields,
            vec![
                ("Role", "string"),
                ("Score", "double"),
                ("Count", "int"),
                ("Total", "int"),
            ]
        );
    }

    /// A declared type from another file is resolved through the shared resolver.
    #[test]
    fn resolves_return_fields_from_external_type_index() {
        let source = "class Builder { public RoleResult Build() { return new RoleResult(); } }\n";
        let types_source = "class RoleResult { public string Role { get; set; } }";
        let found = CSharpAdapter::new().discover(source, "Builder.cs");
        assert_eq!(found[0].returns.type_, "RoleResult");
        assert!(found[0].returns.fields.is_empty());

        let types = CSharpAdapter::new().collect_types(types_source);
        let mut resolved = found[0].clone();
        CSharpAdapter::new().resolve_return_fields(&mut resolved, &types);
        assert_eq!(resolved.returns.fields.len(), 1);
        assert_eq!(resolved.returns.fields[0].name, "Role");
        assert_eq!(resolved.returns.fields[0].type_, "string");
    }

    /// Call initializers, guards, arithmetic and returns become ordered steps.
    #[test]
    fn analyzes_call_branch_transform_and_return() {
        let source = concat!(
            "class Flow {\n",
            "  public static string Run(string text) {\n",
            "    string cleaned = Normalize(text);\n",
            "    if (text.Length == 0) { throw new FunctionError(\"EMPTY_TEXT\"); }\n",
            "    int total = text.Length + 1;\n",
            "    return cleaned;\n",
            "  }\n",
            "}\n",
        );
        let found = CSharpAdapter::new().discover(source, "Flow.cs");
        let analysis = CSharpAdapter::new().analyze(&found[0], source, &["Normalize".to_owned()]);
        let kinds: Vec<StepKind> = analysis.steps.iter().map(|step| step.kind).collect();
        assert_eq!(
            kinds,
            vec![
                StepKind::Call,
                StepKind::Branch,
                StepKind::Transform,
                StepKind::Return,
            ]
        );
        assert!(analysis.steps.iter().all(|step| !step.code.contains('\n')));
        assert!(analysis
            .steps
            .iter()
            .any(|step| step.kind == StepKind::Branch
                && step.output == "throws EMPTY_TEXT | continues"));
        assert!(analysis.callee_names.iter().any(|name| name == "Normalize"));
        assert!(analysis
            .steps
            .iter()
            .any(|step| step.calls_fn_id.as_deref() == Some("Normalize")));
    }

    /// A guarded `throw new FunctionError("CODE")` becomes one deduped ErrorSpec.
    #[test]
    fn extracts_guarded_throw_error() {
        let source = concat!(
            "class Check {\n",
            "  public static string Check(string text) {\n",
            "    if (text.Length == 0) { throw new FunctionError(\"EMPTY_TEXT\"); }\n",
            "    throw new FunctionError(\"EMPTY_TEXT\");\n",
            "  }\n",
            "}\n",
        );
        let found = CSharpAdapter::new().discover(source, "Check.cs");
        let analysis = CSharpAdapter::new().analyze(&found[0], source, &[]);
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "EMPTY_TEXT");
        assert_eq!(analysis.errors[0].status, Some(400));
        assert!(analysis.errors[0].when.contains("text.Length == 0"));
    }

    /// Console, filesystem, network and environment markers classify effects.
    #[test]
    fn classifies_side_effects_and_permissions() {
        let effect = concat!(
            "class Effects {\n",
            "  public static string Run(string url) {\n",
            "    Console.WriteLine(Environment.GetEnvironmentVariable(\"API_KEY\"));\n",
            "    string body = File.ReadAllText(\"config.txt\");\n",
            "    var client = new HttpClient();\n",
            "    return body + client.GetStringAsync(url).Result;\n",
            "  }\n",
            "}\n",
        );
        let found = CSharpAdapter::new().discover(effect, "Effects.cs");
        let analysis = CSharpAdapter::new().analyze(&found[0], effect, &[]);
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

        let pure = "class Pure { public static int Add(int a, int b) { return a + b; } }\n";
        let found = CSharpAdapter::new().discover(pure, "Pure.cs");
        let analysis = CSharpAdapter::new().analyze(&found[0], pure, &[]);
        assert_eq!(analysis.purity, Purity::Pure);
        assert!(analysis.permissions.is_empty());
        assert_eq!(analysis.timeout_ms, 150);
        assert_eq!(analysis.runtime, "universal");
    }

    /// Two passes over the same source produce byte-identical facts.
    #[test]
    fn analysis_is_deterministic() {
        let source = concat!(
            "// Describe the work.\n",
            "class Work {\n",
            "  public static string Run(string text) {\n",
            "    if (text.Length == 0) { throw new FunctionError(\"EMPTY_TEXT\"); }\n",
            "    Console.WriteLine(text);\n",
            "    return text;\n",
            "  }\n",
            "}\n",
        );
        let adapter = CSharpAdapter::new();
        let first = adapter.discover(source, "Work.cs");
        let second = adapter.discover(source, "Work.cs");
        assert_eq!(first, second);
        assert_eq!(
            adapter.analyze(&first[0], source, &[]),
            adapter.analyze(&second[0], source, &[])
        );
    }
}
