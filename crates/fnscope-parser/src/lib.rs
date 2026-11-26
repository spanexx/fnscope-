//! Deterministic AST extraction behind a pluggable language-adapter registry.
//! No network, no AI, no heuristics that vary by run.

pub mod adapter;
pub mod body;
pub mod discover;
pub mod rust;
pub mod types;
pub mod typescript;

mod resolve;

pub use adapter::{registry, LanguageAdapter, Registry};
pub use discover::{collect_types, resolve_return_fields};
pub use types::{BodyAnalysis, DiscoveredFn, FnKind, Lang};
