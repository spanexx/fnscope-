//! Language-neutral return-field lookup over a project-wide type index.
//!
//! This module only manipulates declared type *names*. It owns no tree-sitter
//! grammar and no extension table, so both built-in adapters and the
//! compatibility wrapper can share one deterministic lookup.

use std::collections::BTreeMap;

use fnscope_model::{ReturnField, ReturnShape};

use crate::types::DiscoveredFn;

/// Fill empty return fields from the declared type name, leaving the type text unchanged.
pub(crate) fn resolve_fields(found: &mut DiscoveredFn, types: &BTreeMap<String, ReturnShape>) {
    if !found.returns.fields.is_empty() {
        return;
    }
    found.returns.fields = resolve_fields_from_map(&found.returns.type_, types);
}

/// Look up return fields by exact declared text, bare name, then wrapper payloads.
pub(crate) fn resolve_fields_from_map(
    type_: &str,
    types: &BTreeMap<String, ReturnShape>,
) -> Vec<ReturnField> {
    if let Some(shape) = types.get(type_.trim()) {
        return shape.fields.clone();
    }
    if let Some(shape) = types.get(type_lookup_name(type_)) {
        return shape.fields.clone();
    }
    // Rust wrappers hide the payload type: report the declared text but inspect the payload.
    for name in rust_wrapper_candidates(type_) {
        if let Some(shape) = types.get(&name) {
            return shape.fields.clone();
        }
    }
    Vec::new()
}

/// Payload type names hidden inside `Result<T, E>`, `Option<T>` and `Vec<T>`.
fn rust_wrapper_candidates(type_: &str) -> Vec<String> {
    let text = type_.trim();
    let open = match text.find('<') {
        Some(index) => index,
        None => return Vec::new(),
    };
    let close = match text.rfind('>') {
        Some(index) if index > open => index,
        _ => return Vec::new(),
    };
    let wrapper = strip_rust_generics(&text[..open]).to_ascii_lowercase();
    if !matches!(wrapper.as_str(), "result" | "option" | "vec") {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    for arg in split_generic_args(&text[open + 1..close]) {
        for name in rust_wrapper_candidates(arg)
            .into_iter()
            .chain(Some(arg.to_string()))
        {
            let bare = strip_rust_generics(&name);
            if !bare.is_empty() && !candidates.contains(&bare) {
                candidates.push(bare);
            }
        }
    }
    candidates
}

/// Split generic arguments on top-level commas, ignoring commas nested in other generics.
fn split_generic_args(args: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in args.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&args[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&args[start..]);
    parts
}

/// Reduce a declared type to the bare name used as a key in the type index.
fn type_lookup_name(type_: &str) -> &str {
    let mut name = unwrap_promise(type_.trim());
    if let Some(generic_start) = name.find('<') {
        name = name[..generic_start].trim();
    }
    if let Some(dot) = name.rfind('.') {
        name = name[dot + 1..].trim();
    }
    name
}

/// Unwrap `Promise<T>` so a local type `T` can be inspected for fields.
///
/// The reported `ReturnShape::type_` keeps the full declared text (for example
/// `Promise<Role>`); only field lookup uses the unwrapped name.
fn unwrap_promise(type_: &str) -> &str {
    let type_ = type_.trim();
    if let Some(inner) = type_.strip_prefix("Promise<") {
        if let Some(inner) = inner.strip_suffix('>') {
            return inner.trim();
        }
    }
    type_
}

/// Reduce a qualified/generic Rust type to its bare type name.
fn strip_rust_generics(text: &str) -> String {
    let mut name = text.trim();
    if let Some(index) = name.find('<') {
        name = &name[..index];
    }
    if let Some(index) = name.rfind("::") {
        name = &name[index + 2..];
    }
    name.trim().to_string()
}
