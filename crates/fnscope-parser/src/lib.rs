//! Deterministic TypeScript/JavaScript AST extraction. No network, no AI, no heuristics that vary by run.

pub mod body;
pub mod discover;
pub mod types;

pub use discover::{collect_types, resolve_return_fields};
pub use types::{BodyAnalysis, DiscoveredFn, FnKind, Lang};
