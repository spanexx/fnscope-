//! Backwards-compatible discovery entry points.
//!
//! The grammar itself now lives in the `typescript` and `rust` adapter modules.
//! These free functions keep older callers and tests compiling by looking the
//! adapter up in the built-in registry.

use std::collections::BTreeMap;

use fnscope_model::ReturnShape;

use crate::adapter::registry;
use crate::types::{DiscoveredFn, Lang};

#[cfg(test)]
use crate::types::FnKind;

/// Extension claimed by one `Lang`, used only to select a built-in adapter.
pub(crate) fn lang_extension(lang: Lang) -> &'static str {
    match lang {
        Lang::TypeScript => "ts",
        Lang::Tsx => "tsx",
        Lang::JavaScript => "js",
        Lang::Jsx => "jsx",
        Lang::Rust => "rs",
    }
}

/// Discover every callable in one source file, sorted by `(line, qualified_name)`.
///
/// `relative_path` is intentionally not used to invent a module path or description; the
/// discovery pass reports only facts it can read from the AST.
pub fn discover(lang: Lang, source: &str, relative_path: &str) -> Vec<DiscoveredFn> {
    let registry = registry();
    let Some(adapter) = registry.for_extension(lang_extension(lang)) else {
        return Vec::new();
    };
    adapter.discover(source, relative_path)
}

/// Collect object-shaped type aliases and interfaces from one source file.
///
/// The returned map is keyed by type name and can be shared across files to resolve
/// return fields for types declared elsewhere with [`resolve_return_fields`].
pub fn collect_types(lang: Lang, source: &str) -> BTreeMap<String, ReturnShape> {
    let registry = registry();
    let Some(adapter) = registry.for_extension(lang_extension(lang)) else {
        return BTreeMap::new();
    };
    adapter.collect_types(source)
}

/// Fill empty return fields by resolving a declared type name through a project-wide type index.
///
/// The declared `returns.type_` is left untouched; only an empty `fields` list is replaced.
/// Return-field lookup is language-neutral, so any built-in adapter yields the same result.
pub fn resolve_return_fields(found: &mut DiscoveredFn, types: &BTreeMap<String, ReturnShape>) {
    let registry = registry();
    let Some(adapter) = registry.for_extension("ts") else {
        return;
    };
    adapter.resolve_return_fields(found, types);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(lang: Lang, source: &str) -> DiscoveredFn {
        let found = discover(lang, source, "src/example.ts");
        assert_eq!(
            found.len(),
            1,
            "expected exactly one function in: {}",
            source
        );
        found.into_iter().next().unwrap()
    }

    fn assert_num(actual: &Option<serde_json::Value>, expected: i64) {
        assert_eq!(actual, &Some(serde_json::Value::from(expected)));
    }

    #[test]
    fn declaration_with_required_and_optional_params_and_return_type() {
        let f = one(
            Lang::TypeScript,
            "export function foo(a: string, b?: number): boolean { return true; }",
        );
        assert_eq!(f.name, "foo");
        assert_eq!(f.qualified_name, "foo");
        assert_eq!(f.kind, FnKind::Declaration);
        assert_eq!(f.line, 1);
        assert_eq!(f.returns.type_, "boolean");

        assert_eq!(f.parameters.len(), 2);
        assert_eq!(f.parameters[0].name, "a");
        assert!(f.parameters[0].required);
        assert_eq!(f.parameters[0].type_, "string");
        assert_eq!(f.parameters[1].name, "b");
        assert!(!f.parameters[1].required);
        assert_eq!(f.parameters[1].type_, "number");
    }

    #[test]
    fn object_typed_parameter_with_optional_property_is_still_required() {
        let source = r#"
export async function evaluateGrammarRole(input: {
  sentence: string;
  targetWord: string;
  includeTeachingLayer?: boolean;
}): Promise<RoleEvaluationResult> { return Promise.resolve({}); }
"#;
        let f = one(Lang::TypeScript, source);
        assert_eq!(f.parameters.len(), 1);
        assert_eq!(f.parameters[0].name, "input");
        assert!(f.parameters[0].required);
        assert!(f.is_async);

        let optional = one(Lang::TypeScript, "export function f(b?: number) {}");
        assert!(!optional.parameters[0].required);
    }

    #[test]
    fn arrow_with_default_value() {
        let f = one(
            Lang::TypeScript,
            "export const add = (a: number, b: number = 2) => a + b;",
        );
        assert_eq!(f.name, "add");
        assert_eq!(f.kind, FnKind::Arrow);
        assert_eq!(f.parameters.len(), 2);
        assert_eq!(f.parameters[1].name, "b");
        assert!(!f.parameters[1].required);
        assert_num(&f.parameters[1].default_value, 2);
    }

    #[test]
    fn class_method_records_qualified_name_and_class() {
        let f = one(
            Lang::TypeScript,
            "class Service { public run(x: string) { return x; } }",
        );
        assert_eq!(f.name, "run");
        assert_eq!(f.qualified_name, "Service.run");
        assert_eq!(f.class_name.as_deref(), Some("Service"));
        assert_eq!(f.kind, FnKind::Method);
        assert_eq!(f.parameters[0].name, "x");
        assert_eq!(f.parameters[0].type_, "string");
    }

    #[test]
    fn function_expression_initializer_is_arrow() {
        let f = one(
            Lang::TypeScript,
            "export const make = function (a: number) { return a; };",
        );
        assert_eq!(f.name, "make");
        assert_eq!(f.kind, FnKind::Arrow);
    }

    #[test]
    fn class_field_arrow_function_is_method() {
        let f = one(
            Lang::TypeScript,
            "class Handler { handle = (x: string) => x; }",
        );
        assert_eq!(f.name, "handle");
        assert_eq!(f.qualified_name, "Handler.handle");
        assert_eq!(f.class_name.as_deref(), Some("Handler"));
        assert_eq!(f.kind, FnKind::Method);
    }

    #[test]
    fn jsdoc_description_params_and_tags_are_captured() {
        let source = r#"/**
 * Adds two numbers.
 * @param a the first number
 * @returns the sum
 * @tags nlp, pure
 */
export function add(a: number, b: number): number { return a + b; }
"#;
        let f = one(Lang::TypeScript, source);
        assert_eq!(f.doc_description.as_deref(), Some("Adds two numbers."));
        assert_eq!(f.tags, vec!["nlp".to_string(), "pure".to_string()]);
        assert_eq!(f.parameters[0].description, "the first number");
        assert!(f.parameters[1].description.is_empty());
    }

    #[test]
    fn promise_return_resolves_local_type_alias_fields() {
        let source = r#"
type Role = { id: string; label: string };
function loadRole(): Promise<Role> { return Promise.resolve({ id: "1", label: "admin" }); }
"#;
        let found = discover(Lang::TypeScript, source, "src/roles.ts");
        let f = found.iter().find(|f| f.name == "loadRole").unwrap();
        assert_eq!(f.returns.type_, "Promise<Role>");
        assert_eq!(f.returns.fields.len(), 2);
        assert_eq!(f.returns.fields[0].name, "id");
        assert_eq!(f.returns.fields[0].type_, "string");
        assert_eq!(f.returns.fields[1].name, "label");
        assert_eq!(f.returns.fields[1].type_, "string");
    }

    #[test]
    fn collect_types_finds_type_alias_fields() {
        let types = collect_types(
            Lang::TypeScript,
            "type Role = { roleId: string; label: string };",
        );
        let shape = types.get("Role").unwrap();
        assert_eq!(shape.fields.len(), 2);
        assert_eq!(shape.fields[0].name, "roleId");
        assert_eq!(shape.fields[0].type_, "string");
        assert_eq!(shape.fields[1].name, "label");
        assert_eq!(shape.fields[1].type_, "string");
    }

    #[test]
    fn resolve_return_fields_fills_promise_type_and_preserves_type_text() {
        let types = collect_types(
            Lang::TypeScript,
            "type Role = { roleId: string; label: string };",
        );
        let mut found = one(
            Lang::TypeScript,
            "export function load(): Promise<Role> { return Promise.resolve({}); }",
        );
        assert!(found.returns.fields.is_empty());
        resolve_return_fields(&mut found, &types);
        assert_eq!(found.returns.type_, "Promise<Role>");
        assert_eq!(found.returns.fields.len(), 2);
        assert_eq!(found.returns.fields[0].name, "roleId");
        assert_eq!(found.returns.fields[1].name, "label");
    }

    #[test]
    fn resolve_return_fields_keeps_existing_fields() {
        let types = collect_types(
            Lang::TypeScript,
            "type Role = { roleId: string; label: string };",
        );
        let source = r#"
type Local = { keep: number };
export function load(): Promise<Local> { return Promise.resolve({}); }
"#;
        let mut found = one(Lang::TypeScript, source);
        assert_eq!(found.returns.fields.len(), 1);
        assert_eq!(found.returns.fields[0].name, "keep");
        resolve_return_fields(&mut found, &types);
        assert_eq!(found.returns.fields.len(), 1);
        assert_eq!(found.returns.fields[0].name, "keep");
    }

    #[test]
    fn nested_functions_are_ignored_and_async_is_flagged() {
        let source = r#"
export async function outer(x: number) {
  function inner() { return x; }
  return inner();
}
"#;
        let found = discover(Lang::TypeScript, source, "src/a.ts");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "outer");
        assert!(found[0].is_async);
    }

    #[test]
    fn tsx_arrow_component_parses_without_panicking() {
        let found = discover(
            Lang::Tsx,
            "export const App = (name: string) => <div>Hello {name}</div>;",
            "src/App.tsx",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "App");
        assert_eq!(found[0].kind, FnKind::Arrow);
    }

    #[test]
    fn deterministic_order_and_line_scoped_duplicate_suppression() {
        let source = "function b() {}\nfunction a() {}\nfunction b() {}\n";
        let found = discover(Lang::JavaScript, source, "src/a.js");
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        // Dedup is keyed by (qualified_name, line), so same-name declarations on different lines are distinct.
        assert_eq!(names, vec!["b", "a", "b"]);
    }

    #[test]
    fn export_wrapper_does_not_double_count() {
        let found = discover(Lang::TypeScript, "export function once() {}", "src/a.ts");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "once");
    }

    #[test]
    fn rust_free_function_records_signature() {
        let f = one(Lang::Rust, "pub fn add(a: i32, b: i32) -> i32 { a + b }");
        assert_eq!(f.name, "add");
        assert_eq!(f.qualified_name, "add");
        assert_eq!(f.kind, FnKind::Declaration);
        assert_eq!(f.line, 1);
        assert_eq!(f.end_line, 1);
        assert_eq!(f.returns.type_, "i32");
        assert_eq!(f.parameters.len(), 2);
        assert_eq!(f.parameters[0].name, "a");
        assert_eq!(f.parameters[0].type_, "i32");
        assert!(f.parameters[0].required);
        assert!(f.source_code.starts_with("pub fn add"));
    }

    #[test]
    fn rust_impl_method_records_qualified_name_and_skips_self() {
        let f = one(
            Lang::Rust,
            "impl Service { pub fn add(&self, x: i32) -> i32 { x } }",
        );
        assert_eq!(f.name, "add");
        assert_eq!(f.qualified_name, "Service.add");
        assert_eq!(f.class_name.as_deref(), Some("Service"));
        assert_eq!(f.kind, FnKind::Method);
        assert_eq!(f.parameters.len(), 1);
        assert_eq!(f.parameters[0].name, "x");
        assert_eq!(f.parameters[0].type_, "i32");
    }

    #[test]
    fn rust_result_return_resolves_struct_fields() {
        let source = r#"
struct Label { id: i32, name: String }
struct AppError { code: String }
pub fn label(id: i32) -> Result<Label, AppError> { Ok(Label { id, name: "x".to_string() }) }
"#;
        let f = one(Lang::Rust, source);
        assert_eq!(f.returns.type_, "Result<Label, AppError>");
        assert_eq!(f.returns.fields.len(), 2);
        assert_eq!(f.returns.fields[0].name, "id");
        assert_eq!(f.returns.fields[0].type_, "i32");
        assert_eq!(f.returns.fields[1].name, "name");
        assert_eq!(f.returns.fields[1].type_, "String");
    }

    #[test]
    fn rust_doc_comment_captures_description_and_tags() {
        let source = r#"/// Adds two numbers.
/// @tags math, pure
pub fn add(a: i32, b: i32) -> i32 { a + b }
"#;
        let f = one(Lang::Rust, source);
        assert_eq!(f.doc_description.as_deref(), Some("Adds two numbers."));
        assert_eq!(f.tags, vec!["math".to_string(), "pure".to_string()]);
        assert_eq!(f.line, 3);
    }

    #[test]
    fn rust_collect_types_finds_struct_and_enum_fields() {
        let source = r#"
struct Label { id: i32, name: String }
enum AppError { NotFound, Invalid { code: String } }
"#;
        let types = collect_types(Lang::Rust, source);
        let label = types.get("Label").unwrap();
        assert_eq!(label.fields.len(), 2);
        assert_eq!(label.fields[1].name, "name");
        let error = types.get("AppError").unwrap();
        assert_eq!(error.fields.len(), 2);
        assert_eq!(error.fields[0].name, "NotFound");
        assert_eq!(error.fields[1].name, "Invalid");
    }

    #[test]
    fn rust_nested_function_is_ignored() {
        let source = r#"
pub fn outer(x: i32) -> i32 {
    fn inner(y: i32) -> i32 { y }
    inner(x)
}
"#;
        let found = discover(Lang::Rust, source, "src/lib.rs");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "outer");
    }

    #[test]
    fn rust_trait_signature_is_discovered() {
        let f = one(
            Lang::Rust,
            "trait Greet { fn greet(&self, name: &str) -> String; }",
        );
        assert_eq!(f.name, "greet");
        assert_eq!(f.kind, FnKind::Declaration);
        assert_eq!(f.returns.type_, "String");
        assert_eq!(f.parameters.len(), 1);
        assert_eq!(f.parameters[0].type_, "str");
    }
}
