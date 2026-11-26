//! C language adapter.
//!
//! The directory is named `c_lang` so it cannot be confused with the
//! `fnscope-parser` crate itself or with the `c` toolchain. All C grammar
//! knowledge stays inside this module and its two children; registration is a
//! separate, reviewed change, so this adapter never edits core or global state.
//!
//! Only `.c` sources are claimed. Header files (`.h`) are deliberately excluded
//! because they repeat declarations that already live in a `.c` implementation:
//! discovering both would create duplicate functions with different ids.

mod body;
mod discover;

use std::collections::BTreeMap;

use fnscope_model::ReturnShape;

use crate::adapter::LanguageAdapter;
use crate::resolve::resolve_fields;
use crate::types::{BodyAnalysis, DiscoveredFn};

/// Stateless adapter for `.c` sources, registered by a separate job.
pub struct CAdapter;

impl CAdapter {
    /// Construct the adapter so callers can register it without global state.
    pub fn new() -> Self {
        Self
    }
}

impl Default for CAdapter {
    /// Return the same stateless value as [`CAdapter::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageAdapter for CAdapter {
    fn id(&self) -> &'static str {
        "c"
    }

    // The trait stores suffixes without a leading dot and the registry trims a
    // caller's dot, so both "c" and ".c" select this adapter. Headers are not
    // claimed: see the module note about duplicate declarations.
    fn extensions(&self) -> &'static [&'static str] {
        &["c"]
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
    fn c_adapter_reports_contract_identity() {
        let adapter = CAdapter::new();
        assert_eq!(adapter.id(), "c");
        assert_eq!(adapter.extensions(), &["c"]);
        assert_eq!(CAdapter::default().id(), "c");
        assert_eq!(
            adapter
                .discover("int main(void) { return 0; }\n", "main.c")
                .len(),
            1
        );
    }

    /// Return-field resolution delegates to the shared grammar-free resolver.
    #[test]
    fn c_adapter_resolves_return_fields_from_a_type_index() {
        // The typedef lives in another file, so discovery leaves fields empty
        // and the project-wide index supplied later must fill them.
        let source = "Point make(void) { Point p; return p; }";
        let types_source = "typedef struct Point { int x; int y; } Point;";
        let found = CAdapter::new().discover(source, "make.c");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].returns.type_, "Point");
        assert!(found[0].returns.fields.is_empty());

        let types = CAdapter::new().collect_types(types_source);
        let mut resolved = found[0].clone();
        CAdapter::new().resolve_return_fields(&mut resolved, &types);
        assert_eq!(resolved.returns.fields.len(), 2);
        assert_eq!(resolved.returns.fields[0].name, "x");
        assert_eq!(resolved.returns.fields[0].type_, "int");
        assert_eq!(resolved.returns.fields[1].name, "y");
    }
}
