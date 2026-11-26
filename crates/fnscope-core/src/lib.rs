//! Deterministic scanning and manifest I/O for fnscope.
//!
//! [`scan`] builds a validated but un-enriched [`fnscope_model::FnGraph`]; an
//! enrichment pass may run between scanning and [`write_manifest`] without
//! changing any id or content hash.

pub mod manifest;
pub mod normalize;
pub mod scan;

pub use manifest::{read_manifest, write_manifest};
pub use normalize::{ast_hash, make_id, normalize_source};
pub use scan::{scan, ScanOptions, ScanReport};
