//! Kotlin language adapter.
//!
//! All Kotlin grammar knowledge stays in this module and its two children. The
//! adapter is deliberately not registered globally: a caller registers it in a
//! private `Registry` and injects that registry into `scan_with_registry`, so
//! no edit to core, the schema or the UI is needed.

mod body;
mod discover;

use std::collections::BTreeMap;

use fnscope_model::ReturnShape;

use crate::adapter::LanguageAdapter;
use crate::resolve::resolve_fields;
use crate::types::{BodyAnalysis, DiscoveredFn};

/// Stateless adapter for `.kt` sources, registered explicitly by the caller.
pub struct KotlinAdapter;

impl KotlinAdapter {
    /// Construct the adapter so callers can register it without global state.
    pub fn new() -> Self {
        Self
    }
}

impl Default for KotlinAdapter {
    /// Return the same stateless value as [`KotlinAdapter::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// Load the Kotlin grammar through the `-ng` crate so one native tree-sitter runtime is linked.
///
/// `tree-sitter-kotlin` 0.3.8 pins `tree-sitter >= 0.21, < 0.23`, which declares
/// `links = "tree-sitter"` and therefore cannot coexist with this workspace's
/// 0.25 runtime. `tree-sitter-kotlin-ng` 1.1 exposes a `LanguageFn` built only
/// on `tree-sitter-language`, whose dev-only tree-sitter 0.24 dependency is not
/// linked into dependents. Its `LANGUAGE` converts directly into the workspace's
/// `Language`, so no unsafe pointer reinterpretation is needed.
pub(crate) fn kotlin_language() -> tree_sitter::Language {
    tree_sitter_kotlin_ng::LANGUAGE.into()
}

impl LanguageAdapter for KotlinAdapter {
    fn id(&self) -> &'static str {
        "kotlin"
    }

    // The trait stores suffixes without a leading dot and the registry trims a
    // caller's dot, so both "kt" and ".kt" select this adapter. `.kts` scripts
    // are intentionally not claimed: they execute top-level code rather than
    // declaring callable units, and no caller has asked for them to enter the
    // function graph. Keeping one suffix also avoids counting a generated
    // script beside its source as two discoveries of the same callable.
    fn extensions(&self) -> &'static [&'static str] {
        &["kt"]
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
    fn kotlin_adapter_reports_contract_identity() {
        let adapter = KotlinAdapter::new();
        assert_eq!(adapter.id(), "kotlin");
        assert_eq!(adapter.extensions(), &["kt"]);
        assert_eq!(KotlinAdapter::default().id(), "kotlin");
        assert!(adapter.discover("package demo\n", "Main.kt").is_empty());
    }

    /// Return-field resolution delegates to the shared grammar-free resolver.
    #[test]
    fn kotlin_adapter_resolves_return_fields_from_a_type_index() {
        let source = "fun build(): Item = Item()";
        let types_source = "data class Item(val id: Int, val label: String)";
        let found = KotlinAdapter::new().discover(source, "Build.kt");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].returns.type_, "Item");
        assert!(found[0].returns.fields.is_empty());

        let types = KotlinAdapter::new().collect_types(types_source);
        let mut resolved = found[0].clone();
        KotlinAdapter::new().resolve_return_fields(&mut resolved, &types);
        assert_eq!(resolved.returns.fields.len(), 2);
        assert_eq!(resolved.returns.fields[0].name, "id");
        assert_eq!(resolved.returns.fields[1].name, "label");
    }
}
