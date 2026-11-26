//! Deterministic discovery of C# callables inside namespace-level types.
//!
//! Only `class_declaration`, `struct_declaration` and `record_declaration`
//! reachable from a namespace or the compilation unit are walked. Their direct
//! `method_declaration` and `constructor_declaration` members, C# 12 primary
//! constructors, and first-level `local_function_statement` nodes become
//! callables. A local function nested inside another local function is skipped,
//! and discovery never invents a module path from `relative_path`.

use std::collections::BTreeMap;

use fnscope_model::{Parameter, ReturnField, ReturnShape};
use serde_json::Value;
use tree_sitter::{Node, Parser, Tree};

use crate::resolve::resolve_fields_from_map;
use crate::types::{DiscoveredFn, FnKind};

/// Node kinds that own a callable and may be declared inside a namespace.
const TYPE_KINDS: &[&str] = &[
    "class_declaration",
    "struct_declaration",
    "record_declaration",
];

/// Parse one C# source file, or `None` when no tree can be produced.
fn parse_tree(source: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_c_sharp::LANGUAGE.into();
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
            "namespace_declaration" => {
                if let Some(body) = child.child_by_field_name("body") {
                    visit_declaration_list(body, source, &types, &mut found);
                }
            }
            kind if TYPE_KINDS.contains(&kind) => visit_type(child, source, &types, &mut found),
            "global_statement" => visit_global_statement(child, source, &types, &mut found),
            _ => {}
        }
    }
    found.sort_by(|a, b| (a.line, &a.qualified_name).cmp(&(b.line, &b.qualified_name)));
    found.dedup_by(|a, b| a.qualified_name == b.qualified_name && a.line == b.line);
    found
}

/// Visit the `declaration_list` body of a namespace, without a type qualifier.
fn visit_declaration_list(
    body: Node<'_>,
    source: &str,
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        match member.kind() {
            "namespace_declaration" => {
                if let Some(inner) = member.child_by_field_name("body") {
                    visit_declaration_list(inner, source, types, out);
                }
            }
            kind if TYPE_KINDS.contains(&kind) => visit_type(member, source, types, out),
            "method_declaration" => {
                record_member(member, None, FnKind::Method, "returns", source, types, out);
            }
            "constructor_declaration" => {
                record_member(member, None, FnKind::Method, "void", source, types, out);
            }
            // Attributes, fields and other declarations are not callables here.
            _ => {}
        }
    }
}

/// Discover the callables owned by one class, struct or record declaration.
fn visit_type(
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
    if let Some(body) = node.child_by_field_name("body") {
        visit_typed_declaration_list(body, &class_name, source, types, out);
    }
    if let Some(primary) = primary_constructor(node) {
        record_primary_constructor(node, primary, &class_name, source, out);
    }
}

/// Visit the members of a type body, qualifying callables with the type name.
fn visit_typed_declaration_list(
    body: Node<'_>,
    class_name: &str,
    source: &str,
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        match member.kind() {
            kind if TYPE_KINDS.contains(&kind) => visit_type(member, source, types, out),
            "method_declaration" => {
                record_member(
                    member,
                    Some(class_name),
                    FnKind::Method,
                    "returns",
                    source,
                    types,
                    out,
                );
            }
            "constructor_declaration" => {
                record_member(
                    member,
                    Some(class_name),
                    FnKind::Method,
                    "void",
                    source,
                    types,
                    out,
                );
            }
            // Properties, fields and nested types are not callables here.
            _ => {}
        }
    }
}

/// First-level local functions in a top-level statement program are callables.
fn visit_global_statement(
    node: Node<'_>,
    source: &str,
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "local_function_statement" => {
                record_local_function(child, "", None, source, types, out);
            }
            "block" => {
                let mut inner = child.walk();
                for statement in child.named_children(&mut inner) {
                    if statement.kind() == "local_function_statement" {
                        record_local_function(statement, "", None, source, types, out);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The `parameter_list` child used by a class/struct/record primary constructor.
fn primary_constructor(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    let found = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "parameter_list");
    found
}

/// Record a C# 12 primary constructor from a type header.
fn record_primary_constructor(
    owner: Node<'_>,
    primary: Node<'_>,
    class_name: &str,
    source: &str,
    out: &mut Vec<DiscoveredFn>,
) {
    let bytes = source.as_bytes();
    let qualified_name = format!("{class_name}.{class_name}");
    let source_code =
        String::from_utf8_lossy(&bytes[owner.start_byte()..primary.end_byte()]).into_owned();
    push_function(
        owner,
        class_name.to_owned(),
        qualified_name,
        FnKind::Method,
        Some(class_name.to_owned()),
        ReturnShape {
            type_: "void".to_owned(),
            fields: Vec::new(),
        },
        extract_parameters(primary, bytes),
        doc_description(source, owner),
        false,
        primary.end_position().row + 1,
        source_code,
        out,
    );
}

/// Record a type member and its first-level local functions.
fn record_member(
    node: Node<'_>,
    class_name: Option<&str>,
    kind: FnKind,
    return_field_or_void: &str,
    source: &str,
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let bytes = source.as_bytes();
    let name = node
        .child_by_field_name("name")
        .map(|name| node_text(&name, bytes))
        .unwrap_or_default();
    if name.is_empty() {
        return;
    }
    let qualified_name = match class_name {
        Some(class) => format!("{class}.{name}"),
        None => name.clone(),
    };
    let return_type = if return_field_or_void == "void" {
        "void".to_owned()
    } else {
        node.child_by_field_name(return_field_or_void)
            .map(|node| node_text(&node, bytes).trim().to_owned())
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "void".to_owned())
    };
    let returns = ReturnShape {
        fields: resolve_fields_from_map(&return_type, types),
        type_: return_type,
    };
    push_function(
        node,
        name,
        qualified_name.clone(),
        kind,
        class_name.map(str::to_owned),
        returns,
        node.child_by_field_name("parameters")
            .map(|list| extract_parameters(list, bytes))
            .unwrap_or_default(),
        doc_description(source, node),
        is_async(node, bytes),
        node.end_position().row + 1,
        String::from_utf8_lossy(&bytes[node.start_byte()..node.end_byte()]).into_owned(),
        out,
    );

    if let Some(body) = node.child_by_field_name("body") {
        if body.kind() == "block" {
            let mut cursor = body.walk();
            for statement in body.named_children(&mut cursor) {
                if statement.kind() == "local_function_statement" {
                    record_local_function(
                        statement,
                        &qualified_name,
                        class_name,
                        source,
                        types,
                        out,
                    );
                }
            }
        }
    }
}

/// Record a first-level local function; deeper local functions are not descended into.
fn record_local_function(
    node: Node<'_>,
    owner: &str,
    class_name: Option<&str>,
    source: &str,
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let bytes = source.as_bytes();
    let name = node
        .child_by_field_name("name")
        .map(|name| node_text(&name, bytes))
        .unwrap_or_default();
    if name.is_empty() {
        return;
    }
    let qualified_name = if owner.is_empty() {
        name.clone()
    } else {
        format!("{owner}.{name}")
    };
    let return_type = node
        .child_by_field_name("type")
        .map(|node| node_text(&node, bytes).trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "void".to_owned());
    let returns = ReturnShape {
        fields: resolve_fields_from_map(&return_type, types),
        type_: return_type,
    };
    push_function(
        node,
        name,
        qualified_name,
        FnKind::Declaration,
        class_name.map(str::to_owned),
        returns,
        node.child_by_field_name("parameters")
            .map(|list| extract_parameters(list, bytes))
            .unwrap_or_default(),
        doc_description(source, node),
        is_async(node, bytes),
        node.end_position().row + 1,
        String::from_utf8_lossy(&bytes[node.start_byte()..node.end_byte()]).into_owned(),
        out,
    );
}

/// Append one fully built callable; this helper never descends into a body.
#[allow(clippy::too_many_arguments)]
fn push_function(
    node: Node<'_>,
    name: String,
    qualified_name: String,
    kind: FnKind,
    class_name: Option<String>,
    returns: ReturnShape,
    parameters: Vec<Parameter>,
    doc_description: Option<String>,
    is_async: bool,
    end_line: usize,
    source_code: String,
    out: &mut Vec<DiscoveredFn>,
) {
    out.push(DiscoveredFn {
        name,
        qualified_name,
        kind,
        line: node.start_position().row + 1,
        end_line,
        source_code,
        parameters,
        returns,
        doc_description,
        tags: Vec::new(),
        is_async,
        class_name,
    });
}

/// Read `parameter` children and their optional `= expression` defaults.
fn extract_parameters(list: Node<'_>, bytes: &[u8]) -> Vec<Parameter> {
    let mut params = Vec::new();
    let mut cursor = list.walk();
    for child in list.named_children(&mut cursor) {
        if child.kind() != "parameter" {
            continue;
        }
        let name = match child.child_by_field_name("name") {
            Some(node) => node_text(&node, bytes).trim().to_owned(),
            None => continue,
        };
        if name.is_empty() {
            continue;
        }
        let type_ = child
            .child_by_field_name("type")
            .map(|node| node_text(&node, bytes).trim().to_owned())
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "unknown".to_owned());
        let default = value_after_equals(child)
            .map(|node| node_text(&node, bytes).trim().to_owned())
            .filter(|text| !text.is_empty());
        params.push(Parameter {
            name,
            type_,
            // Only this parameter's own `?` or `=` makes it optional; a `?`
            // nested inside a generic type argument does not.
            required: default.is_none() && !is_nullable_parameter(child),
            default_value: default.map(Value::String),
            description: String::new(),
        });
    }
    params
}

/// Index property and field declarations of every class, struct and record.
pub fn collect_types(source: &str) -> BTreeMap<String, ReturnShape> {
    let tree = match parse_tree(source) {
        Some(tree) => tree,
        None => return BTreeMap::new(),
    };
    type_index(tree.root_node(), source)
}

/// Build the same-file type index for one parsed tree.
pub(crate) fn type_index(root: Node<'_>, source: &str) -> BTreeMap<String, ReturnShape> {
    let mut map = BTreeMap::new();
    visit_type_index(root, source, &mut map);
    map
}

/// Recursive half of [`type_index`], so nested types are indexed too.
fn visit_type_index(node: Node<'_>, source: &str, map: &mut BTreeMap<String, ReturnShape>) {
    if TYPE_KINDS.contains(&node.kind()) {
        index_one_type(node, source, map);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_type_index(child, source, map);
    }
}

/// Add one type's named fields to the index when it declares any.
fn index_one_type(node: Node<'_>, source: &str, map: &mut BTreeMap<String, ReturnShape>) {
    let name = node
        .child_by_field_name("name")
        .map(|name| node_text(&name, source.as_bytes()).trim().to_owned())
        .unwrap_or_default();
    if name.is_empty() {
        return;
    }
    let mut fields = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for member in body.named_children(&mut cursor) {
            match member.kind() {
                "field_declaration" => collect_field_declaration(member, source, &mut fields),
                "property_declaration" => collect_property_declaration(member, source, &mut fields),
                _ => {}
            }
        }
    }
    if let Some(primary) = primary_constructor(node) {
        for parameter in extract_parameters(primary, source.as_bytes()) {
            fields.push(ReturnField {
                name: parameter.name,
                type_: parameter.type_,
            });
        }
    }
    if !fields.is_empty() {
        map.insert(
            name.clone(),
            ReturnShape {
                type_: name,
                fields,
            },
        );
    }
}

/// Append one field per `variable_declarator` of a `field_declaration`.
fn collect_field_declaration(node: Node<'_>, source: &str, fields: &mut Vec<ReturnField>) {
    let bytes = source.as_bytes();
    let mut cursor = node.walk();
    let declaration = match node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "variable_declaration")
    {
        Some(node) => node,
        None => return,
    };
    let type_ = declaration
        .child_by_field_name("type")
        .map(|node| node_text(&node, bytes).trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());
    let mut declarators = declaration.walk();
    for declarator in declaration.named_children(&mut declarators) {
        if declarator.kind() != "variable_declarator" {
            continue;
        }
        if let Some(name) = declarator.child_by_field_name("name") {
            let name = node_text(&name, bytes).trim().to_owned();
            if !name.is_empty() {
                fields.push(ReturnField {
                    name,
                    type_: type_.clone(),
                });
            }
        }
    }
}

/// Append one field for a `property_declaration` with a name and a type.
fn collect_property_declaration(node: Node<'_>, source: &str, fields: &mut Vec<ReturnField>) {
    let bytes = source.as_bytes();
    let name = match node.child_by_field_name("name") {
        Some(node) => node_text(&node, bytes).trim().to_owned(),
        None => return,
    };
    let type_ = match node.child_by_field_name("type") {
        Some(node) => node_text(&node, bytes).trim().to_owned(),
        None => return,
    };
    if !name.is_empty() && !type_.is_empty() {
        fields.push(ReturnField { name, type_ });
    }
}

/// True when the declaration carries an `async` modifier child.
fn is_async(node: Node<'_>, bytes: &[u8]) -> bool {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .any(|child| child.kind() == "modifier" && node_text(&child, bytes).trim() == "async");
    found
}

/// Named value after an `=` token in a parameter or variable declarator.
///
/// `expression` is a hidden supertype, so the initializer is a concrete
/// literal/call node; matching a child of kind `expression` never finds it.
pub(super) fn value_after_equals<'tree>(node: Node<'tree>) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    let children: Vec<Node<'tree>> = node.children(&mut cursor).collect();
    for pair in children.windows(2) {
        if pair[0].kind() == "=" && pair[1].is_named() {
            return Some(pair[1]);
        }
    }
    None
}

/// True when a parameter's own type carries the `?` nullable marker.
fn is_nullable_parameter(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    if node.children(&mut cursor).any(|child| child.kind() == "?") {
        return true;
    }
    match node.child_by_field_name("type") {
        Some(type_node) => {
            let mut type_cursor = type_node.walk();
            let found = type_node
                .children(&mut type_cursor)
                .any(|child| child.kind() == "?");
            found
        }
        None => false,
    }
}

/// Leading doc comment of a callable, inheriting its type's doc block when absent.
fn doc_description(source: &str, node: Node<'_>) -> Option<String> {
    if let Some(text) = leading_slash_comments(source, node.start_byte()) {
        return Some(text);
    }
    let mut current = node.parent();
    while let Some(parent) = current {
        if TYPE_KINDS.contains(&parent.kind()) {
            return leading_slash_comments(source, parent.start_byte());
        }
        current = parent.parent();
    }
    None
}

/// Contiguous `//` or `///` lines directly above a declaration, XML tags removed.
fn leading_slash_comments(source: &str, anchor_byte: usize) -> Option<String> {
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
        let text = line
            .strip_prefix("///")
            .or_else(|| line.strip_prefix("//"))
            .map(strip_xml_tags)
            .unwrap_or_default();
        if text.is_empty() {
            break;
        }
        block.push(text);
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

/// Remove `<tag>` sequences so XML doc comments read as plain sentences.
fn strip_xml_tags(text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for ch in text.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Exact source text of a node, degrading to an empty string for invalid UTF-8.
fn node_text(node: &Node<'_>, bytes: &[u8]) -> String {
    node.utf8_text(bytes).unwrap_or("").to_owned()
}
