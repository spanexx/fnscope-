//! C++ language adapter.
//!
//! The directory is `cpp` because `cpp` is a valid Rust identifier. All C++
//! grammar knowledge stays inside this module and its two children; registering
//! the adapter is a separate job, so this adapter never edits core or global
//! state. The grammar is `tree_sitter_cpp::LANGUAGE` from tree-sitter-cpp 0.23.4.

mod body;
mod discover;

use std::collections::BTreeMap;

use fnscope_model::ReturnShape;

use crate::adapter::LanguageAdapter;
use crate::resolve::resolve_fields;
use crate::types::{BodyAnalysis, DiscoveredFn};

/// Stateless adapter for `.cpp` and `.cc` sources, registered by the integrator.
pub struct CppAdapter;

impl CppAdapter {
    /// Construct the adapter so callers can register it without global state.
    pub fn new() -> Self {
        Self
    }
}

impl Default for CppAdapter {
    /// Return the same stateless value as [`CppAdapter::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageAdapter for CppAdapter {
    fn id(&self) -> &'static str {
        "cpp"
    }

    // The trait stores suffixes without a leading dot and the registry trims a
    // caller's dot, so both "cpp" and ".cc" select this adapter. `.cxx`/`.hpp`
    // are intentionally not claimed: they belong to a future registration job.
    fn extensions(&self) -> &'static [&'static str] {
        &["cpp", "cc"]
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
    fn cpp_adapter_reports_contract_identity() {
        let adapter = CppAdapter::new();
        assert_eq!(adapter.id(), "cpp");
        assert_eq!(adapter.extensions(), &["cpp", "cc"]);
        assert_eq!(CppAdapter::default().id(), "cpp");
        assert!(adapter.discover("int x;\n", "x.cpp").is_empty());
    }

    /// Return-field resolution delegates to the shared grammar-free resolver.
    #[test]
    fn cpp_adapter_resolves_return_fields_from_a_type_index() {
        let source = "Point build() { return Point{}; }";
        let types_source = "struct Point { int x; };";
        let found = CppAdapter.discover(source, "build.cpp");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].returns.type_, "Point");
        assert!(found[0].returns.fields.is_empty());

        let types = CppAdapter.collect_types(types_source);
        let mut resolved = found[0].clone();
        CppAdapter.resolve_return_fields(&mut resolved, &types);
        assert_eq!(resolved.returns.fields.len(), 1);
        assert_eq!(resolved.returns.fields[0].name, "x");
        assert_eq!(resolved.returns.fields[0].type_, "int");
    }
}
