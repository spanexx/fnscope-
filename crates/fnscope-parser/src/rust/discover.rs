//! Deterministic discovery of module-scoped functions and class methods.
//!
//! The pass walks only the shapes that can legally be a callable export or class member. It
//! never descends into a function body, so a nested helper is never mistaken for a top-level
//! function. Everything is derived from source text and the tree-sitter AST — no heuristics
//! that vary between runs.

use fnscope_model::{Parameter, ReturnField, ReturnShape};
use std::collections::{BTreeMap, HashMap};
use tree_sitter::{Node, Parser, Tree};

use crate::resolve::resolve_fields_from_map;
use crate::types::{DiscoveredFn, FnKind};

/// Parse one Rust source file, or `None` on failure.
fn parse_tree(source: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    parser.set_language(&language).ok()?;
    parser.parse(source, None)
}

/// Discover every callable in one source file, sorted by `(line, qualified_name)`.
///
/// `relative_path` is intentionally not used to invent a module path or description; the
/// discovery pass reports only facts it can read from the AST.
pub fn discover(source: &str, relative_path: &str) -> Vec<DiscoveredFn> {
    let _ = relative_path; // discovery never fabricates a module path or fallback description

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
        visit_rust_top_level(child, bytes, &types, &mut found);
    }

    found.sort_by(|a, b| (a.line, &a.qualified_name).cmp(&(b.line, &b.qualified_name)));
    found.dedup_by(|a, b| a.qualified_name == b.qualified_name && a.line == b.line);
    found
}

// ---------------------------------------------------------------------------
// Rust items
// ---------------------------------------------------------------------------

/// Visit one Rust module item, recording only items that can declare a callable.
fn visit_rust_top_level(
    node: Node,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    match node.kind() {
        "function_item" | "function_signature_item" => {
            record_rust_fn(node, FnKind::Declaration, None, bytes, types, out);
        }
        "impl_item" => visit_rust_impl(node, bytes, types, out),
        "trait_item" => {
            // Trait methods are signatures; only the declarations directly in the trait body count.
            let body = match node.child_by_field_name("body") {
                Some(body) => body,
                None => return,
            };
            let mut cursor = body.walk();
            for member in body.named_children(&mut cursor) {
                if member.kind() == "function_signature_item" {
                    record_rust_fn(member, FnKind::Declaration, None, bytes, types, out);
                }
            }
        }
        // Structs, enums, modules and every other item are not callables.
        _ => {}
    }
}

/// Record inherent/trait impl methods with `TypeName.method` as their qualified name.
fn visit_rust_impl(
    node: Node,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let raw_type = node
        .child_by_field_name("type")
        .map(|n| node_text(&n, bytes))
        .unwrap_or_default();
    let type_name = strip_rust_generics(&raw_type);
    if type_name.is_empty() {
        return;
    }
    let body = match node.child_by_field_name("body") {
        Some(body) => body,
        None => return,
    };
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        // Only direct impl children are methods — functions nested in a body are skipped.
        if member.kind() == "function_item" {
            record_rust_fn(member, FnKind::Method, Some(&type_name), bytes, types, out);
        }
    }
}

/// Build one `DiscoveredFn` from a Rust function item or signature item.
fn record_rust_fn(
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
    let doc_start = statement_start(node);
    let root = root_of(node);
    let doc = leading_rust_doc(root, doc_start, bytes)
        .or_else(|| leading_comment(root, doc_start, bytes))
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
        parameters: extract_rust_parameters(node, bytes, &doc),
        returns: extract_returns(node, bytes, types),
        doc_description: doc.description.clone(),
        tags: doc.tags.clone(),
        is_async: is_async(node, bytes),
        class_name: class_name.map(str::to_string),
    });
}

/// Collect Rust parameters, ignoring `self`/`&self`/`&mut self` receiver parameters.
fn extract_rust_parameters(node: Node, bytes: &[u8], doc: &DocInfo) -> Vec<Parameter> {
    let mut params = Vec::new();
    let list = match node.child_by_field_name("parameters") {
        Some(list) => list,
        None => return params,
    };
    let mut cursor = list.walk();
    for param in list.named_children(&mut cursor) {
        if param.kind() != "parameter" {
            continue; // self_parameter, variadic_parameter and attributes are not named params
        }
        let pattern = match param.child_by_field_name("pattern") {
            Some(pattern) => pattern,
            None => continue,
        };
        let name = node_text(&pattern, bytes);
        if name.is_empty() || name == "self" || name.starts_with('&') || name.starts_with("mut ") {
            continue;
        }
        let type_ = param
            .child_by_field_name("type")
            .map(|n| strip_rust_type(&node_text(&n, bytes)))
            .unwrap_or_else(|| "unknown".to_string());
        let description = doc
            .param_descriptions
            .get(&name)
            .cloned()
            .unwrap_or_default();
        // Rust has no optional parameters, so every named parameter is required.
        params.push(Parameter {
            name,
            type_,
            required: true,
            default_value: None,
            description,
        });
    }
    params
}

/// Strip leading references, lifetimes and `mut` so the declared type stays readable.
fn strip_rust_type(text: &str) -> String {
    let mut rest = text.trim();
    loop {
        if let Some(next) = rest.strip_prefix('&') {
            rest = next.trim_start();
            continue;
        }
        if let Some(next) = rest.strip_prefix("mut ") {
            rest = next.trim_start();
            continue;
        }
        if rest.starts_with('\'') {
            match rest.find(char::is_whitespace) {
                Some(index) => {
                    rest = rest[index..].trim_start();
                    continue;
                }
                None => return String::new(), // a bare lifetime is not a value type
            }
        }
        break;
    }
    rest.to_string()
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

// ---------------------------------------------------------------------------
// Returns and type members
// ---------------------------------------------------------------------------

/// Index the structs and enums declared in one Rust source file.
///
/// The returned map is keyed by type name and can be shared across files to
/// resolve return fields for types declared elsewhere.
pub fn collect_types(source: &str) -> BTreeMap<String, ReturnShape> {
    let tree = match parse_tree(source) {
        Some(tree) => tree,
        None => return BTreeMap::new(),
    };
    TypeIndex::build(tree.root_node(), source.as_bytes()).named_map()
}

/// Read the declared return type and, when it names a known type, its member fields.
fn extract_returns(node: Node, bytes: &[u8], types: &BTreeMap<String, ReturnShape>) -> ReturnShape {
    let type_ = node
        .child_by_field_name("return_type")
        .map(|n| strip_colon(&node_text(&n, bytes)))
        .unwrap_or_default();
    let fields = if type_.is_empty() {
        vec![]
    } else {
        resolve_fields_from_map(&type_, types)
    };
    ReturnShape { type_, fields }
}

/// Index of Rust structs and enums used for return fields.
struct TypeIndex {
    named: HashMap<String, Vec<ReturnField>>,
}

impl TypeIndex {
    /// Walk the whole tree once so a type declared after its use is still resolvable.
    fn build(root: Node, bytes: &[u8]) -> Self {
        let mut index = Self {
            named: HashMap::new(),
        };
        index.walk(root, bytes);
        index
    }

    fn walk(&mut self, node: Node, bytes: &[u8]) {
        match node.kind() {
            "struct_item" => {
                if let (Some(name), Some(body)) = (
                    node.child_by_field_name("name"),
                    node.child_by_field_name("body"),
                ) {
                    let fields = self.fields_from_rust_fields(body, bytes);
                    self.named.insert(node_text(&name, bytes), fields);
                }
            }
            "enum_item" => {
                if let (Some(name), Some(body)) = (
                    node.child_by_field_name("name"),
                    node.child_by_field_name("body"),
                ) {
                    let fields = self.fields_from_rust_enum(body, bytes);
                    self.named.insert(node_text(&name, bytes), fields);
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child, bytes);
        }
    }

    /// Named fields of a Rust struct, in source order.
    fn fields_from_rust_fields(&self, node: Node, bytes: &[u8]) -> Vec<ReturnField> {
        let mut fields = Vec::new();
        let mut cursor = node.walk();
        for member in node.named_children(&mut cursor) {
            if member.kind() != "field_declaration" {
                continue;
            }
            if let (Some(name), Some(type_node)) = (
                member.child_by_field_name("name"),
                member.child_by_field_name("type"),
            ) {
                fields.push(ReturnField {
                    name: node_text(&name, bytes),
                    type_: strip_rust_type(&node_text(&type_node, bytes)),
                });
            }
        }
        fields
    }

    /// Enum variants become fields so callers can inspect `Result<T, E>` payloads.
    fn fields_from_rust_enum(&self, node: Node, bytes: &[u8]) -> Vec<ReturnField> {
        let mut fields = Vec::new();
        let mut cursor = node.walk();
        for member in node.named_children(&mut cursor) {
            if member.kind() != "enum_variant" {
                continue;
            }
            let name = match member.child_by_field_name("name") {
                Some(name) => node_text(&name, bytes),
                None => continue,
            };
            let type_ = match member.child_by_field_name("body") {
                Some(body) => {
                    let inner = self.fields_from_rust_fields(body, bytes);
                    if inner.is_empty() {
                        "()".to_string()
                    } else {
                        format!(
                            "({})",
                            inner
                                .iter()
                                .map(|field| field.type_.clone())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                }
                None => "()".to_string(),
            };
            fields.push(ReturnField { name, type_ });
        }
        fields
    }

    /// Structs and enums keyed by their declared name.
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
        map
    }

    /// Rust has no inline object types, so the full map equals the named map.
    fn full_map(&self) -> BTreeMap<String, ReturnShape> {
        self.named_map()
    }
}

// ---------------------------------------------------------------------------
// Documentation and small helpers
// ---------------------------------------------------------------------------

/// Documentation facts read from the nearest preceding comment.
#[derive(Default)]
struct DocInfo {
    description: Option<String>,
    param_descriptions: HashMap<String, String>,
    tags: Vec<String>,
}

/// Parse JSDoc/line-comment text into a description, `@param` texts and `@tags` values.
fn parse_doc(raw: &str) -> DocInfo {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let mut line = line.trim();
        for prefix in ["/**", "///", "//!", "/*", "*/", "//", "*"] {
            if let Some(rest) = line.strip_prefix(prefix) {
                line = rest.trim();
            }
        }
        line = line.trim_end_matches("*/").trim();
        if !line.is_empty() {
            lines.push(line.to_string());
        }
    }

    let mut doc = DocInfo::default();
    let mut description_lines = Vec::new();
    for line in lines {
        if let Some(rest) = line.strip_prefix("@param") {
            let mut words = rest.split_whitespace();
            let mut name = words.next().unwrap_or_default().to_string();
            if name.starts_with('{') {
                name = words.next().unwrap_or_default().to_string();
            }
            let description = words.collect::<Vec<_>>().join(" ");
            if !name.is_empty() {
                doc.param_descriptions.insert(name, description);
            }
        } else if let Some(rest) = line.strip_prefix("@tags") {
            for tag in rest.split(',') {
                let tag = tag.trim();
                if !tag.is_empty() {
                    doc.tags.push(tag.to_string());
                }
            }
        } else if !line.starts_with('@') {
            description_lines.push(line);
        }
    }
    if !description_lines.is_empty() {
        doc.description = Some(description_lines.join(" "));
    }
    doc
}

/// Combine the contiguous `///` doc lines immediately above a Rust declaration.
fn leading_rust_doc(root: Node, declaration_start: usize, bytes: &[u8]) -> Option<String> {
    let mut comments = Vec::new();
    collect_rust_doc_comments(root, declaration_start, bytes, &mut comments);
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

/// Recursive half of [`leading_rust_doc`]; only doc comments qualify.
fn collect_rust_doc_comments<'a>(
    node: Node<'a>,
    limit: usize,
    bytes: &[u8],
    out: &mut Vec<Node<'a>>,
) {
    if matches!(node.kind(), "line_comment" | "block_comment")
        && node.end_byte() <= limit
        && (node_text(&node, bytes).trim_start().starts_with("///")
            || node.child_by_field_name("doc").is_some())
    {
        out.push(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_rust_doc_comments(child, limit, bytes, out);
    }
}

/// Find the closest comment that ends immediately before `declaration_start`.
fn leading_comment(root: Node, declaration_start: usize, bytes: &[u8]) -> Option<String> {
    let mut best: Option<Node> = None;
    collect_comments(root, declaration_start, &mut best);
    let comment = best?;
    let between = &bytes[comment.end_byte()..declaration_start];
    if between.iter().any(|b| !b.is_ascii_whitespace()) {
        return None;
    }
    Some(node_text(&comment, bytes))
}

fn collect_comments<'a>(node: Node<'a>, limit: usize, best: &mut Option<Node<'a>>) {
    if matches!(node.kind(), "comment" | "line_comment" | "block_comment")
        && node.end_byte() <= limit
        && best
            .as_ref()
            .map_or(true, |current| node.end_byte() > current.end_byte())
    {
        *best = Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_comments(child, limit, best);
    }
}

/// Walk up through wrappers so a documented declaration uses its statement's start byte.
fn statement_start(node: Node) -> usize {
    let mut current = node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "export_statement"
            | "lexical_declaration"
            | "variable_declaration"
            | "variable_declarator"
            | "public_field_definition" => current = parent,
            _ => break,
        }
    }
    current.start_byte()
}

fn root_of(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

/// True when `async` appears in the function header rather than in its body.
fn is_async(node: Node, bytes: &[u8]) -> bool {
    let body = node.child_by_field_name("body").map(|body| body.id());
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).any(|child| {
        Some(child.id()) != body
            && (child.kind() == "async" || node_text(&child, bytes).trim() == "async")
    });
    found
}

/// Exact source text of a node, tolerating a slice that is not valid UTF-8.
fn node_text(node: &Node, bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[node.start_byte()..node.end_byte()]).into_owned()
}

/// Remove a leading colon and surrounding whitespace from a type-annotation slice.
fn strip_colon(text: &str) -> String {
    text.trim().trim_start_matches(':').trim().to_string()
}
