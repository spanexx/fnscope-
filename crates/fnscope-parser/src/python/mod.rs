//! Python adapter.
//!
//! All Python grammar knowledge lives in this module. The shared type-name
//! resolver in `crate::resolve` is grammar-free string handling, so it is reused
//! here exactly as the Rust adapter reuses it. This job does not register the
//! adapter globally; registration is a separate, reviewed job.

mod body;
mod discover;

use std::collections::BTreeMap;

use fnscope_model::ReturnShape;

use crate::adapter::LanguageAdapter;
use crate::resolve::resolve_fields;
use crate::types::{BodyAnalysis, DiscoveredFn};

/// Stateless adapter for `.py` sources, held by the registry without global state.
pub struct PythonAdapter;

impl PythonAdapter {
    /// Construct the stateless adapter; all grammar state is local to each call.
    pub fn new() -> Self {
        Self
    }
}

impl Default for PythonAdapter {
    fn default() -> Self {
        Self
    }
}

impl LanguageAdapter for PythonAdapter {
    fn id(&self) -> &'static str {
        "python"
    }

    fn extensions(&self) -> &'static [&'static str] {
        // `Registry::for_extension` trims a leading dot, so this is `py`, not `.py`.
        &["py"]
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

    /// A free function exposes its bare name, parameters, defaults and return annotation.
    #[test]
    fn discovers_free_function_with_parameters_and_defaults() {
        let source = "def greet(name: str, times: int = 2) -> str:\n    return name * times\n";
        let found = PythonAdapter::new().discover(source, "pkg/greet.py");
        assert_eq!(found.len(), 1);
        let function = &found[0];
        assert_eq!(function.name, "greet");
        assert_eq!(function.qualified_name, "greet");
        assert_eq!(function.kind, FnKind::Declaration);
        assert_eq!(function.returns.type_, "str");
        assert_eq!(function.parameters.len(), 2);
        assert_eq!(function.parameters[0].name, "name");
        assert_eq!(function.parameters[0].type_, "str");
        assert!(function.parameters[0].required);
        assert_eq!(function.parameters[1].name, "times");
        assert_eq!(function.parameters[1].type_, "int");
        assert!(!function.parameters[1].required);
        assert_eq!(
            function.parameters[1].default_value,
            Some(serde_json::Value::String("2".to_owned()))
        );
    }

    /// Class methods are qualified as `ClassName.method` and drop the `self` receiver.
    #[test]
    fn discovers_class_method_with_qualified_name() {
        let source =
            "class Greeter:\n    def greet(self, name: str) -> str:\n        return name\n";
        let found = PythonAdapter::new().discover(source, "pkg/greeter.py");
        assert_eq!(found.len(), 1);
        let method = &found[0];
        assert_eq!(method.name, "greet");
        assert_eq!(method.qualified_name, "Greeter.greet");
        assert_eq!(method.kind, FnKind::Method);
        assert_eq!(method.class_name.as_deref(), Some("Greeter"));
        assert_eq!(method.parameters.len(), 1);
        assert_eq!(method.parameters[0].name, "name");
    }

    /// A nested helper inside a body is not a second discoverable function.
    #[test]
    fn ignores_nested_functions() {
        let source = "def outer(x):\n    def inner(y):\n        return y\n    return inner(x)\n";
        let found = PythonAdapter::new().discover(source, "pkg/nested.py");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "outer");
    }

    /// An `async def` sets `is_async` without changing discovery shapes.
    #[test]
    fn flags_async_function() {
        let source = "async def fetch(url: str) -> str:\n    return url\n";
        let found = PythonAdapter::new().discover(source, "pkg/fetch.py");
        assert_eq!(found.len(), 1);
        assert!(found[0].is_async);
    }

    /// Docstrings and leading `#` comments both feed `doc_description`.
    #[test]
    fn uses_docstring_then_leading_hash_comment() {
        let docstring = "def documented(x):\n    \"\"\"Do a useful thing.\"\"\"\n    return x\n";
        let found = PythonAdapter::new().discover(docstring, "pkg/doc.py");
        assert_eq!(
            found[0].doc_description.as_deref(),
            Some("Do a useful thing.")
        );

        let comment = "# Explain the purpose.\ndef commented(x):\n    return x\n";
        let found = PythonAdapter::new().discover(comment, "pkg/comment.py");
        assert_eq!(
            found[0].doc_description.as_deref(),
            Some("Explain the purpose.")
        );
    }

    /// Module-level aliases become type-index entries with the alias name as key.
    #[test]
    fn collects_module_type_aliases() {
        let source = "UserId = int\nAlias = Dict[str, int]\nMAX = 10\n";
        let types = PythonAdapter::new().collect_types(source);
        assert_eq!(types.len(), 2);
        assert_eq!(types["UserId"].type_, "int");
        assert_eq!(types["Alias"].type_, "Dict[str, int]");
        assert!(!types.contains_key("MAX"));
    }

    /// Call, branch, transform and return statements produce ordered, single-line steps.
    #[test]
    fn analyzes_call_branch_transform_and_return_steps() {
        let source = concat!(
            "def normalize(text):\n",
            "    helper(text)\n",
            "    if not text:\n",
            "        raise ValueError(\"EMPTY_TEXT\")\n",
            "    result = text.upper()\n",
            "    return result\n",
        );
        let found = PythonAdapter::new().discover(source, "pkg/normalize.py");
        let analysis = PythonAdapter::new().analyze(&found[0], source, &["helper".to_owned()]);
        let kinds: Vec<StepKind> = analysis.steps.iter().map(|step| step.kind).collect();
        // Spec §4 step 3: `result = text.upper()` invokes a function, so it is a Call step.
        assert_eq!(
            kinds,
            vec![
                StepKind::Call,
                StepKind::Branch,
                StepKind::Call,
                StepKind::Return,
            ]
        );
        assert!(analysis.steps.iter().all(|step| !step.code.contains('\n')));
        assert!(analysis
            .steps
            .iter()
            .any(|step| step.kind == StepKind::Branch
                && step.output == "throws EMPTY_TEXT | continues"));
        assert!(analysis.callee_names.iter().any(|name| name == "helper"));
        assert!(analysis
            .steps
            .iter()
            .any(|step| step.calls_fn_id.as_deref() == Some("helper")));
    }

    /// A guarded `raise ValueError("CODE")` becomes an `ErrorSpec` with a status hint.
    #[test]
    fn extracts_raise_error_with_guard_condition() {
        let source = concat!(
            "def check(text):\n",
            "    if not text:\n",
            "        raise ValueError(\"EMPTY_TEXT\")\n",
            "    return text\n",
        );
        let found = PythonAdapter::new().discover(source, "pkg/check.py");
        let analysis = PythonAdapter::new().analyze(&found[0], source, &[]);
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "EMPTY_TEXT");
        assert_eq!(analysis.errors[0].status, Some(400));
        assert!(analysis.errors[0].when.contains("not text"));
    }

    /// `print` marks a side effect with `console:out`; pure code keeps the base budget.
    #[test]
    fn print_is_side_effect_and_pure_stays_pure() {
        let effect = "def report(text):\n    print(text)\n";
        let found = PythonAdapter::new().discover(effect, "pkg/report.py");
        let analysis = PythonAdapter::new().analyze(&found[0], effect, &[]);
        assert_eq!(analysis.purity, Purity::SideEffect);
        assert!(analysis
            .permissions
            .iter()
            .any(|permission| permission == "console:out"));
        assert_eq!(analysis.timeout_ms, 1000);
        assert_eq!(analysis.runtime, "node20");

        let pure = "def add(a, b):\n    return a + b\n";
        let found = PythonAdapter::new().discover(pure, "pkg/add.py");
        let analysis = PythonAdapter::new().analyze(&found[0], pure, &[]);
        assert_eq!(analysis.purity, Purity::Pure);
        assert!(analysis.permissions.is_empty());
        assert_eq!(analysis.timeout_ms, 150);
        assert_eq!(analysis.runtime, "universal");
    }

    /// Repeating discovery and analysis on one source yields identical facts.
    #[test]
    fn analysis_is_deterministic() {
        let source = concat!(
            "# Describe the work.\n",
            "def work(text):\n",
            "    if not text:\n",
            "        raise AppError(\"EMPTY_TEXT\", text)\n",
            "    print(text)\n",
            "    return text\n",
        );
        let adapter = PythonAdapter::new();
        let first = adapter.discover(source, "pkg/work.py");
        let second = adapter.discover(source, "pkg/work.py");
        assert_eq!(first, second);
        assert_eq!(
            adapter.analyze(&first[0], source, &[]),
            adapter.analyze(&second[0], source, &[])
        );
    }
}
