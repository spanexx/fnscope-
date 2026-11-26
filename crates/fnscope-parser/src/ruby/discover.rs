//! Deterministic discovery of Ruby methods and their owning scopes.
//!
//! Only module-level `method`/`singleton_method` nodes and the direct method
//! members of a `class`/`module` body are recorded. A helper nested inside a
//! method body is never a second callable, and `attr_*` calls are not functions.
//! Everything is derived from the tree-sitter AST and exact source slices, so
//! discovery is stable across runs and never invents a module path.
//!
//! Wrapper nodes unwrapped here: `body_statement` (the `body` field of `class`
//! and `module`), `method_parameters` (the `parameters` field of a method), and
//! `argument_list` (the `arguments` field of `attr_*` and `Struct.new` calls).
//! The abstract `_`-prefixed node types in `node-types.json` are field
//! descriptors, not concrete tree nodes, so they are never matched directly.

use std::collections::BTreeMap;

use fnscope_model::{Parameter, ReturnField, ReturnShape};
use serde_json::Value;
use tree_sitter::{Node, Parser, Tree};

use super::{callee_name, node_text, normalize_line, unquote};
use crate::resolve::resolve_fields_from_map;
use crate::types::{DiscoveredFn, FnKind};

/// Parse one Ruby source file, or `None` when no tree can be produced.
fn parse_tree(source: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_ruby::LANGUAGE.into();
    parser.set_language(&language).ok()?;
    parser.parse(source, None)
}

/// Discover every callable in one source file, sorted by `(line, qualified_name)`.
pub fn discover(source: &str, relative_path: &str) -> Vec<DiscoveredFn> {
    let _ = relative_path; // discovery never fabricates a module path or fallback description
    let tree = match parse_tree(source) {
        Some(tree) => tree,
        None => return Vec::new(),
    };
    let root = tree.root_node();
    let types = type_index(root, source);
    let mut found = Vec::new();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        match child.kind() {
            "method" | "singleton_method" => record_function(
                child,
                None,
                child.start_byte(),
                FnKind::Declaration,
                source,
                &types,
                &mut found,
            ),
            "class" | "module" => visit_scope(child, source, &types, &mut found),
            _ => {}
        }
    }
    found.sort_by(|a, b| (a.line, &a.qualified_name).cmp(&(b.line, &b.qualified_name)));
    found.dedup_by(|a, b| a.qualified_name == b.qualified_name && a.line == b.line);
    found
}

/// Record the direct method members of a class/module `body_statement`.
fn visit_scope(
    node: Node<'_>,
    source: &str,
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let class_name = match node.child_by_field_name("name") {
        Some(name) => node_text(&name, source).trim().to_owned(),
        None => return,
    };
    if class_name.is_empty() {
        return;
    }
    let body = match node.child_by_field_name("body") {
        Some(body) => body,
        None => return,
    };
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        match member.kind() {
            "method" | "singleton_method" => record_function(
                member,
                Some(&class_name),
                member.start_byte(),
                FnKind::Method,
                source,
                types,
                out,
            ),
            _ => {}
        }
    }
}

/// Build one `DiscoveredFn` from a Ruby `method` or `singleton_method` node.
fn record_function(
    node: Node<'_>,
    class_name: Option<&str>,
    doc_anchor: usize,
    kind: FnKind,
    source: &str,
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let name = match node.child_by_field_name("name") {
        Some(name) => node_text(&name, source).trim().to_owned(),
        None => return,
    };
    if name.is_empty() {
        return;
    }
    let start = node.start_position();
    let end = node.end_position();
    let qualified_name = match class_name {
        Some(class) => format!("{class}.{name}"),
        None => name.clone(),
    };
    let comments = leading_comments(source, doc_anchor);
    out.push(DiscoveredFn {
        name,
        qualified_name,
        kind,
        line: start.row + 1,
        end_line: end.row + 1,
        source_code: node_text(&node, source),
        parameters: extract_parameters(node.child_by_field_name("parameters"), source, &comments),
        returns: extract_returns(&comments, types),
        doc_description: description_from_comments(&comments),
        tags: Vec::new(),
        is_async: false, // Ruby has no `async def`; the runtime label comes from body effects
        class_name: class_name.map(str::to_owned),
    });
}

/// Contiguous `#` comment lines directly above a declaration, in source order.
fn leading_comments(source: &str, anchor_byte: usize) -> Vec<String> {
    let before = &source[..anchor_byte.min(source.len())];
    let anchor_line = before.bytes().filter(|byte| *byte == b'\n').count();
    if anchor_line == 0 {
        return Vec::new();
    }
    let lines: Vec<&str> = before.lines().collect();
    let mut block = Vec::new();
    let mut row = anchor_line - 1;
    loop {
        let line = lines.get(row).copied().unwrap_or("").trim();
        match line.strip_prefix('#') {
            Some(comment) => block.push(comment.trim().to_owned()),
            None => break,
        }
        if row == 0 {
            break;
        }
        row -= 1;
    }
    block.reverse();
    block
}

/// Plain leading comment lines (YARD tags are parsed separately) joined as prose.
fn description_from_comments(comments: &[String]) -> Option<String> {
    let parts: Vec<String> = comments
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('@'))
        .map(str::to_owned)
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Split a leading `[Type]` from the rest of a YARD tag's text.
fn bracket_value(text: &str) -> Option<(String, String)> {
    let text = text.trim();
    if !text.starts_with('[') {
        return None;
    }
    let close = text.find(']')?;
    Some((
        text[1..close].trim().to_owned(),
        text[close + 1..].trim().to_owned(),
    ))
}

/// `# @param [Type] name Description` lines keyed by parameter name.
fn yard_params(comments: &[String]) -> BTreeMap<String, (String, String)> {
    let mut map = BTreeMap::new();
    for line in comments {
        let Some(rest) = line.trim().strip_prefix("@param") else {
            continue;
        };
        let rest = rest.trim();
        let (type_, rest) =
            bracket_value(rest).unwrap_or_else(|| ("unknown".to_owned(), rest.to_owned()));
        let mut words = rest.split_whitespace();
        let Some(name) = words.next() else {
            continue;
        };
        map.insert(
            name.trim_start_matches('*').trim().to_owned(),
            (type_, words.collect::<Vec<_>>().join(" ")),
        );
    }
    map
}

/// Declared type of the first `# @return [Type]` line, or empty text.
fn yard_return(comments: &[String]) -> String {
    for line in comments {
        if let Some(rest) = line.trim().strip_prefix("@return") {
            return bracket_value(rest.trim())
                .map(|(type_, _)| type_)
                .unwrap_or_default();
        }
    }
    String::new()
}

/// Exact trimmed text of one named field of a node.
fn field_text(node: Node<'_>, field: &str, source: &str) -> String {
    node.child_by_field_name(field)
        .map(|node| node_text(&node, source).trim().to_owned())
        .unwrap_or_default()
}

/// Collect parameters from `method_parameters`, including optional, keyword,
/// splat, hash-splat and block forms. Types and descriptions come from YARD.
fn extract_parameters(
    params: Option<Node<'_>>,
    source: &str,
    comments: &[String],
) -> Vec<Parameter> {
    let mut out = Vec::new();
    let params = match params {
        Some(params) => params,
        None => return out,
    };
    let yard = yard_params(comments);
    let mut cursor = params.walk();
    for child in params.named_children(&mut cursor) {
        // `(name, default, required, fallback_type)` for this formal parameter.
        let (name, default, required, fallback_type) = match child.kind() {
            "identifier" => (
                node_text(&child, source).trim().to_owned(),
                None,
                true,
                "unknown",
            ),
            "optional_parameter" => (
                field_text(child, "name", source),
                Some(field_text(child, "value", source)),
                false,
                "unknown",
            ),
            "keyword_parameter" => {
                let name = field_text(child, "name", source);
                let default = child
                    .child_by_field_name("value")
                    .map(|node| normalize_line(&node_text(&node, source)))
                    .filter(|text| !text.is_empty());
                let required = default.is_none();
                (name, default, required, "unknown")
            }
            "splat_parameter" => {
                let name = field_text(child, "name", source);
                let name = if name.is_empty() {
                    "*".to_owned()
                } else {
                    name
                };
                (name, None, false, "Array")
            }
            "hash_splat_parameter" => {
                let name = field_text(child, "name", source);
                let name = if name.is_empty() {
                    "**".to_owned()
                } else {
                    name
                };
                (name, None, false, "Hash")
            }
            "block_parameter" => {
                let name = field_text(child, "name", source);
                let name = if name.is_empty() {
                    "&".to_owned()
                } else {
                    name
                };
                (name, None, false, "Proc")
            }
            // `...`, destructured and anonymous `**nil` forms have no stable name.
            _ => continue,
        };
        if name.is_empty() {
            continue;
        }
        let (type_, description) = match yard.get(&name) {
            Some((declared, description)) => (
                if declared.is_empty() {
                    fallback_type.to_owned()
                } else {
                    declared.clone()
                },
                description.clone(),
            ),
            None => (fallback_type.to_owned(), String::new()),
        };
        out.push(Parameter {
            name,
            type_,
            required,
            default_value: default.map(Value::String),
            description,
        });
    }
    out
}

/// Return shape from the leading YARD `@return` tag; fields use the shared resolver.
fn extract_returns(comments: &[String], types: &BTreeMap<String, ReturnShape>) -> ReturnShape {
    let type_ = yard_return(comments);
    let fields = if type_.is_empty() {
        Vec::new()
    } else {
        resolve_fields_from_map(&type_, types)
    };
    ReturnShape { type_, fields }
}

/// Build the type index from an already-parsed root.
///
/// Both discovery (for same-file return fields) and `collect_types` (for the
/// project-wide index) call this one function, so the two paths cannot drift.
/// It is private to this module: both callers live in `discover.rs`.
fn type_index(root: Node<'_>, source: &str) -> BTreeMap<String, ReturnShape> {
    let attr_types = yard_attribute_types(source);
    let mut map = BTreeMap::new();
    index_scope_children(root, source, &attr_types, &mut map);
    map
}

/// Index object-shaped types declared in one Ruby source file.
pub fn collect_types(source: &str) -> BTreeMap<String, ReturnShape> {
    let tree = match parse_tree(source) {
        Some(tree) => tree,
        None => return BTreeMap::new(),
    };
    type_index(tree.root_node(), source)
}

/// `# @!attribute [Type] name` lines keyed by attribute name.
fn yard_attribute_types(source: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in source.lines() {
        let trimmed = line.trim();
        let text = trimmed.strip_prefix('#').unwrap_or(trimmed).trim();
        let Some(rest) = text.strip_prefix("@!attribute") else {
            continue;
        };
        let rest = rest.trim();
        let (type_, rest) =
            bracket_value(rest).unwrap_or_else(|| ("unknown".to_owned(), rest.to_owned()));
        if let Some(name) = rest.split_whitespace().next() {
            map.insert(name.to_owned(), type_);
        }
    }
    map
}

/// Index class/module declarations and constant assignments in one scope body.
fn index_scope_children(
    node: Node<'_>,
    source: &str,
    attr_types: &BTreeMap<String, String>,
    map: &mut BTreeMap<String, ReturnShape>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "class" | "module" => index_class(child, source, attr_types, map),
            "assignment" => index_constant_assignment(child, source, map),
            // Method bodies are not type declaration scopes; skip them.
            _ => {}
        }
    }
}

/// Index one `class`/`module` and its `attr_*` fields, then recurse into its body.
fn index_class(
    node: Node<'_>,
    source: &str,
    attr_types: &BTreeMap<String, String>,
    map: &mut BTreeMap<String, ReturnShape>,
) {
    let name = field_text(node, "name", source);
    if name.is_empty() {
        return;
    }
    let mut fields = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for member in body.named_children(&mut cursor) {
            if member.kind() != "call" {
                continue;
            }
            let Some(callee) = callee_name(member, source) else {
                continue;
            };
            if !matches!(
                callee.as_str(),
                "attr_reader" | "attr_writer" | "attr_accessor"
            ) {
                continue;
            }
            if let Some(args) = member.child_by_field_name("arguments") {
                for field_name in argument_words(args, source) {
                    let type_ = attr_types
                        .get(&field_name)
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_owned());
                    fields.push(ReturnField {
                        name: field_name,
                        type_,
                    });
                }
            }
        }
        index_scope_children(body, source, attr_types, map);
    }
    map.insert(
        name.clone(),
        ReturnShape {
            type_: name,
            fields,
        },
    );
}

/// Index `Record = Struct.new(:a, :b)`, `Data.define(...)` and constant aliases.
fn index_constant_assignment(
    node: Node<'_>,
    source: &str,
    map: &mut BTreeMap<String, ReturnShape>,
) {
    let left = match node.child_by_field_name("left") {
        Some(left) => left,
        None => return,
    };
    if left.kind() != "constant" {
        return;
    }
    let name = node_text(&left, source).trim().to_owned();
    if name.is_empty() {
        return;
    }
    let right = match node.child_by_field_name("right") {
        Some(right) => right,
        None => return,
    };
    let shape = if right.kind() == "call" {
        let method = match callee_name(right, source) {
            Some(method) => method,
            None => return,
        };
        let receiver = right
            .child_by_field_name("receiver")
            .map(|node| node_text(&node, source).trim().to_owned())
            .unwrap_or_default();
        if method != "new" && method != "define" {
            return;
        }
        if receiver != "Struct" && receiver != "Data" {
            return;
        }
        let fields = right
            .child_by_field_name("arguments")
            .map(|args| {
                argument_words(args, source)
                    .into_iter()
                    .map(|name| ReturnField {
                        name,
                        type_: "unknown".to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        ReturnShape {
            type_: receiver,
            fields,
        }
    } else if matches!(right.kind(), "constant" | "scope_resolution") {
        ReturnShape {
            type_: node_text(&right, source).trim().to_owned(),
            fields: Vec::new(),
        }
    } else {
        return;
    };
    map.insert(name, shape);
}

/// Symbol and string arguments of an `argument_list`, used for field names.
fn argument_words(node: Node<'_>, source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let name = match child.kind() {
            "simple_symbol" | "delimited_symbol" => node_text(&child, source)
                .trim()
                .trim_start_matches(':')
                .trim()
                .to_owned(),
            "string" => unquote(&node_text(&child, source)),
            _ => continue,
        };
        if !name.is_empty() {
            out.push(name);
        }
    }
    out
}
