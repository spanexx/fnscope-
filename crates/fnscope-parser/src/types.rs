//! Shared, language-agnostic shapes passed between the discovery and body-analysis passes.
//! Orchestrator-owned integration surface — do not edit.

use fnscope_model::{ErrorSpec, Parameter, Purity, ReturnShape, Step};

/// The concrete grammar used to parse a source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    TypeScript,
    Tsx,
    JavaScript,
    Jsx,
    /// Rust source (`.rs`), supported by the same deterministic AST pipeline.
    Rust,
}

/// How a function was declared in the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FnKind {
    Declaration,
    Arrow,
    Method,
}

/// One function found by the deterministic discovery pass.
#[derive(Clone, Debug, PartialEq)]
pub struct DiscoveredFn {
    pub name: String,
    pub qualified_name: String,
    pub kind: FnKind,
    /// 1-based line of the function's first byte.
    pub line: usize,
    /// 1-based line of the function's last byte.
    pub end_line: usize,
    /// Exact source slice from the first to the last byte of the function node.
    pub source_code: String,
    pub parameters: Vec<Parameter>,
    pub returns: ReturnShape,
    pub doc_description: Option<String>,
    pub tags: Vec<String>,
    pub is_async: bool,
    pub class_name: Option<String>,
}

/// Deterministic facts extracted from a function body.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyAnalysis {
    pub steps: Vec<Step>,
    pub errors: Vec<ErrorSpec>,
    pub purity: Purity,
    pub permissions: Vec<String>,
    /// Raw callee identifiers in step order; core resolves these to `callsFnId`.
    pub callee_names: Vec<String>,
    pub timeout_ms: u64,
    pub runtime: String,
}

impl Default for BodyAnalysis {
    fn default() -> Self {
        Self {
            steps: Vec::new(),
            errors: Vec::new(),
            purity: Purity::Pure,
            permissions: Vec::new(),
            callee_names: Vec::new(),
            timeout_ms: 150,
            runtime: "universal".to_string(),
        }
    }
}
