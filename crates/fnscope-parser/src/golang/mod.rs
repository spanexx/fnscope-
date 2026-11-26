//! Go language adapter.
//!
//! The directory is named `golang` because `go` is a Rust keyword and cannot be
//! used as a module identifier. All Go grammar knowledge stays inside this module
//! and its two children; a separate registration job adds the `pub mod` line and
//! the registry entry, so this adapter never edits core or global state.

mod body;
mod discover;

use std::collections::BTreeMap;

use fnscope_model::ReturnShape;

use crate::adapter::LanguageAdapter;
use crate::resolve::resolve_fields;
use crate::types::{BodyAnalysis, DiscoveredFn};

/// Stateless adapter for `.go` sources, registered as a built-in adapter.
pub struct GoAdapter;

impl GoAdapter {
    /// Construct the adapter so callers can register it without global state.
    pub fn new() -> Self {
        Self
    }
}

impl Default for GoAdapter {
    /// Return the same stateless value as [`GoAdapter::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageAdapter for GoAdapter {
    fn id(&self) -> &'static str {
        "go"
    }

    // The trait stores suffixes without a leading dot and the registry trims a
    // caller's dot, so both "go" and ".go" select this adapter.
    fn extensions(&self) -> &'static [&'static str] {
        &["go"]
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
    fn go_adapter_reports_contract_identity() {
        let adapter = GoAdapter::new();
        assert_eq!(adapter.id(), "go");
        assert_eq!(adapter.extensions(), &["go"]);
        assert_eq!(GoAdapter::default().id(), "go");
        assert!(adapter.discover("package p\n", "p.go").is_empty());
    }

    /// Return-field resolution delegates to the shared grammar-free resolver.
    #[test]
    fn go_adapter_resolves_return_fields_from_a_type_index() {
        // The type lives in another file, so discovery leaves fields empty and
        // the project-wide index supplied later must fill them.
        let source = "package p\nfunc build() Item { return Item{} }";
        let types_source = "package p\ntype Item struct { id int }";
        let found = GoAdapter.discover(source, "build.go");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].returns.type_, "Item");
        assert!(found[0].returns.fields.is_empty());

        let types = GoAdapter.collect_types(types_source);
        let mut resolved = found[0].clone();
        GoAdapter.resolve_return_fields(&mut resolved, &types);
        assert_eq!(resolved.returns.fields.len(), 1);
        assert_eq!(resolved.returns.fields[0].name, "id");
        assert_eq!(resolved.returns.fields[0].type_, "int");
    }

    /// Same-file struct fields are resolved during discovery, before core runs.
    #[test]
    fn go_discovery_resolves_same_file_return_fields() {
        let source = "package p\ntype Item struct { id int }\nfunc build() Item { return Item{} }";
        let found = GoAdapter.discover(source, "items.go");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].returns.fields.len(), 1);
        assert_eq!(found[0].returns.fields[0].name, "id");
    }
}
