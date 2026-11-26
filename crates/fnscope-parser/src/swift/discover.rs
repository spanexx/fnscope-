//! Deterministic discovery of Swift callables and object-shaped types.
//!
//! The cached `tree-sitter-swift` 0.7.4 grammar has no `struct_declaration` or
//! `extension_declaration` node: `class`, `struct`, `actor`, `extension` and
//! `enum` all share one `class_declaration` kind, separated by its
//! `declaration_kind` field. Only direct function and initializer children of
//! that type body are callables; functions nested in a body are local and are
//! deliberately skipped.

use std::collections::{BTreeMap, HashMap};

use fnscope_model::{Parameter, ReturnField, ReturnShape};
use serde_json::Value;
use tree_sitter::{Node, Parser, Tree};

use crate::resolve::resolve_fields_from_map;
use crate::types::{DiscoveredFn, FnKind};

/// Parse one Swift source file, or `None` when the grammar rejects it.
fn parse_tree(source: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_swift::LANGUAGE.into();
    parser.set_language(&language).ok()?;
    parser.parse(source, None)
}

/// Discover every callable in one source file, sorted by `(line, qualified_name)`.
///
/// `relative_path` is accepted for diagnostics only; discovery never invents a
/// module path or fallback description from it.
pub fn discover(source: &str, relative_path: &str) -> Vec<DiscoveredFn> {
    let _ = relative_path;
    let tree = match parse_tree(source) {
        Some(tree) => tree,
        None => return Vec::new(),
    };
    let root = tree.root_node();
    let bytes = source.as_bytes();
    let types = TypeIndex::build(root, bytes).full_map();

    let mut found = Vec::new();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        visit_top_level(child, bytes, &types, &mut found);
    }

    found.sort_by(|a, b| (a.line, &a.qualified_name).cmp(&(b.line, &b.qualified_name)));
    found.dedup_by(|a, b| a.qualified_name == b.qualified_name && a.line == b.line);
    found
}

// ---------------------------------------------------------------------------
// Declaration visitors
// ---------------------------------------------------------------------------

/// Visit one source-file child, recording only items that can declare a callable.
fn visit_top_level(
    node: Node,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    match node.kind() {
        "function_declaration" => {
            record_fn(node, FnKind::Declaration, None, bytes, types, out);
        }
        "class_declaration" => visit_type(node, bytes, types, out),
        // Imports, protocols, globals and every other item are not callables.
        _ => {}
    }
}

/// Record direct methods and initializers of a `class`/`struct`/`actor`/`extension`.
fn visit_type(
    node: Node,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let kind = node
        .child_by_field_name("declaration_kind")
        .map(|n| node_text(&n, bytes))
        .unwrap_or_default();
    if !matches!(kind.as_str(), "class" | "struct" | "actor" | "extension") {
        return;
    }
    let type_name = match node.child_by_field_name("name") {
        Some(name) => node_text(&name, bytes),
        None => return,
    };
    if type_name.is_empty() {
        return;
    }
    let body = match node.child_by_field_name("body") {
        Some(body) => body,
        None => return,
    };
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        // Only direct type-body children are members; local functions nested in
        // a method body never reach this loop.
        match member.kind() {
            "function_declaration" => {
                record_fn(member, FnKind::Method, Some(&type_name), bytes, types, out);
            }
            "init_declaration" => {
                record_fn(member, FnKind::Method, Some(&type_name), bytes, types, out);
            }
            _ => {}
        }
    }
}

/// Build one `DiscoveredFn` from a Swift function or initializer declaration.
fn record_fn(
    node: Node,
    kind: FnKind,
    class_name: Option<&str>,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, bytes))
        .unwrap_or_default();
    if name.is_empty() {
        return;
    }

    let root = root_of(node);
    let doc = leading_swift_doc(root, node.start_byte(), bytes)
        .map(|raw| parse_doc(&raw))
        .unwrap_or_default();

    let start = node.start_position();
    let end = node.end_position();
    let qualified_name = match class_name {
        Some(class) => format!("{}.{}", class, name),
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
        parameters: extract_parameters(node, bytes, &doc),
        returns: extract_returns(node, bytes, types),
        doc_description: doc.description.clone(),
        tags: doc.tags.clone(),
        is_async: is_async(node, bytes),
        class_name: class_name.map(str::to_string),
    });
}

// ---------------------------------------------------------------------------
// Parameters and returns
// ---------------------------------------------------------------------------

/// Collect named parameters, their external labels, declared types and defaults.
///
/// Swift default values are `default_value` fields on the function node, not on
/// the `parameter` child, so each default is located by scanning the source gap
/// after its parameter and before the next parameter (or the body).
fn extract_parameters(node: Node, bytes: &[u8], doc: &DocInfo) -> Vec<Parameter> {
    let mut cursor = node.walk();
    let parameter_nodes: Vec<Node> = node
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "parameter")
        .collect();
    if parameter_nodes.is_empty() {
        return Vec::new();
    }

    let body_start = node
        .child_by_field_name("body")
        .map(|n| n.start_byte())
        .unwrap_or(node.end_byte());
    let return_start = node
        .child_by_field_name("return_type")
        .map(|n| n.start_byte())
        .unwrap_or(usize::MAX);

    let mut params = Vec::new();
    for (index, param) in parameter_nodes.iter().enumerate() {
        let name = param
            .child_by_field_name("name")
            .map(|n| node_text(&n, bytes))
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let external = param
            .child_by_field_name("external_name")
            .map(|n| node_text(&n, bytes))
            .unwrap_or_default();
        let type_ = param
            .child_by_field_name("type")
            .map(|n| strip_type_markers(&node_text(&n, bytes)))
            .unwrap_or_else(|| "unknown".to_string());

        let mut description = doc
            .param_descriptions
            .get(&name)
            .cloned()
            .unwrap_or_default();
        if !external.is_empty() && external != name {
            let note = format!("external label: {}", external);
            description = if description.is_empty() {
                note
            } else {
                format!("{}; {}", description, note)
            };
        }

        // A last parameter's gap still contains the `)` closing the parameter
        // list; an earlier parameter's gap ends at the next parameter instead.
        let (boundary, closes_parameter_list) = if index + 1 < parameter_nodes.len() {
            (parameter_nodes[index + 1].start_byte(), false)
        } else {
            (body_start.min(return_start).min(node.end_byte()), true)
        };
        let default = default_after(param.end_byte(), boundary, closes_parameter_list, bytes);
        params.push(Parameter {
            name,
            type_,
            required: default.is_none(),
            default_value: default,
            description,
        });
    }
    params
}

/// Read a `= expression` default from the source gap following one parameter.
///
/// The gap for the final parameter includes the `)` that closes the parameter
/// list, so that delimiter is used as the value boundary. Gaps ending at the
/// next parameter never contain that delimiter.
fn default_after(
    after: usize,
    boundary: usize,
    closes_parameter_list: bool,
    bytes: &[u8],
) -> Option<Value> {
    if boundary <= after {
        return None;
    }
    let segment = &bytes[after..boundary];
    let equals = segment.iter().position(|byte| *byte == b'=')?;
    let value_start = after + equals + 1;
    let value_end = if closes_parameter_list {
        // The last `)` before the body/return type is the list delimiter, even
        // when the literal itself contains parentheses or quoted `)` characters.
        segment
            .iter()
            .rposition(|byte| *byte == b')')
            .map(|index| after + index)
            .filter(|index| *index >= value_start)
            .unwrap_or(boundary)
    } else {
        boundary
    };
    let raw = String::from_utf8_lossy(&bytes[value_start..value_end]);
    let text = raw.trim().trim_end_matches(',').trim();
    if text.is_empty() {
        return None;
    }
    Some(default_json(text))
}

/// Render a simple Swift literal default as a deterministic JSON value.
fn default_json(text: &str) -> Value {
    match text {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "nil" => return Value::Null,
        _ => {}
    }
    if let Ok(number) = text.parse::<i64>() {
        return Value::Number(number.into());
    }
    if let Ok(number) = text.parse::<f64>() {
        if let Some(number) = serde_json::Number::from_f64(number) {
            return Value::Number(number);
        }
    }
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        return Value::String(unescape_string(&text[1..text.len() - 1]));
    }
    Value::String(text.to_owned())
}

/// Decode the escapes a Swift string literal may carry into a JSON string.
fn unescape_string(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Keep the declared Swift type readable, dropping only `inout` modifiers.
fn strip_type_markers(text: &str) -> String {
    let mut rest = text.trim();
    if let Some(next) = rest.strip_prefix("inout ") {
        rest = next.trim_start();
    }
    rest.to_owned()
}

/// Read the declared return type (after `->`) and resolve its known fields.
fn extract_returns(node: Node, bytes: &[u8], types: &BTreeMap<String, ReturnShape>) -> ReturnShape {
    let type_ = node
        .child_by_field_name("return_type")
        .map(|n| strip_arrow(&node_text(&n, bytes)))
        .unwrap_or_default();
    let fields = if type_.is_empty() {
        Vec::new()
    } else {
        resolve_fields_from_map(&type_, types)
    };
    ReturnShape { type_, fields }
}

/// Remove a leading `->` and surrounding whitespace from a return-type slice.
fn strip_arrow(text: &str) -> String {
    text.trim()
        .trim_start_matches("->")
        .trim()
        .trim_start_matches("inout ")
        .trim()
        .to_owned()
}

/// True when `async` appears in the header rather than in the function body.
fn is_async(node: Node, bytes: &[u8]) -> bool {
    let body_start = node
        .child_by_field_name("body")
        .map(|n| n.start_byte())
        .unwrap_or(node.end_byte());
    let header =
        String::from_utf8_lossy(&bytes[node.start_byte()..body_start]).to_ascii_lowercase();
    header.split_whitespace().any(|word| word == "async")
}

// ---------------------------------------------------------------------------
// Type index
// ---------------------------------------------------------------------------

/// Index structs (and classes/actors) and type aliases for return fields.
pub fn collect_types(source: &str) -> BTreeMap<String, ReturnShape> {
    let tree = match parse_tree(source) {
        Some(tree) => tree,
        None => return BTreeMap::new(),
    };
    TypeIndex::build(tree.root_node(), source.as_bytes()).named_map()
}

/// Index of Swift named types used for return fields.
struct TypeIndex {
    /// Stored properties of named types, in source order.
    named: HashMap<String, Vec<ReturnField>>,
    /// Type aliases keyed by alias name, with the aliased type text as value.
    aliases: HashMap<String, String>,
}

impl TypeIndex {
    /// Walk the whole tree once so a type declared after its use is resolvable.
    fn build(root: Node, bytes: &[u8]) -> Self {
        let mut index = Self {
            named: HashMap::new(),
            aliases: HashMap::new(),
        };
        index.walk(root, bytes);
        index
    }

    /// Recursive half of [`TypeIndex::build`].
    fn walk(&mut self, node: Node, bytes: &[u8]) {
        match node.kind() {
            "class_declaration" => {
                let kind = node
                    .child_by_field_name("declaration_kind")
                    .map(|n| node_text(&n, bytes))
                    .unwrap_or_default();
                if matches!(kind.as_str(), "class" | "struct" | "actor") {
                    if let (Some(name), Some(body)) = (
                        node.child_by_field_name("name"),
                        node.child_by_field_name("body"),
                    ) {
                        let fields = stored_properties(body, bytes);
                        self.named.insert(node_text(&name, bytes), fields);
                    }
                }
            }
            "typealias_declaration" => {
                if let (Some(name), Some(value)) = (
                    node.child_by_field_name("name"),
                    node.child_by_field_name("value"),
                ) {
                    self.aliases
                        .insert(node_text(&name, bytes), node_text(&value, bytes));
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child, bytes);
        }
    }

    /// Named types first, then aliases whose fields copy their target's fields.
    fn named_map(&self) -> BTreeMap<String, ReturnShape> {
        let mut map = BTreeMap::new();
        for (name, fields) in &self.named {
            if !fields.is_empty() {
                map.insert(
                    name.clone(),
                    ReturnShape {
                        type_: name.clone(),
                        fields: fields.clone(),
                    },
                );
            }
        }
        for (alias, target) in &self.aliases {
            let fields = resolve_fields_from_map(target, &map);
            if !fields.is_empty() {
                map.insert(
                    alias.clone(),
                    ReturnShape {
                        type_: alias.clone(),
                        fields,
                    },
                );
            }
        }
        map
    }

    /// The type index for discovery equals the named map: Swift has no inline
    /// object literal that could contribute anonymous shapes.
    fn full_map(&self) -> BTreeMap<String, ReturnShape> {
        self.named_map()
    }
}

/// Stored properties (not computed properties) of a type body, in source order.
fn stored_properties(body: Node, bytes: &[u8]) -> Vec<ReturnField> {
    let mut fields = Vec::new();
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        if member.kind() != "property_declaration" {
            continue;
        }
        if member.child_by_field_name("computed_value").is_some() {
            continue;
        }
        let names = match member.child_by_field_name("name") {
            Some(name) => node_text(&name, bytes),
            None => continue,
        };
        let annotated = {
            let mut inner = member.walk();
            let found = member
                .named_children(&mut inner)
                .find(|child| child.kind() == "type_annotation")
                .and_then(|annotation| {
                    annotation
                        .child_by_field_name("type")
                        .map(|type_node| strip_type_markers(&node_text(&type_node, bytes)))
                });
            found
        };
        let type_ = annotated.unwrap_or_else(|| "unknown".to_owned());
        for name in names
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            fields.push(ReturnField {
                name: name.to_owned(),
                type_: type_.clone(),
            });
        }
    }
    fields
}

// ---------------------------------------------------------------------------
// Documentation and small helpers
// ---------------------------------------------------------------------------

/// Documentation facts read from the nearest preceding `//`/`///` comment block.
#[derive(Default)]
struct DocInfo {
    description: Option<String>,
    param_descriptions: HashMap<String, String>,
    tags: Vec<String>,
}

/// Parse comment text into a description, parameter texts and tags.
fn parse_doc(raw: &str) -> DocInfo {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let mut line = line.trim();
        for prefix in ["///", "//!", "/*", "*/", "//", "*"] {
            if let Some(rest) = line.strip_prefix(prefix) {
                line = rest.trim();
            }
        }
        line = line.trim_end_matches("*/").trim();
        if !line.is_empty() {
            lines.push(line.to_owned());
        }
    }

    let mut doc = DocInfo::default();
    let mut description_lines = Vec::new();
    for line in lines {
        if let Some(rest) = line.strip_prefix("- Parameter ") {
            if let Some((name, text)) = rest.split_once(':') {
                doc.param_descriptions
                    .insert(name.trim().to_owned(), text.trim().to_owned());
                continue;
            }
        }
        if let Some(rest) = line.strip_prefix("@param") {
            let mut words = rest.split_whitespace();
            let name = words.next().unwrap_or_default().to_owned();
            if !name.is_empty() {
                doc.param_descriptions
                    .insert(name, words.collect::<Vec<_>>().join(" "));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("@tags") {
            for tag in rest.split(',') {
                let tag = tag.trim();
                if !tag.is_empty() {
                    doc.tags.push(tag.to_owned());
                }
            }
            continue;
        }
        if !line.starts_with('@') {
            description_lines.push(line);
        }
    }
    if !description_lines.is_empty() {
        doc.description = Some(description_lines.join(" "));
    }
    doc
}

/// Combine the contiguous comment nodes immediately above a declaration.
fn leading_swift_doc(root: Node, declaration_start: usize, bytes: &[u8]) -> Option<String> {
    let mut comments = Vec::new();
    collect_comments(root, declaration_start, bytes, &mut comments);
    comments.sort_by_key(|node| node.start_byte());

    let mut chosen = Vec::new();
    let mut cursor_end = declaration_start;
    for comment in comments.into_iter().rev() {
        if comment.end_byte() > cursor_end {
            continue;
        }
        if bytes[comment.end_byte()..cursor_end]
            .iter()
            .any(|byte| !byte.is_ascii_whitespace())
        {
            break;
        }
        cursor_end = comment.start_byte();
        chosen.push(comment);
    }
    if chosen.is_empty() {
        return None;
    }
    chosen.reverse();
    let mut raw = String::new();
    for comment in chosen {
        raw.push_str(&node_text(&comment, bytes));
        raw.push('\n');
    }
    Some(raw)
}

/// Recursive half of [`leading_swift_doc`].
fn collect_comments<'tree>(
    node: Node<'tree>,
    limit: usize,
    bytes: &[u8],
    out: &mut Vec<Node<'tree>>,
) {
    if matches!(node.kind(), "comment" | "multiline_comment") && node.end_byte() <= limit {
        out.push(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_comments(child, limit, bytes, out);
    }
}

/// Walk up through wrappers to the tree root.
fn root_of(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

/// Exact source text of a node, tolerating a slice that is not valid UTF-8.
fn node_text(node: &Node, bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[node.start_byte()..node.end_byte()]).into_owned()
}
