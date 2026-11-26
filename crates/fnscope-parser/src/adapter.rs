//! Pluggable language-adapter seam.
//!
//! Everything a language needs is expressed here in contract shapes
//! ([`DiscoveredFn`], [`BodyAnalysis`], [`ReturnShape`]). The scanner only ever
//! sees a [`LanguageAdapter`]; it never sees a grammar, an extension table or a
//! language enum. Adding a language therefore means registering an adapter, not
//! editing the graph builder.

use std::collections::BTreeMap;

use fnscope_model::ReturnShape;

use crate::c_lang::CAdapter;
use crate::cpp::CppAdapter;
use crate::csharp::CSharpAdapter;
use crate::golang::GoAdapter;
use crate::java::JavaAdapter;
use crate::kotlin::KotlinAdapter;
use crate::php::PhpAdapter;
use crate::python::PythonAdapter;
use crate::ruby::RubyAdapter;
use crate::rust::RustAdapter;
use crate::swift::SwiftAdapter;
use crate::types::{BodyAnalysis, DiscoveredFn, Lang};
use crate::typescript::TypeScriptAdapter;

/// One language's grammar knowledge, expressed entirely in contract shapes.
///
/// Implementations own their extensions, discovery, signature extraction,
/// body analysis, type index contributions and return-field resolution. They
/// never assign ids, build graph edges or write manifests — those belong to
/// `fnscope-core`.
pub trait LanguageAdapter: Send + Sync {
    /// Stable name for diagnostics, such as `typescript` or `rust`.
    fn id(&self) -> &'static str;

    /// Lower-case file suffixes this adapter claims, without the leading dot.
    fn extensions(&self) -> &'static [&'static str];

    /// Discover every callable in `source`, sorted by `(line, qualified_name)`.
    ///
    /// `relative_path` is passed for diagnostics only; discovery never invents
    /// a module path or fallback description from it.
    fn discover(&self, source: &str, relative_path: &str) -> Vec<DiscoveredFn>;

    /// Analyze one discovered function's body into ordered, deterministic facts.
    fn analyze(&self, found: &DiscoveredFn, source: &str, known_names: &[String]) -> BodyAnalysis;

    /// Index object-shaped types declared in `source` for cross-file return fields.
    fn collect_types(&self, source: &str) -> BTreeMap<String, ReturnShape>;

    /// Fill empty return fields of `found` from a project-wide type index.
    fn resolve_return_fields(
        &self,
        found: &mut DiscoveredFn,
        types: &BTreeMap<String, ReturnShape>,
    );
}

/// Ordered collection of adapters with one lookup by file extension.
///
/// Registration order is stable: the first adapter claiming an extension wins.
#[derive(Default)]
pub struct Registry {
    adapters: Vec<Box<dyn LanguageAdapter>>,
}

impl Registry {
    /// Create an empty registry for callers that register all adapters themselves.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create the built-in registry: TypeScript, TSX, JavaScript, JSX, Rust,
    /// Python, Go, Java, C, C++, C#, Kotlin, PHP, Ruby and Swift.
    pub fn with_builtin_adapters() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(TypeScriptAdapter::new(Lang::TypeScript)));
        registry.register(Box::new(TypeScriptAdapter::new(Lang::Tsx)));
        registry.register(Box::new(TypeScriptAdapter::new(Lang::JavaScript)));
        registry.register(Box::new(TypeScriptAdapter::new(Lang::Jsx)));
        registry.register(Box::new(RustAdapter));
        registry.register(Box::new(PythonAdapter::new()));
        registry.register(Box::new(GoAdapter::new()));
        registry.register(Box::new(JavaAdapter::new()));
        registry.register(Box::new(CAdapter::new()));
        registry.register(Box::new(CppAdapter::new()));
        // New languages register last so the original first-claim order cannot change.
        registry.register(Box::new(CSharpAdapter::new()));
        registry.register(Box::new(KotlinAdapter::new()));
        registry.register(Box::new(PhpAdapter::new()));
        registry.register(Box::new(RubyAdapter::new()));
        registry.register(Box::new(SwiftAdapter::new()));
        registry
    }

    /// Add an adapter. Later registrations never override an earlier claim.
    pub fn register(&mut self, adapter: Box<dyn LanguageAdapter>) {
        self.adapters.push(adapter);
    }

    /// Find the adapter claiming `ext`, ignoring case and a leading dot.
    pub fn for_extension(&self, ext: &str) -> Option<&dyn LanguageAdapter> {
        let needle = ext.trim_start_matches('.').to_ascii_lowercase();
        self.adapters.iter().find_map(|adapter| {
            adapter
                .extensions()
                .iter()
                .any(|known| (*known).eq_ignore_ascii_case(needle.as_str()))
                .then(|| adapter.as_ref())
        })
    }
}

/// Built-in registry for callers that do not inject one.
pub fn registry() -> Registry {
    Registry::with_builtin_adapters()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every built-in extension resolves to the expected adapter id.
    #[test]
    fn builtin_registry_resolves_every_extension() {
        let registry = registry();
        for (extension, id) in [
            ("ts", "typescript"),
            ("tsx", "tsx"),
            ("js", "javascript"),
            ("jsx", "jsx"),
            ("rs", "rust"),
            ("py", "python"),
            ("go", "go"),
            ("java", "java"),
            ("c", "c"),
            ("cpp", "cpp"),
            ("cc", "cpp"),
            ("cs", "csharp"),
            ("kt", "kotlin"),
            ("php", "php"),
            ("rb", "ruby"),
            ("swift", "swift"),
        ] {
            let adapter = registry
                .for_extension(extension)
                .unwrap_or_else(|| panic!("no built-in adapter for .{extension}"));
            assert_eq!(adapter.id(), id, "wrong adapter for .{extension}");
        }
    }

    /// An unknown extension resolves to nothing, and matching is case/prefix tolerant.
    #[test]
    fn unregistered_extension_returns_none() {
        let registry = registry();
        assert!(registry.for_extension("zig").is_none());
        assert!(registry.for_extension("").is_none());
        assert_eq!(
            registry.for_extension("RS").map(|adapter| adapter.id()),
            Some("rust")
        );
        assert_eq!(
            registry.for_extension(".ts").map(|adapter| adapter.id()),
            Some("typescript")
        );
    }
}
