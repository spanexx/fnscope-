//! Deterministic AST extraction behind a pluggable language-adapter registry.
//! No network, no AI, no heuristics that vary by run.

pub mod adapter;
pub mod body;
pub mod c_lang;
pub mod cpp;
pub mod csharp;
pub mod discover;
pub mod golang;
pub mod java;
pub mod kotlin;
pub mod php;
pub mod python;
pub mod ruby;
pub mod rust;
pub mod swift;
pub mod types;
pub mod typescript;

mod resolve;

pub use adapter::{registry, LanguageAdapter, Registry};
pub use discover::{collect_types, resolve_return_fields};
pub use types::{BodyAnalysis, DiscoveredFn, FnKind, Lang};
