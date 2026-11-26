//! Java language adapter.
//!
//! All Java grammar knowledge stays inside this module and its two children.
//! A separate registration job adds the `pub mod` line and the registry entry,
//! so this adapter never edits core or global state.

mod body;
mod discover;

use std::collections::BTreeMap;

use fnscope_model::ReturnShape;

use crate::adapter::LanguageAdapter;
use crate::resolve::resolve_fields;
use crate::types::{BodyAnalysis, DiscoveredFn};

/// Stateless adapter for `.java` sources, registered by a separate job.
pub struct JavaAdapter;

impl JavaAdapter {
    /// Construct the adapter so callers can register it without global state.
    pub fn new() -> Self {
        Self
    }
}

impl Default for JavaAdapter {
    /// Return the same stateless value as [`JavaAdapter::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageAdapter for JavaAdapter {
    fn id(&self) -> &'static str {
        "java"
    }

    // The trait stores suffixes without a leading dot and the registry trims a
    // caller's dot, so both "java" and ".java" select this adapter.
    fn extensions(&self) -> &'static [&'static str] {
        &["java"]
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

    /// The adapter's identity matches the registry contract before registration.
    #[test]
    fn java_adapter_reports_contract_identity() {
        let adapter = JavaAdapter::new();
        assert_eq!(adapter.id(), "java");
        assert_eq!(adapter.extensions(), &["java"]);
        assert_eq!(JavaAdapter::default().id(), "java");
        assert!(adapter.discover("class Empty {}", "Empty.java").is_empty());
    }

    /// Return-field resolution delegates to the shared grammar-free resolver.
    #[test]
    fn java_adapter_resolves_return_fields_from_a_type_index() {
        // The type lives in another file, so discovery leaves fields empty and
        // the project-wide index supplied later must fill them.
        let source = "class Repo { Item load() { return null; } }";
        let types_source = "class Item { String id; }";
        let found = JavaAdapter::new().discover(source, "Repo.java");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].returns.type_, "Item");
        assert!(found[0].returns.fields.is_empty());

        let types = JavaAdapter::new().collect_types(types_source);
        let mut resolved = found[0].clone();
        JavaAdapter::new().resolve_return_fields(&mut resolved, &types);
        assert_eq!(resolved.returns.fields.len(), 1);
        assert_eq!(resolved.returns.fields[0].name, "id");
        assert_eq!(resolved.returns.fields[0].type_, "String");
    }

    /// The adapter's six trait methods are callable without registration.
    #[test]
    fn java_adapter_runs_the_full_trait_surface() {
        let adapter = JavaAdapter::new();
        let source = r#"
class Main {
    // Build a result.
    String build(String input) { return input; }
}
"#;
        let known = vec!["build".to_owned()];
        let found = adapter.discover(source, "Main.java");
        assert_eq!(found.len(), 1);
        let analysis = adapter.analyze(&found[0], source, &known);
        assert_eq!(analysis.steps.len(), 1);
        assert_eq!(analysis.steps[0].kind, fnscope_model::StepKind::Return);
        assert!(adapter.collect_types(source).is_empty());
    }
}
