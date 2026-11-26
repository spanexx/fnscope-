//! PHP adapter.
//!
//! All PHP grammar knowledge lives in this module. The shared type-name
//! resolver in `crate::resolve` is grammar-free string handling, so it is reused
//! here exactly as the Python adapter reuses it. This job does not register the
//! adapter globally; registration is a separate, reviewed job.

mod body;
mod discover;

use std::collections::BTreeMap;

use fnscope_model::ReturnShape;

use crate::adapter::LanguageAdapter;
use crate::resolve::resolve_fields;
use crate::types::{BodyAnalysis, DiscoveredFn};

/// Whether a PHP binary operator yields a boolean rather than a numeric/string value.
fn is_comparison_operator(operator: &str) -> bool {
    matches!(
        operator,
        "==" | "==="
            | "!="
            | "!=="
            | "<>"
            | "<"
            | ">"
            | "<="
            | ">="
            | "<=>"
            | "&&"
            | "||"
            | "and"
            | "or"
            | "xor"
            | "instanceof"
    )
}

/// Stateless adapter for `.php` sources, held by the registry without global state.
pub struct PhpAdapter;

impl PhpAdapter {
    /// Construct the stateless adapter; all grammar state is local to each call.
    pub fn new() -> Self {
        Self
    }
}

impl Default for PhpAdapter {
    fn default() -> Self {
        Self
    }
}

impl LanguageAdapter for PhpAdapter {
    fn id(&self) -> &'static str {
        "php"
    }

    fn extensions(&self) -> &'static [&'static str] {
        // `Registry::for_extension` trims a leading dot, so this is `php`, not `.php`.
        &["php"]
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

    /// Identity is bare-suffix and the adapter stays out of the global registry.
    #[test]
    fn identity_is_php_and_extension_is_bare() {
        let adapter = PhpAdapter::new();
        assert_eq!(adapter.id(), "php");
        assert_eq!(adapter.extensions().to_vec(), vec!["php"]);
        let _ = PhpAdapter::default();
    }

    /// A free function exposes its bare name, typed parameters, default and docblock.
    #[test]
    fn discovers_free_function_with_parameters_and_defaults() {
        let source = concat!(
            "<?php\n",
            "/** Adds two numbers. */\n",
            "function add(int $a, int $b = 2): int {\n",
            "    return $a + $b;\n",
            "}\n",
        );
        let found = PhpAdapter::new().discover(source, "src/math.php");
        assert_eq!(found.len(), 1);
        let function = &found[0];
        assert_eq!(function.name, "add");
        assert_eq!(function.qualified_name, "add");
        assert_eq!(function.kind, FnKind::Declaration);
        assert_eq!(function.line, 3);
        assert_eq!(function.returns.type_, "int");
        assert_eq!(
            function.doc_description.as_deref(),
            Some("Adds two numbers.")
        );
        assert_eq!(function.parameters.len(), 2);
        assert_eq!(function.parameters[0].name, "a");
        assert_eq!(function.parameters[0].type_, "int");
        assert!(function.parameters[0].required);
        assert_eq!(function.parameters[1].name, "b");
        assert_eq!(function.parameters[1].type_, "int");
        assert!(!function.parameters[1].required);
        assert_eq!(
            function.parameters[1].default_value,
            Some(serde_json::Value::String("2".to_owned()))
        );
    }

    /// A variadic parameter is included as an optional value parameter.
    #[test]
    fn discovers_variadic_parameter() {
        let source = "<?php\nfunction collect(string $prefix = 'x', int ...$rest): array {\n    return $rest;\n}\n";
        let found = PhpAdapter::new().discover(source, "src/collect.php");
        assert_eq!(found.len(), 1);
        let params = &found[0].parameters;
        assert_eq!(params.len(), 2);
        assert_eq!(params[1].name, "rest");
        assert_eq!(params[1].type_, "int");
        assert!(!params[1].required);
    }

    /// Class methods use `Class.method`, set `class_name`, and skip property members.
    #[test]
    fn discovers_class_method_with_qualified_name() {
        let source = concat!(
            "<?php\n",
            "class Calculator {\n",
            "    public int $factor = 2;\n",
            "    public function scale(int $value): int {\n",
            "        return $value * $this->factor;\n",
            "    }\n",
            "}\n",
        );
        let found = PhpAdapter::new().discover(source, "src/calculator.php");
        assert_eq!(found.len(), 1);
        let method = &found[0];
        assert_eq!(method.name, "scale");
        assert_eq!(method.qualified_name, "Calculator.scale");
        assert_eq!(method.kind, FnKind::Method);
        assert_eq!(method.class_name.as_deref(), Some("Calculator"));
        assert_eq!(method.parameters.len(), 1);
        assert_eq!(method.parameters[0].name, "value");
    }

    /// A declaration nested in another body is not a second discoverable function.
    #[test]
    fn ignores_nested_functions() {
        let source = concat!(
            "<?php\n",
            "function outer($x) {\n",
            "    function inner($y) {\n",
            "        return $y;\n",
            "    }\n",
            "    return inner($x);\n",
            "}\n",
        );
        let found = PhpAdapter::new().discover(source, "src/nested.php");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "outer");
    }

    /// Contiguous `//` lines and multi-line `/** */` blocks feed `doc_description`.
    #[test]
    fn uses_leading_line_and_block_comments() {
        let lines = concat!(
            "<?php\n",
            "// First line.\n",
            "// Second line.\n",
            "function documented($x) {\n",
            "    return $x;\n",
            "}\n",
        );
        let found = PhpAdapter::new().discover(lines, "src/doc.php");
        assert_eq!(
            found[0].doc_description.as_deref(),
            Some("First line. Second line.")
        );

        let block = concat!(
            "<?php\n",
            "/**\n",
            " * Do work.\n",
            " * More detail.\n",
            " */\n",
            "function work($x) {\n",
            "    return $x;\n",
            "}\n",
        );
        let found = PhpAdapter::new().discover(block, "src/block.php");
        assert_eq!(
            found[0].doc_description.as_deref(),
            Some("Do work. More detail.")
        );
    }

    /// Class properties and class constants become fields of the class shape.
    #[test]
    fn collects_class_property_and_constant_types() {
        let source = concat!(
            "<?php\n",
            "class Config {\n",
            "    public string $name = 'x';\n",
            "    public const MAX = 10;\n",
            "}\n",
        );
        let types = PhpAdapter::new().collect_types(source);
        let shape = types.get("Config").expect("class must be indexed");
        assert_eq!(shape.type_, "object");
        assert_eq!(shape.fields.len(), 2);
        assert_eq!(shape.fields[0].name, "name");
        assert_eq!(shape.fields[0].type_, "string");
        assert_eq!(shape.fields[1].name, "MAX");
        assert_eq!(shape.fields[1].type_, "10");
    }

    /// Same-file discovery resolves return fields through the class type index.
    #[test]
    fn resolves_return_fields_from_same_file_index() {
        let source = concat!(
            "<?php\n",
            "class Payload {\n",
            "    public string $name = '';\n",
            "}\n",
            "function build(): Payload {\n",
            "    return new Payload();\n",
            "}\n",
        );
        let found = PhpAdapter::new().discover(source, "src/payload.php");
        let build = found.iter().find(|item| item.name == "build").unwrap();
        assert_eq!(build.returns.type_, "Payload");
        assert_eq!(build.returns.fields.len(), 1);
        assert_eq!(build.returns.fields[0].name, "name");
        assert_eq!(build.returns.fields[0].type_, "string");
    }

    /// Cross-file collection feeds the shared resolver for a class in another file.
    #[test]
    fn resolves_return_fields_from_cross_file_index() {
        let types_source = "<?php\nclass Remote {\n    public int $id = 0;\n}\n";
        let function_source = "<?php\nfunction remote(): Remote {\n    return new Remote();\n}\n";
        let adapter = PhpAdapter::new();
        let types = adapter.collect_types(types_source);
        let mut found = adapter.discover(function_source, "src/use.php");
        assert!(found[0].returns.fields.is_empty());
        adapter.resolve_return_fields(&mut found[0], &types);
        assert_eq!(found[0].returns.fields.len(), 1);
        assert_eq!(found[0].returns.fields[0].name, "id");
    }

    /// Assignment-with-call is a Call step; guards are Branch and returns are Return.
    #[test]
    fn analyzes_call_branch_transform_and_return_steps() {
        let source = concat!(
            "<?php\n",
            "function normalize(string $text): string {\n",
            "    // Clean the text.\n",
            "    $clean = trim($text);\n",
            "    if ($clean === '') {\n",
            "        throw new FunctionError(\"EMPTY_TEXT\");\n",
            "    }\n",
            "    $upper = strtoupper($clean);\n",
            "    return $upper;\n",
            "}\n",
        );
        let adapter = PhpAdapter::new();
        let found = adapter.discover(source, "src/normalize.php");
        let analysis = adapter.analyze(&found[0], source, &["trim".to_owned()]);
        let kinds: Vec<StepKind> = analysis.steps.iter().map(|step| step.kind).collect();
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
        assert_eq!(analysis.steps[0].title, "Clean the text.");
        assert_eq!(analysis.steps[1].output, "throws EMPTY_TEXT | continues");
        assert_eq!(analysis.steps[3].output, "string");
        assert!(analysis.callee_names.iter().any(|name| name == "trim"));
        assert_eq!(analysis.steps[0].calls_fn_id.as_deref(), Some("trim"));
    }

    /// A pure arithmetic expression produces a Transform step.
    #[test]
    fn analyzes_pure_expression_as_transform() {
        let source = concat!(
            "<?php\n",
            "function double(int $value): int {\n",
            "    $result = $value * 2;\n",
            "    return $result;\n",
            "}\n",
        );
        let adapter = PhpAdapter::new();
        let found = adapter.discover(source, "src/double.php");
        let analysis = adapter.analyze(&found[0], source, &[]);
        assert_eq!(analysis.steps[0].kind, StepKind::Transform);
        assert_eq!(analysis.steps[0].output, "$result: int");
        assert_eq!(analysis.steps[0].inputs, vec!["value".to_owned()]);
        assert_eq!(analysis.steps[1].kind, StepKind::Return);
    }

    /// Operator expressions infer string, float and bool instead of `unknown`.
    #[test]
    fn infers_operator_expression_types() {
        let source = concat!(
            "<?php\n",
            "function mix(string $name, float $ratio): string {\n",
            "    $joined = \"x\" . $name;\n",
            "    $scaled = 1.5 + 2;\n",
            "    $ok = $ratio > 1;\n",
            "    return $joined;\n",
            "}\n",
        );
        let adapter = PhpAdapter::new();
        let found = adapter.discover(source, "src/mix.php");
        let analysis = adapter.analyze(&found[0], source, &[]);
        assert_eq!(analysis.steps[0].output, "$joined: string");
        assert_eq!(analysis.steps[1].output, "$scaled: float");
        assert_eq!(analysis.steps[2].output, "$ok: bool");
    }

    /// A guarded `throw new FunctionError("CODE")` becomes one deduplicated ErrorSpec.
    #[test]
    fn extracts_throw_error_with_guard_condition() {
        let source = concat!(
            "<?php\n",
            "function check(string $text): string {\n",
            "    if ($text === '') {\n",
            "        throw new FunctionError(\"EMPTY_TEXT\");\n",
            "    }\n",
            "    throw new FunctionError(\"EMPTY_TEXT\");\n",
            "    return $text;\n",
            "}\n",
        );
        let adapter = PhpAdapter::new();
        let found = adapter.discover(source, "src/check.php");
        let analysis = adapter.analyze(&found[0], source, &[]);
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "EMPTY_TEXT");
        assert_eq!(analysis.errors[0].status, Some(400));
        assert_eq!(analysis.errors[0].when, "$text === ''");
    }

    /// `echo` marks console output; file, network and env markers add capabilities.
    #[test]
    fn echo_and_environment_markers_are_side_effects() {
        let echo_source = concat!(
            "<?php\n",
            "function report(string $text): void {\n",
            "    echo \"report: \" . $text;\n",
            "}\n",
        );
        let adapter = PhpAdapter::new();
        let found = adapter.discover(echo_source, "src/report.php");
        let analysis = adapter.analyze(&found[0], echo_source, &[]);
        assert_eq!(analysis.purity, Purity::SideEffect);
        assert!(analysis
            .permissions
            .iter()
            .any(|permission| permission == "console:out"));
        assert_eq!(analysis.timeout_ms, 1000);
        assert_eq!(analysis.runtime, "node20");
        assert_eq!(analysis.steps[0].kind, StepKind::Call);

        let io_source = concat!(
            "<?php\n",
            "function fetch(string $url): string {\n",
            "    $key = getenv(\"API_KEY\");\n",
            "    $body = file_get_contents(\"https://example.test\");\n",
            "    file_put_contents(\"out.txt\", $body);\n",
            "    return $body;\n",
            "}\n",
        );
        let found = adapter.discover(io_source, "src/fetch.php");
        let analysis = adapter.analyze(&found[0], io_source, &[]);
        for expected in [
            "fs:read",
            "fs:write",
            "net:outbound",
            "secrets:read:API_KEY",
        ] {
            assert!(
                analysis
                    .permissions
                    .iter()
                    .any(|permission| permission == expected),
                "missing permission {expected}"
            );
        }
        assert_eq!(analysis.timeout_ms, 5000);
    }

    /// Pure code keeps the base budget and reports no permissions.
    #[test]
    fn pure_function_keeps_base_budget() {
        let source = "<?php\nfunction add(int $a, int $b): int {\n    return $a + $b;\n}\n";
        let adapter = PhpAdapter::new();
        let found = adapter.discover(source, "src/add.php");
        let analysis = adapter.analyze(&found[0], source, &[]);
        assert_eq!(analysis.purity, Purity::Pure);
        assert!(analysis.permissions.is_empty());
        assert_eq!(analysis.timeout_ms, 150);
        assert_eq!(analysis.runtime, "universal");
    }

    /// Repeating discovery and analysis on one source yields identical facts.
    #[test]
    fn analysis_is_deterministic() {
        let source = concat!(
            "<?php\n",
            "// Describe the work.\n",
            "function work(string $text): string {\n",
            "    if ($text === '') {\n",
            "        throw new AppError(\"EMPTY_TEXT\", $text);\n",
            "    }\n",
            "    echo $text;\n",
            "    return $text;\n",
            "}\n",
        );
        let adapter = PhpAdapter::new();
        let first = adapter.discover(source, "src/work.php");
        let second = adapter.discover(source, "src/work.php");
        assert_eq!(first, second);
        assert_eq!(
            adapter.analyze(&first[0], source, &[]),
            adapter.analyze(&second[0], source, &[])
        );
    }
}
