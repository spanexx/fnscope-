//! Deterministic discovery of top-level PHP functions and class methods.
//!
//! Only `function_definition` nodes at program/namespace scope and direct
//! `method_declaration` members of a `class_declaration` are recorded. A helper
//! nested in a body is never a second callable. Every filter starts from a
//! named field or an explicitly unwrapped wrapper listed in
//! `docs/contract/adapters/php.md`, so no grammar kind is guessed by text.

use std::collections::BTreeMap;

use fnscope_model::{Parameter, ReturnField, ReturnShape};
use serde_json::Value;
use tree_sitter::{Node, Parser, Tree};

use crate::resolve::resolve_fields_from_map;
use crate::types::{DiscoveredFn, FnKind};

/// Parse one PHP source file with the full PHP grammar, or `None` on failure.
///
/// The cached `tree-sitter-php` crate exports `LANGUAGE_PHP`, not `LANGUAGE`;
/// the full grammar is required so `<?php` tags and inline text are recognized.
fn parse_tree(source: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_php::LANGUAGE_PHP.into();
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
    visit_scope_children(root, source, &types, &mut found);
    found.sort_by(|a, b| (a.line, &a.qualified_name).cmp(&(b.line, &b.qualified_name)));
    found.dedup_by(|a, b| a.qualified_name == b.qualified_name && a.line == b.line);
    found
}

/// Walk the concrete statement children of a program or a namespace body.
fn visit_scope_children(
    scope: Node<'_>,
    source: &str,
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    // A braced `namespace Foo { ... }` hides its statements behind a
    // `compound_statement` body field; an unbraced namespace has no body.
    let body = if scope.kind() == "namespace_definition" {
        match scope.child_by_field_name("body") {
            Some(body) => body,
            None => return,
        }
    } else {
        scope
    };
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        visit_scope_item(child, source, types, out);
    }
}

/// Record the callable shapes that may appear at program or namespace scope.
fn visit_scope_item(
    node: Node<'_>,
    source: &str,
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let item = unwrap_statement(node);
    match item.kind() {
        "function_definition" => record_function(
            item,
            None,
            item.start_byte(),
            FnKind::Declaration,
            source,
            types,
            out,
        ),
        "class_declaration" => visit_class(item, source, types, out),
        "namespace_definition" => visit_scope_children(item, source, types, out),
        _ => {}
    }
}

/// Descend through a hidden `statement` / `expression` supertype wrapper.
fn unwrap_statement<'tree>(mut node: Node<'tree>) -> Node<'tree> {
    while matches!(node.kind(), "statement" | "expression") {
        let mut cursor = node.walk();
        // Bind the consumer so the iterator borrowing `cursor` is dropped first.
        let first = node.named_children(&mut cursor).next();
        match first {
            Some(child) => node = child,
            None => break,
        }
    }
    node
}

/// Record the direct method members of a class body with `Class.method` names.
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
    // `class_declaration` hides its members behind a `declaration_list` body.
    let body = match node.child_by_field_name("body") {
        Some(body) => body,
        None => return,
    };
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        let method = unwrap_statement(member);
        if method.kind() == "method_declaration" {
            record_function(
                method,
                Some(&class_name),
                method.start_byte(),
                FnKind::Method,
                source,
                types,
                out,
            );
        }
    }
}

/// Build one `DiscoveredFn` from a PHP function or method declaration node.
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
        doc_description: leading_doc_comment(source, doc_anchor),
        tags: Vec::new(),
        // This grammar has no `async` keyword on declarations.
        is_async: false,
        class_name: class_name.map(str::to_string),
    });
}

/// Collect named parameters from the `formal_parameters` wrapper.
fn extract_parameters(node: Node<'_>, bytes: &[u8]) -> Vec<Parameter> {
    let mut params = Vec::new();
    let list = match node.child_by_field_name("parameters") {
        Some(list) => list,
        None => return params,
    };
    let mut cursor = list.walk();
    for wrapper in list.named_children(&mut cursor) {
        let param = match wrapper.kind() {
            "simple_parameter" | "variadic_parameter" | "property_promotion_parameter" => wrapper,
            _ => continue,
        };
        let raw_name = match param.child_by_field_name("name") {
            Some(name) => node_text(&name, bytes),
            None => continue,
        };
        // `variable_name` wraps `name`; report the identifier without `$` / `&`.
        let name = raw_name
            .trim()
            .trim_start_matches('&')
            .trim_start_matches('$')
            .trim()
            .to_owned();
        if name.is_empty() {
            continue;
        }
        let type_ = param
            .child_by_field_name("type")
            .map(|node| node_text(&node, bytes).trim().to_owned())
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "unknown".to_owned());
        let default = param
            .child_by_field_name("default_value")
            .map(|node| node_text(&node, bytes).trim().to_owned())
            .filter(|text| !text.is_empty());
        params.push(Parameter {
            name,
            type_,
            required: default.is_none() && param.kind() != "variadic_parameter",
            default_value: default.map(Value::String),
            description: String::new(),
        });
    }
    params
}

/// Read the `: type` annotation; fields resolve through the project type index.
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

/// Index class properties and class constants into per-class `ReturnShape` fields.
pub fn collect_types(source: &str) -> BTreeMap<String, ReturnShape> {
    let tree = match parse_tree(source) {
        Some(tree) => tree,
        None => return BTreeMap::new(),
    };
    type_index(tree.root_node(), source)
}

/// Build one class-shape index for same-file discovery and cross-file collection.
///
/// `discover` passes this map to `extract_returns`; `collect_types` returns the
/// same shape so core can pass it to `resolve_return_fields` for other files.
pub(super) fn type_index(root: Node<'_>, source: &str) -> BTreeMap<String, ReturnShape> {
    let mut map = BTreeMap::new();
    index_classes(root, source, &mut map);
    map
}

/// Find every class declaration, including ones nested in a namespace body.
fn index_classes(node: Node<'_>, source: &str, out: &mut BTreeMap<String, ReturnShape>) {
    if node.kind() == "class_declaration" {
        let name = node
            .child_by_field_name("name")
            .map(|name| node_text(&name, source.as_bytes()).trim().to_owned())
            .unwrap_or_default();
        if !name.is_empty() {
            out.insert(name, class_shape(node, source));
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        index_classes(child, source, out);
    }
}

/// Turn one class's `property_declaration` and `const_declaration` members into fields.
fn class_shape(class: Node<'_>, source: &str) -> ReturnShape {
    let bytes = source.as_bytes();
    let body = match class.child_by_field_name("body") {
        Some(body) => body,
        None => return ReturnShape::default(),
    };
    let mut fields = Vec::new();
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        match member.kind() {
            "property_declaration" => {
                let declared = member
                    .child_by_field_name("type")
                    .map(|node| node_text(&node, bytes).trim().to_owned())
                    .filter(|text| !text.is_empty());
                let mut elements = member.walk();
                for element in member.named_children(&mut elements) {
                    if element.kind() != "property_element" {
                        continue;
                    }
                    let name = element
                        .child_by_field_name("name")
                        .map(|node| bare_variable_name(&node, bytes))
                        .unwrap_or_default();
                    if name.is_empty() {
                        continue;
                    }
                    let type_ = declared.clone().unwrap_or_else(|| {
                        element
                            .child_by_field_name("default_value")
                            .map(|node| infer_literal_type(&node, bytes))
                            .unwrap_or_else(|| "unknown".to_owned())
                    });
                    fields.push(ReturnField { name, type_ });
                }
            }
            // This grammar has no `class_constant` node: class constants are
            // `const_declaration` wrappers around named `const_element`s.
            "const_declaration" => {
                let declared = member
                    .child_by_field_name("type")
                    .map(|node| node_text(&node, bytes).trim().to_owned())
                    .filter(|text| !text.is_empty());
                let mut elements = member.walk();
                for element in member.named_children(&mut elements) {
                    if element.kind() != "const_element" {
                        continue;
                    }
                    // `const_element` has positional children, not a `name` field.
                    let name = const_element_name(element, bytes);
                    if name.is_empty() {
                        continue;
                    }
                    let type_ = declared
                        .clone()
                        .unwrap_or_else(|| first_expression_text(element, bytes));
                    fields.push(ReturnField { name, type_ });
                }
            }
            _ => {}
        }
    }
    ReturnShape {
        type_: "object".to_owned(),
        fields,
    }
}

/// Name child of a `const_element`, whose children are positional, not fields.
fn const_element_name(element: Node<'_>, bytes: &[u8]) -> String {
    let mut cursor = element.walk();
    for child in element.named_children(&mut cursor) {
        if child.kind() == "name" {
            return node_text(&child, bytes).trim().to_owned();
        }
    }
    String::new()
}

/// First non-name child of a `const_element`, i.e. its value expression text.
fn first_expression_text(element: Node<'_>, bytes: &[u8]) -> String {
    let mut cursor = element.walk();
    for child in element.named_children(&mut cursor) {
        if child.kind() != "name" {
            return normalize_line(&node_text(&child, bytes));
        }
    }
    "unknown".to_owned()
}

/// Name under a `variable_name` wrapper, without the PHP `$` sigil.
fn bare_variable_name(node: &Node<'_>, bytes: &[u8]) -> String {
    node_text(node, bytes)
        .trim()
        .trim_start_matches('$')
        .trim()
        .to_owned()
}

/// Conservative display type for a property's default expression.
fn infer_literal_type(node: &Node<'_>, bytes: &[u8]) -> String {
    match node.kind() {
        "integer" => "int".to_owned(),
        "float" => "float".to_owned(),
        "string" | "encapsed_string" | "heredoc" | "nowdoc" => "string".to_owned(),
        "boolean" => "bool".to_owned(),
        "null" => "null".to_owned(),
        "array_creation_expression" => "array".to_owned(),
        "binary_expression" | "unary_op_expression" | "update_expression" => {
            infer_operator_type(node, bytes)
        }
        "parenthesized_expression" => {
            let mut cursor = node.walk();
            // Bind the consumer so the iterator borrowing `cursor` is dropped first.
            let inner = node.named_children(&mut cursor).next();
            match inner {
                Some(child) => infer_literal_type(&child, bytes),
                None => normalize_line(&node_text(node, bytes)),
            }
        }
        _ => normalize_line(&node_text(node, bytes)),
    }
}

/// Type of an operator expression from its operator and its literal operands.
fn infer_operator_type(node: &Node<'_>, bytes: &[u8]) -> String {
    let operator = node
        .child_by_field_name("operator")
        .map(|op| normalize_line(&node_text(&op, bytes)))
        .unwrap_or_default();
    if operator == "." {
        return "string".to_owned();
    }
    if super::is_comparison_operator(&operator) {
        return "bool".to_owned();
    }
    let left = node
        .child_by_field_name("left")
        .or_else(|| node.child_by_field_name("argument"));
    let right = node.child_by_field_name("right");
    let left_type = left
        .map(|inner| infer_literal_type(&inner, bytes))
        .unwrap_or_default();
    let right_type = right
        .map(|inner| infer_literal_type(&inner, bytes))
        .unwrap_or_default();
    match (left_type.as_str(), right_type.as_str()) {
        ("float", _) | (_, "float") => "float".to_owned(),
        ("int", _) | (_, "int") => "int".to_owned(),
        (left, right) if !left.is_empty() && left == right => left.to_owned(),
        _ => normalize_line(&node_text(node, bytes)),
    }
}

/// Contiguous `//` lines or one `/** ... */` block directly above a declaration.
fn leading_doc_comment(source: &str, anchor_byte: usize) -> Option<String> {
    let before = &source[..anchor_byte];
    let anchor_line = before.bytes().filter(|byte| *byte == b'\n').count();
    if anchor_line == 0 {
        return None;
    }
    let lines: Vec<&str> = before.lines().collect();
    let mut row = anchor_line - 1;
    let trimmed = lines.get(row).copied().unwrap_or("").trim();
    let mut block: Vec<String> = Vec::new();
    if trimmed.ends_with("*/") {
        loop {
            let line = lines.get(row).copied().unwrap_or("").trim();
            block.push(strip_doc_line(line));
            if line.starts_with("/**") || row == 0 {
                break;
            }
            row -= 1;
        }
        block.reverse();
    } else {
        loop {
            let line = lines.get(row).copied().unwrap_or("").trim();
            match line.strip_prefix("//") {
                Some(comment) => block.push(comment.trim().to_owned()),
                None => break,
            }
            if row == 0 {
                break;
            }
            row -= 1;
        }
        block.reverse();
    }
    let text = block
        .into_iter()
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty()).then_some(text)
}

/// Remove one doc-comment line's delimiters and leading asterisk.
fn strip_doc_line(line: &str) -> String {
    let mut text = line.trim();
    if let Some(rest) = text.strip_prefix("/**") {
        text = rest.trim();
    }
    if let Some(rest) = text.strip_suffix("*/") {
        text = rest.trim();
    }
    if let Some(rest) = text.strip_prefix('*') {
        text = rest.trim();
    }
    text.to_owned()
}

/// Exact source text of a node, degrading to an empty string for invalid UTF-8.
fn node_text(node: &Node<'_>, bytes: &[u8]) -> String {
    node.utf8_text(bytes).unwrap_or("").to_owned()
}

/// Collapse every whitespace run into a single space and trim the result.
fn normalize_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
