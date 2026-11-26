//! Backwards-compatible body-analysis entry point.
//!
//! The AST walker now lives in the `typescript` and `rust` adapter modules.
//! This free function keeps older callers and tests compiling by looking the
//! adapter up in the built-in registry.

use crate::adapter::registry;
use crate::discover::lang_extension;
use crate::types::{BodyAnalysis, DiscoveredFn, Lang};

#[cfg(test)]
use fnscope_model::{Purity, StepKind};

/// Analyze one discovered function's body into ordered, deterministic facts.
///
/// When no function node matches the discovery record, the neutral
/// [`BodyAnalysis::default()`] is returned so a partially discovered file
/// cannot corrupt a manifest.
pub fn analyze(
    found: &DiscoveredFn,
    lang: Lang,
    source: &str,
    known_names: &[String],
) -> BodyAnalysis {
    let registry = registry();
    let Some(adapter) = registry.for_extension(lang_extension(lang)) else {
        return BodyAnalysis::default();
    };
    adapter.analyze(found, source, known_names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FnKind;
    use fnscope_model::ReturnShape;

    /// Builds a discovery record whose source slice is the whole test snippet.
    fn discovered(name: &str, source: &str, returns: &str) -> DiscoveredFn {
        DiscoveredFn {
            name: name.to_owned(),
            qualified_name: name.to_owned(),
            kind: FnKind::Declaration,
            line: 1,
            end_line: source.lines().count(),
            source_code: source.to_owned(),
            parameters: Vec::new(),
            returns: ReturnShape {
                type_: returns.to_owned(),
                fields: Vec::new(),
            },
            doc_description: None,
            tags: Vec::new(),
            is_async: false,
            class_name: None,
        }
    }

    #[test]
    fn call_step_resolves_known_callee_and_inputs() {
        let source = "function evaluateGrammarRole(input: Input): Token[] {\n  const tokens: Token[] = tokenize(input.sentence);\n  return tokens;\n}";
        let found = discovered("evaluateGrammarRole", source, "Token[]");
        let known = vec!["tokenize".to_owned()];

        let analysis = analyze(&found, Lang::TypeScript, source, &known);
        let step = &analysis.steps[0];
        assert_eq!(step.id, "step-1");
        assert_eq!(step.kind, StepKind::Call);
        assert_eq!(step.calls_fn_id.as_deref(), Some("tokenize"));
        assert!(step.inputs.iter().any(|name| name == "input.sentence"));
        assert_eq!(step.output, "tokens: Token[]");
        assert_eq!(step.title, "Call tokenize()");
        assert!(!step.code.contains('\n'));
    }

    #[test]
    fn guard_step_reports_thrown_code_and_status() {
        let source = "function check(sentence: string): void {\n  const match = /^[a-z]+$/.test(sentence);\n  if (!match) {\n    throw new FunctionError('TOKEN_NOT_FOUND');\n  }\n  return;\n}";
        let found = discovered("check", source, "void");

        let analysis = analyze(&found, Lang::TypeScript, source, &[]);
        let guard = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Branch)
            .expect("guard step");
        assert!(guard.output.contains("throws TOKEN_NOT_FOUND"));
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "TOKEN_NOT_FOUND");
        assert_eq!(analysis.errors[0].status, Some(422));
        assert!(analysis.errors[0].when.contains("!match"));
    }

    #[test]
    fn return_step_uses_declared_return_type() {
        let source = "function role(): Role {\n  return { roleId, label };\n}";
        let found = discovered("role", source, "Role");

        let analysis = analyze(&found, Lang::TypeScript, source, &[]);
        let step = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Return)
            .expect("return step");
        assert_eq!(step.output, found.returns.type_);
    }

    #[test]
    fn annotated_comments_become_title_and_detail() {
        let source = "function evaluate(input: Input): Token[] {\n  // 1. Tokenize sentence\n  // Splits into tokens\n  const tokens: Token[] = tokenize(input.sentence);\n  return tokens;\n}";
        let found = discovered("evaluate", source, "Token[]");

        let analysis = analyze(&found, Lang::TypeScript, source, &[]);
        assert_eq!(analysis.steps[0].title, "Tokenize sentence");
        assert_eq!(analysis.steps[0].detail, "Splits into tokens");
    }

    #[test]
    fn effects_yield_permissions_and_budgets() {
        let source = "async function load(url: string) {\n  const key = process.env.API_KEY;\n  const res = await fetch(url);\n  return res;\n}";
        let found = discovered("load", source, "Response");

        let analysis = analyze(&found, Lang::TypeScript, source, &[]);
        assert_eq!(analysis.purity, Purity::SideEffect);
        assert!(analysis.permissions.iter().any(|p| p == "net:outbound"));
        assert!(analysis
            .permissions
            .iter()
            .any(|p| p == "secrets:read:API_KEY"));
        assert_eq!(analysis.timeout_ms, 5000);
        assert_eq!(analysis.runtime, "node20");
    }

    #[test]
    fn arithmetic_declaration_is_a_pure_transform() {
        let source =
            "function double(n: number): number {\n  const doubled = n * 2;\n  return doubled;\n}";
        let found = discovered("double", source, "number");

        let analysis = analyze(&found, Lang::TypeScript, source, &[]);
        assert_eq!(analysis.steps[0].kind, StepKind::Transform);
        assert!(analysis.steps[0].inputs.iter().any(|name| name == "n"));
        assert_eq!(analysis.steps[0].output, "doubled: number");
        assert_eq!(analysis.purity, Purity::Pure);
        assert_eq!(analysis.timeout_ms, 150);
        assert_eq!(analysis.runtime, "universal");
    }

    #[test]
    fn repeated_analysis_is_identical() {
        let source = "function evaluate(input: Input): Token[] {\n  const tokens: Token[] = tokenize(input.sentence);\n  if (!tokens) {\n    throw new FunctionError('EMPTY_SENTENCE');\n  }\n  return tokens;\n}";
        let found = discovered("evaluate", source, "Token[]");
        let known = vec!["tokenize".to_owned()];

        let first = analyze(&found, Lang::TypeScript, source, &known);
        let second = analyze(&found, Lang::TypeScript, source, &known);
        assert_eq!(first, second);
    }

    #[test]
    fn rust_let_and_call_steps() {
        let source =
            "fn evaluate(input: i32) -> i32 {\n  let doubled: i32 = scale(input);\n  doubled\n}";
        let found = discovered("evaluate", source, "i32");
        let known = vec!["scale".to_owned()];

        let analysis = analyze(&found, Lang::Rust, source, &known);
        assert_eq!(analysis.steps.len(), 2);
        let call = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Call)
            .expect("call step");
        assert_eq!(call.id, "step-1");
        assert_eq!(call.calls_fn_id.as_deref(), Some("scale"));
        assert!(call.inputs.iter().any(|name| name == "input"));
        assert_eq!(call.output, "doubled: i32");
        assert_eq!(call.code, "let doubled: i32 = scale(input);");
        assert!(!call.code.contains('\n'));
    }

    #[test]
    fn rust_guard_with_return_err_reports_error() {
        let source = "fn load(id: i32) -> Result<i32, AppError> {\n  if id < 1 {\n    return Err(AppError::NotFound);\n  }\n  Ok(id)\n}";
        let found = discovered("load", source, "Result<i32, AppError>");

        let analysis = analyze(&found, Lang::Rust, source, &[]);
        let guard = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Branch)
            .expect("guard step");
        assert!(guard.output.contains("throws NotFound"));
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "NotFound");
        assert_eq!(analysis.errors[0].status, Some(500));
        assert!(analysis.errors[0].when.contains("id < 1"));
    }

    #[test]
    fn rust_println_is_side_effect_with_console_permission() {
        let source = "fn greet(name: &str) {\n  println!(\"hi {}\", name);\n}";
        let found = discovered("greet", source, "()");

        let analysis = analyze(&found, Lang::Rust, source, &[]);
        assert_eq!(analysis.purity, Purity::SideEffect);
        assert!(analysis.permissions.iter().any(|p| p == "console:out"));
        assert_eq!(analysis.timeout_ms, 1000);
        assert_eq!(analysis.runtime, "node20");
    }

    #[test]
    fn rust_method_call_resolves_by_last_segment() {
        let source = "impl Service { fn run(&self, x: i32) -> i32 { self.add(x) } }";
        let found = discovered("run", source, "i32");
        let known = vec!["add".to_owned()];

        let analysis = analyze(&found, Lang::Rust, source, &known);
        let call = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Call)
            .expect("call step");
        assert_eq!(call.calls_fn_id.as_deref(), Some("add"));
        assert!(call.inputs.iter().any(|name| name == "x"));
        assert_eq!(call.title, "Call add()");
    }

    #[test]
    fn rust_doc_comments_become_step_title_and_detail() {
        let source = "fn evaluate(x: i32) -> i32 {\n  /// 1. Double value\n  /// Keeps it pure\n  let doubled = x * 2;\n  doubled\n}";
        let found = discovered("evaluate", source, "i32");

        let analysis = analyze(&found, Lang::Rust, source, &[]);
        let transform = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Transform)
            .expect("transform step");
        assert_eq!(transform.title, "Double value");
        assert_eq!(transform.detail, "Keeps it pure");
        assert_eq!(transform.output, "doubled: number");
    }

    #[test]
    fn rust_repeated_analysis_is_identical() {
        let source = "fn load(id: i32) -> Result<i32, AppError> {\n  if id < 1 {\n    return Err(\"BAD_REQUEST\");\n  }\n  Ok(id)\n}";
        let found = discovered("load", source, "Result<i32, AppError>");

        let first = analyze(&found, Lang::Rust, source, &[]);
        let second = analyze(&found, Lang::Rust, source, &[]);
        assert_eq!(first, second);
        assert_eq!(first.errors[0].code, "BAD_REQUEST");
        assert_eq!(first.errors[0].status, Some(400));
    }

    #[test]
    fn rust_struct_literal_error_code_feeds_guard_output() {
        let source = r#"pub fn normalize(text: &str) -> Result<String, AppError> {
  if text.is_empty() {
    return Err(AppError { code: "EMPTY_TEXT" });
  }
  Ok(text.to_string())
}"#;
        let found = discovered("normalize", source, "Result<String, AppError>");

        let analysis = analyze(&found, Lang::Rust, source, &[]);
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "EMPTY_TEXT");
        let guard = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Branch)
            .expect("guard step");
        assert!(
            guard.output.contains("throws EMPTY_TEXT"),
            "unexpected guard output: {}",
            guard.output
        );
    }

    #[test]
    fn rust_const_error_code_from_struct_literal() {
        let source = "const EMPTY_TEXT: &str = \"EMPTY_TEXT\";\nfn check(text: &str) -> Result<(), AppError> {\n  if text.is_empty() {\n    return Err(AppError { code: EMPTY_TEXT });\n  }\n  Ok(())\n}";
        let fn_start = source.find("fn check").expect("function start");
        let found = DiscoveredFn {
            name: "check".to_owned(),
            qualified_name: "check".to_owned(),
            kind: FnKind::Declaration,
            line: 2,
            end_line: source.lines().count(),
            source_code: source[fn_start..].to_owned(),
            parameters: Vec::new(),
            returns: ReturnShape {
                type_: "Result<(), AppError>".to_owned(),
                fields: Vec::new(),
            },
            doc_description: None,
            tags: Vec::new(),
            is_async: false,
            class_name: None,
        };

        let analysis = analyze(&found, Lang::Rust, source, &[]);
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "EMPTY_TEXT");
    }

    #[test]
    fn rust_chained_call_title_uses_resolved_callee() {
        let source = "fn calculate(x: i32) -> i32 { add(x).abs() }";
        let found = discovered("calculate", source, "i32");
        let known = vec!["add".to_owned()];

        let analysis = analyze(&found, Lang::Rust, source, &known);
        let call = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Call)
            .expect("call step");
        assert_eq!(call.calls_fn_id.as_deref(), Some("add"));
        assert_eq!(call.title, "Call add()");
    }
}
