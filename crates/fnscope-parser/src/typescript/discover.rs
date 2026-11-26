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
use crate::types::{DiscoveredFn, FnKind, Lang};

/// Parse one source file with the grammar selected by `lang`, or `None` on failure.
fn parse_tree(lang: Lang, source: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = match lang {
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Lang::JavaScript | Lang::Jsx => tree_sitter_javascript::LANGUAGE.into(),
        _ => return None,
    };
    parser.set_language(&language).ok()?;
    parser.parse(source, None)
}

/// Discover every callable in one source file, sorted by `(line, qualified_name)`.
///
/// `relative_path` is intentionally not used to invent a module path or description; the
/// discovery pass reports only facts it can read from the AST.
pub fn discover(lang: Lang, source: &str, relative_path: &str) -> Vec<DiscoveredFn> {
    let _ = relative_path; // discovery never fabricates a module path or fallback description

    let tree = match parse_tree(lang, source) {
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
// AST walking
// ---------------------------------------------------------------------------

/// Visit one program-level statement, handling only declaration shapes that can be discovered.
fn visit_top_level(
    node: Node,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let node = if node.kind() == "export_statement" {
        match node.child_by_field_name("declaration") {
            Some(decl) => decl,
            None => return, // e.g. `export { foo }` — no new callable here
        }
    } else {
        node
    };

    match node.kind() {
        "function_declaration" | "generator_function_declaration" => {
            let name = node
                .child_by_field_name("name")
                .map(|n| node_text(&n, bytes))
                .unwrap_or_default();
            if !name.is_empty() {
                record(node, &name, FnKind::Declaration, None, bytes, types, out);
            }
        }
        "class_declaration" | "abstract_class_declaration" => visit_class(node, bytes, types, out),
        "lexical_declaration" | "variable_declaration" => {
            let mut cursor = node.walk();
            for decl in node.named_children(&mut cursor) {
                if decl.kind() != "variable_declarator" {
                    continue;
                }
                let name = decl
                    .child_by_field_name("name")
                    .map(|n| node_text(&n, bytes))
                    .unwrap_or_default();
                let value = match decl.child_by_field_name("value") {
                    Some(value) => value,
                    None => continue,
                };
                let kind = match value.kind() {
                    "arrow_function" | "function_expression" => Some(FnKind::Arrow),
                    _ => None,
                };
                if let (Some(kind), true) = (kind, !name.is_empty()) {
                    record(value, &name, kind, None, bytes, types, out);
                }
            }
        }
        // Any other statement (if/for/expression statements, nested blocks, etc.) is ignored on
        // purpose: a function declared there is not a module-scoped callable.
        _ => {}
    }
}

/// Record the methods and arrow-function fields declared directly inside a class body.
fn visit_class(
    node: Node,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let class_name = node
        .child_by_field_name("name")
        .map(|n| node_text(&n, bytes))
        .unwrap_or_default();
    let body = match node.child_by_field_name("body") {
        Some(body) => body,
        None => return,
    };

    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        match member.kind() {
            "method_definition" => {
                let name = member
                    .child_by_field_name("name")
                    .map(|n| node_text(&n, bytes))
                    .unwrap_or_default();
                if !name.is_empty() {
                    record(
                        member,
                        &name,
                        FnKind::Method,
                        Some(&class_name),
                        bytes,
                        types,
                        out,
                    );
                }
            }
            "public_field_definition" => {
                let value = match member.child_by_field_name("value") {
                    Some(value) if value.kind() == "arrow_function" => value,
                    _ => continue,
                };
                let name = member
                    .child_by_field_name("name")
                    .map(|n| node_text(&n, bytes))
                    .unwrap_or_default();
                if !name.is_empty() {
                    record(
                        value,
                        &name,
                        FnKind::Method,
                        Some(&class_name),
                        bytes,
                        types,
                        out,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Build one `DiscoveredFn` from a function node and push it into `out`.
fn record(
    node: Node,
    bare_name: &str,
    kind: FnKind,
    class_name: Option<&str>,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    // The comment that documents a declaration sits before its whole statement (`export const
    // f = ...`), not before the bare function node, so walk up to the outermost wrapper.
    let doc_start = statement_start(node);
    let root = root_of(node);
    let doc = leading_comment(root, doc_start, bytes)
        .map(|raw| parse_doc(&raw))
        .unwrap_or_default();

    let start = node.start_position();
    let end = node.end_position();
    let qualified_name = match class_name {
        Some(class) => format!("{}.{}", class, bare_name),
        None => bare_name.to_string(),
    };

    out.push(DiscoveredFn {
        name: bare_name.to_string(),
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
// Parameters
// ---------------------------------------------------------------------------

/// Collect parameters from every parameter-list shape the grammar can produce.
fn extract_parameters(node: Node, bytes: &[u8], doc: &DocInfo) -> Vec<Parameter> {
    let mut params = Vec::new();
    if let Some(list) = node.child_by_field_name("parameters") {
        let mut cursor = list.walk();
        for param in list.named_children(&mut cursor) {
            if let Some(parameter) = parse_parameter(param, bytes, doc) {
                params.push(parameter);
            }
        }
    }
    // A single unparenthesized arrow parameter (`x => x + 1`) is stored in the `parameter`
    // field instead of a `formal_parameters` list.
    if let Some(param) = node.child_by_field_name("parameter") {
        if let Some(parameter) = parse_parameter(param, bytes, doc) {
            params.push(parameter);
        }
    }
    params
}

/// Source of the parameter's own name segment, before any type annotation or initializer.
fn parameter_head<'a>(node: Node, bytes: &'a [u8]) -> &'a [u8] {
    let end = node
        .child_by_field_name("type")
        .or_else(|| node.child_by_field_name("value"))
        .map(|part| part.start_byte())
        .unwrap_or_else(|| node.end_byte())
        .min(node.end_byte());
    &bytes[node.start_byte()..end]
}

/// Extract one parameter's name, type, required flag, default and JSDoc description.
fn parse_parameter(node: Node, bytes: &[u8], doc: &DocInfo) -> Option<Parameter> {
    let name_node = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("pattern"))
        .or_else(|| match node.kind() {
            "identifier"
            | "object_pattern"
            | "array_pattern"
            | "rest_pattern"
            | "shorthand_property_identifier_pattern"
            | "assignment_pattern" => Some(node),
            _ => None,
        })?;
    let name = node_text(&name_node, bytes);
    if name.is_empty() {
        return None;
    }

    let value_node = node.child_by_field_name("value");
    // Only the parameter's own `?` makes it optional. A `?` inside an object type annotation
    // (or an initializer expression) belongs to something else entirely.
    let optional_marker =
        node.kind() == "optional_parameter" || parameter_head(node, bytes).contains(&b'?');
    let required = !optional_marker && value_node.is_none();
    let type_ = match node.child_by_field_name("type") {
        Some(annotation) => strip_colon(&node_text(&annotation, bytes)),
        None => match value_node {
            Some(value) => infer_literal_type(value, bytes).to_string(),
            None => "unknown".to_string(),
        },
    };
    let default_value = match value_node {
        Some(value) => literal_json(value, bytes),
        None => None,
    };
    let description = doc
        .param_descriptions
        .get(&name)
        .cloned()
        .unwrap_or_default();

    Some(Parameter {
        name,
        type_,
        required,
        default_value,
        description,
    })
}

/// Infer a coarse type from an initializer literal when no annotation is present.
fn infer_literal_type(node: Node, bytes: &[u8]) -> &'static str {
    let text = node_text(&node, bytes);
    let text = text.trim();
    if text.starts_with('"') || text.starts_with('\'') || text.starts_with('`') {
        "string"
    } else if text == "true" || text == "false" {
        "boolean"
    } else if text.starts_with('[') {
        "unknown[]"
    } else if text.starts_with('{') {
        "object"
    } else if text.parse::<i64>().is_ok() || text.parse::<f64>().is_ok() {
        "number"
    } else {
        "unknown"
    }
}

/// Parse a literal initializer into the model's JSON value type.
fn literal_json(node: Node, bytes: &[u8]) -> Option<serde_json::Value> {
    let raw = node_text(&node, bytes);
    let text = raw.trim();

    let quoted = (text.starts_with('"') && text.ends_with('"') && text.len() >= 2)
        || (text.starts_with('\'') && text.ends_with('\'') && text.len() >= 2)
        || (text.starts_with('`') && text.ends_with('`') && text.len() >= 2);
    if quoted {
        return Some(serde_json::Value::String(unescape(
            &text[1..text.len() - 1],
        )));
    }
    match text {
        "true" => return Some(serde_json::Value::Bool(true)),
        "false" => return Some(serde_json::Value::Bool(false)),
        _ => {}
    }
    let cleaned: String = text.chars().filter(|c| *c != '_').collect();
    if let Ok(n) = cleaned.parse::<i64>() {
        return Some(serde_json::Value::from(n));
    }
    if let Ok(n) = cleaned.parse::<f64>() {
        return Some(serde_json::Value::from(n));
    }
    None
}

/// Decode the common escape sequences found in string literals.
fn unescape(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\'') => out.push('\''),
            Some('`') => out.push('`'),
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

// ---------------------------------------------------------------------------
// Returns and type members
// ---------------------------------------------------------------------------

/// Collects object-shaped type aliases and interfaces from one source file.
///
/// The returned map is keyed by type name and can be shared across files to resolve
/// return fields for types declared elsewhere through the adapter registry.
pub fn collect_types(lang: Lang, source: &str) -> BTreeMap<String, ReturnShape> {
    let tree = match parse_tree(lang, source) {
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

/// Index of named type aliases/interfaces and inline object types used for return fields.
struct TypeIndex {
    named: HashMap<String, Vec<ReturnField>>,
    objects: HashMap<String, Vec<ReturnField>>,
}

impl TypeIndex {
    /// Walk the whole tree once so a type declared after its use is still resolvable.
    fn build(root: Node, bytes: &[u8]) -> Self {
        let mut index = Self {
            named: HashMap::new(),
            objects: HashMap::new(),
        };
        index.walk(root, bytes);
        index
    }

    fn walk(&mut self, node: Node, bytes: &[u8]) {
        match node.kind() {
            "type_alias_declaration" => {
                if let (Some(name), Some(value)) = (
                    node.child_by_field_name("name"),
                    node.child_by_field_name("value"),
                ) {
                    let fields = self.fields_from_type(value, bytes);
                    self.named.insert(node_text(&name, bytes), fields);
                }
            }
            "interface_declaration" => {
                if let (Some(name), Some(body)) = (
                    node.child_by_field_name("name"),
                    node.child_by_field_name("body"),
                ) {
                    let fields = self.fields_from_container(body, bytes);
                    self.named.insert(node_text(&name, bytes), fields);
                }
            }
            "object_type" => {
                let fields = self.fields_from_container(node, bytes);
                self.objects.insert(node_text(&node, bytes), fields);
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child, bytes);
        }
    }

    fn fields_from_type(&self, node: Node, bytes: &[u8]) -> Vec<ReturnField> {
        if node.kind() == "object_type" {
            return self.fields_from_container(node, bytes);
        }
        let mut fields = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "object_type" {
                fields.extend(self.fields_from_container(child, bytes));
            }
        }
        fields
    }

    fn fields_from_container(&self, node: Node, bytes: &[u8]) -> Vec<ReturnField> {
        let mut fields = Vec::new();
        let mut cursor = node.walk();
        for member in node.named_children(&mut cursor) {
            if member.kind() == "property_signature" {
                if let (Some(name), Some(type_node)) = (
                    member.child_by_field_name("name"),
                    member.child_by_field_name("type"),
                ) {
                    fields.push(ReturnField {
                        name: node_text(&name, bytes),
                        type_: strip_colon(&node_text(&type_node, bytes)),
                    });
                }
            }
        }
        fields
    }

    /// Object-shaped aliases and interfaces keyed by their declared name.
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

    /// The named types plus inline object types keyed by their exact source text.
    fn full_map(&self) -> BTreeMap<String, ReturnShape> {
        let mut map = self.named_map();
        for (text, fields) in &self.objects {
            if !fields.is_empty() {
                map.insert(
                    text.clone(),
                    ReturnShape {
                        type_: text.clone(),
                        fields: fields.clone(),
                    },
                );
            }
        }
        map
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
