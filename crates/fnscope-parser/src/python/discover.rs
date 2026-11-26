//! Deterministic discovery of module-level Python functions and class methods.
//!
//! Only module-level `function_definition` / `decorated_definition` nodes and
//! the direct function members of a class body are recorded. A helper nested
//! inside a body is never mistaken for a top-level callable. Everything is
//! derived from the tree-sitter AST and exact source slices, so discovery is
//! stable across runs and never invents a module path from `relative_path`.

use std::collections::BTreeMap;

use fnscope_model::{Parameter, ReturnShape};
use serde_json::Value;
use tree_sitter::{Node, Parser, Tree};

use crate::resolve::resolve_fields_from_map;
use crate::types::{DiscoveredFn, FnKind};

/// Parse one Python source file, or `None` when no tree can be produced.
fn parse_tree(source: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
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
        visit_module_item(child, source, &types, &mut found);
    }
    found.sort_by(|a, b| (a.line, &a.qualified_name).cmp(&(b.line, &b.qualified_name)));
    found.dedup_by(|a, b| a.qualified_name == b.qualified_name && a.line == b.line);
    found
}

/// Visit one module item, recording only shapes that can declare a callable.
fn visit_module_item(
    node: Node<'_>,
    source: &str,
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    match node.kind() {
        "function_definition" => record_function(
            node,
            None,
            node.start_byte(),
            FnKind::Declaration,
            source,
            types,
            out,
        ),
        "decorated_definition" => {
            if let Some(function) = wrapped_function(node) {
                record_function(
                    function,
                    None,
                    node.start_byte(),
                    FnKind::Declaration,
                    source,
                    types,
                    out,
                );
            }
        }
        "class_definition" => visit_class(node, source, types, out),
        _ => {}
    }
}

/// Record the direct function members of a class body with `Class.method` names.
fn visit_class(
    node: Node<'_>,
    source: &str,
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let class_name = node
        .child_by_field_name("name")
        .map(|name| node_text(&name, source.as_bytes()))
        .unwrap_or_default();
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
            "function_definition" => record_function(
                member,
                Some(&class_name),
                member.start_byte(),
                FnKind::Method,
                source,
                types,
                out,
            ),
            "decorated_definition" => {
                if let Some(function) = wrapped_function(member) {
                    record_function(
                        function,
                        Some(&class_name),
                        member.start_byte(),
                        FnKind::Method,
                        source,
                        types,
                        out,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Find the `function_definition` wrapped by a `decorated_definition`.
fn wrapped_function(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    let found = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "function_definition");
    found
}

/// Build one `DiscoveredFn` from a Python `function_definition` node.
fn record_function(
    node: Node<'_>,
    class_name: Option<&str>,
    doc_anchor: usize,
    kind: FnKind,
    source: &str,
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let name = node
        .child_by_field_name("name")
        .map(|name| node_text(&name, source.as_bytes()))
        .unwrap_or_default();
    if name.is_empty() {
        return;
    }
    let bytes = source.as_bytes();
    let start = node.start_position();
    let end = node.end_position();
    let qualified_name = match class_name {
        Some(class) => format!("{class}.{name}"),
        None => name.clone(),
    };
    let doc_description =
        body_docstring(node, source).or_else(|| leading_hash_comment(source, doc_anchor));

    out.push(DiscoveredFn {
        name,
        qualified_name,
        kind,
        line: start.row + 1,
        end_line: end.row + 1,
        source_code: String::from_utf8_lossy(&bytes[node.start_byte()..node.end_byte()])
            .into_owned(),
        parameters: extract_parameters(node, bytes),
        returns: extract_returns(node, bytes, types),
        doc_description,
        tags: Vec::new(),
        is_async: is_async(node),
        class_name: class_name.map(str::to_string),
    });
}

/// Collect named parameters, including defaults and exact annotation source text.
fn extract_parameters(node: Node<'_>, bytes: &[u8]) -> Vec<Parameter> {
    let mut params = Vec::new();
    let list = match node.child_by_field_name("parameters") {
        Some(list) => list,
        None => return params,
    };
    let mut cursor = list.walk();
    for wrapper in list.named_children(&mut cursor) {
        // `parameters` holds `parameter` wrappers; the inner node carries the fields.
        let param = if wrapper.kind() == "parameter" {
            match wrapper.named_child(0) {
                Some(inner) => inner,
                None => continue,
            }
        } else {
            wrapper
        };
        let (name, type_, default) = match parameter_parts(param, bytes) {
            Some(parts) => parts,
            None => continue,
        };
        // `self` and `cls` are receivers, not caller-supplied parameters.
        if name.is_empty() || name == "self" || name == "cls" {
            continue;
        }
        params.push(Parameter {
            name,
            type_: type_.unwrap_or_else(|| "unknown".to_owned()),
            required: default.is_none(),
            default_value: default.map(Value::String),
            description: String::new(),
        });
    }
    params
}

/// Name, annotation and default of one unwrapped Python parameter node.
fn parameter_parts(
    node: Node<'_>,
    bytes: &[u8],
) -> Option<(String, Option<String>, Option<String>)> {
    let annotated = |node: Node<'_>| {
        node.child_by_field_name("type")
            .map(|node| node_text(&node, bytes).trim().to_owned())
            .filter(|text| !text.is_empty())
    };
    let defaulted = |node: Node<'_>| {
        node.child_by_field_name("value")
            .map(|node| node_text(&node, bytes).trim().to_owned())
            .filter(|text| !text.is_empty())
    };
    match node.kind() {
        "identifier" => Some((node_text(&node, bytes), None, None)),
        "typed_parameter" => {
            let name = node.named_child(0).map(|node| node_text(&node, bytes))?;
            Some((name, annotated(node), None))
        }
        "default_parameter" => {
            let name = node
                .child_by_field_name("name")
                .map(|node| node_text(&node, bytes))?;
            Some((name, None, defaulted(node)))
        }
        "typed_default_parameter" => {
            let name = node
                .child_by_field_name("name")
                .map(|node| node_text(&node, bytes))?;
            Some((name, annotated(node), defaulted(node)))
        }
        // Splat and separator parameters are not named value parameters.
        _ => None,
    }
}

/// Read the `->` annotation; fields resolve through the project type index only.
fn extract_returns(
    node: Node<'_>,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
) -> ReturnShape {
    let type_ = node
        .child_by_field_name("return_type")
        .map(|node| node_text(&node, bytes).trim().to_owned())
        .unwrap_or_default();
    let fields = if type_.is_empty() {
        Vec::new()
    } else {
        resolve_fields_from_map(&type_, types)
    };
    ReturnShape { type_, fields }
}

/// Index simple module-level aliases such as `UserId = int` for return fields.
fn type_index(root: Node<'_>, source: &str) -> BTreeMap<String, ReturnShape> {
    let bytes = source.as_bytes();
    let mut map = BTreeMap::new();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        // Module-level assignments arrive wrapped in an `expression_statement`.
        let statement = if child.kind() == "expression_statement" {
            match child.named_child(0) {
                Some(inner) => inner,
                None => continue,
            }
        } else {
            child
        };
        if statement.kind() != "assignment" {
            continue;
        }
        let left = match statement.child_by_field_name("left") {
            Some(left) => left,
            None => continue,
        };
        if left.kind() != "identifier" {
            continue;
        }
        let right = match statement.child_by_field_name("right") {
            Some(right) => right,
            None => continue,
        };
        if !is_type_expression(right.kind()) {
            continue;
        }
        let name = node_text(&left, bytes).trim().to_owned();
        let type_ = node_text(&right, bytes).trim().to_owned();
        if name.is_empty() || type_.is_empty() {
            continue;
        }
        map.insert(
            name,
            ReturnShape {
                type_,
                fields: Vec::new(),
            },
        );
    }
    map
}

/// Whether a right-hand node is a type expression rather than a literal value.
fn is_type_expression(kind: &str) -> bool {
    matches!(
        kind,
        "identifier" | "attribute" | "subscript" | "generic_type" | "type"
    )
}

/// Index object-shaped types declared in one Python source file.
pub fn collect_types(source: &str) -> BTreeMap<String, ReturnShape> {
    let tree = match parse_tree(source) {
        Some(tree) => tree,
        None => return BTreeMap::new(),
    };
    type_index(tree.root_node(), source)
}

/// First docstring expression of a function body, if one is present.
fn body_docstring(node: Node<'_>, source: &str) -> Option<String> {
    let body = node.child_by_field_name("body")?;
    if body.kind() != "block" {
        return None;
    }
    let mut cursor = body.walk();
    let first = body
        .named_children(&mut cursor)
        .find(|child| child.kind() == "expression_statement")?;
    let mut inner = first.walk();
    let string = first
        .named_children(&mut inner)
        .find(|child| matches!(child.kind(), "string" | "concatenated_string"))?;
    Some(unquote(&node_text(&string, source.as_bytes())))
}

/// Contiguous `#` comments directly above a declaration, or `None`.
fn leading_hash_comment(source: &str, anchor_byte: usize) -> Option<String> {
    let before = &source[..anchor_byte];
    let anchor_line = before.bytes().filter(|byte| *byte == b'\n').count();
    if anchor_line == 0 {
        return None;
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
    if block.is_empty() {
        return None;
    }
    block.reverse();
    Some(block.join(" "))
}

/// True when `async` is a keyword child of the definition, not text in its body.
fn is_async(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .any(|child| child.kind() == "async");
    found
}

/// Exact source text of a node, degrading to an empty string for invalid UTF-8.
fn node_text(node: &Node<'_>, bytes: &[u8]) -> String {
    node.utf8_text(bytes).unwrap_or("").to_owned()
}

/// Remove one layer of Python string quotes, including triple-quoted docstrings.
fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    for quote in ["\"\"\"", "'''"] {
        if trimmed.len() >= 6 && trimmed.starts_with(quote) && trimmed.ends_with(quote) {
            return trimmed[3..trimmed.len() - 3].trim().to_owned();
        }
    }
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'\'' | b'"') && bytes[bytes.len() - 1] == bytes[0] {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    trimmed.to_owned()
}
