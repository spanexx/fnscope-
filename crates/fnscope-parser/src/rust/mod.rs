//! Rust adapter.
//!
//! All Rust grammar knowledge lives in this module. The shared type-name
//! resolver in `crate::resolve` is grammar-free string handling, so it is used
//! by both built-in adapters.

mod body;
mod discover;

use std::collections::BTreeMap;

use fnscope_model::ReturnShape;

use crate::adapter::LanguageAdapter;
use crate::resolve::resolve_fields;
use crate::types::{BodyAnalysis, DiscoveredFn};

/// Adapter for `.rs` sources.
pub struct RustAdapter;

impl LanguageAdapter for RustAdapter {
    fn id(&self) -> &'static str {
        "rust"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
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
    use fnscope_model::Purity;

    /// Representative snippet keeps its pre-refactor discovery facts.
    #[test]
    fn rust_discovery_matches_pre_refactor_facts() {
        let source = r#"
struct Label { id: i32, name: String }
struct AppError { code: String }
impl Service {
  pub fn label(&self, id: i32) -> Result<Label, AppError> {
    Ok(Label { id, name: "x".to_string() })
  }
}
"#;
        let found = RustAdapter.discover(source, "src/lib.rs");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "label");
        assert_eq!(found[0].qualified_name, "Service.label");
        assert_eq!(found[0].kind, FnKind::Method);
        assert_eq!(found[0].returns.type_, "Result<Label, AppError>");
        assert_eq!(found[0].returns.fields.len(), 2);
        assert_eq!(found[0].returns.fields[0].name, "id");
        // `self` is a receiver, not a named parameter.
        assert_eq!(found[0].parameters.len(), 1);
        assert_eq!(found[0].parameters[0].name, "id");
    }

    /// Representative snippet keeps its pre-refactor steps, errors and effects.
    #[test]
    fn rust_analysis_matches_pre_refactor_facts() {
        let source = "fn load(id: i32) -> Result<i32, AppError> {\n  if id < 1 {\n    return Err(AppError::NotFound);\n  }\n  Ok(id)\n}";
        let found = RustAdapter.discover(source, "src/load.rs");
        assert_eq!(found.len(), 1);

        let analysis = RustAdapter.analyze(&found[0], source, &[]);
        assert!(analysis
            .steps
            .iter()
            .any(|step| step.kind == fnscope_model::StepKind::Branch));
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "NotFound");
        assert_eq!(analysis.errors[0].status, Some(500));
        assert_eq!(analysis.purity, Purity::Pure);
        assert_eq!(analysis.timeout_ms, 150);

        let effect = "fn log(message: &str) { println!(\"{}\", message); }";
        let found = RustAdapter.discover(effect, "src/log.rs");
        let effects = RustAdapter.analyze(&found[0], effect, &[]);
        assert_eq!(effects.purity, Purity::SideEffect);
        assert!(effects.permissions.iter().any(|p| p == "console:out"));
    }
}
