//! Deterministic discovery of Kotlin functions, class members and types.
//!
//! The `-ng` grammar uses named fields plus wrapper/supertype nodes; helpers
//! below combine both so only payload nodes are filtered on. Only declarations
//! directly under `source_file`, `class_body` or `companion_object` become
//! callables; a `function_declaration` inside a `function_body`/`block` wrapper
//! is a local function and is never visited.

use std::collections::{BTreeMap, HashMap};

use fnscope_model::{Parameter, ReturnField, ReturnShape};
use serde_json::Value;
use tree_sitter::{Node, Parser, Tree};

use crate::kotlin::kotlin_language;
use crate::resolve::resolve_fields_from_map;
use crate::types::{DiscoveredFn, FnKind};

/// Node kinds that carry a declared type annotation in the `-ng` grammar.
///
/// The `-ng` grammar introduces a visible `type` supertype around
/// `user_type`/`nullable_type`/`non_nullable_type` and renames
/// `not_nullable_type` to `non_nullable_type`.
const TYPE_KINDS: &[&str] = &[
    "type",
    "user_type",
    "nullable_type",
    "non_nullable_type",
    "parenthesized_type",
    "function_type",
];

/// Grammar-only wrapper and supertype nodes that hide the payload we filter on.
const WRAPPER_KINDS: &[&str] = &[
    "statement",
    "declaration",
    "class_member_declaration",
    "expression",
    "primary_expression",
    "index_expression",
    "annotated_expression",
];

/// Parse one Kotlin source file, or `None` when no tree can be produced.
fn parse_tree(source: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    let language = kotlin_language();
    parser.set_language(&language).ok()?;
    parser.parse(source, None)
}

/// Discover every callable in one Kotlin file, sorted by `(line, qualified_name)`.
///
/// `relative_path` is accepted for diagnostics only; discovery never invents a
/// package name or fallback description from it.
pub fn discover(source: &str, relative_path: &str) -> Vec<DiscoveredFn> {
    let _ = relative_path;
    let tree = match parse_tree(source) {
        Some(tree) => tree,
        None => return Vec::new(),
    };
    let root = tree.root_node();
    let bytes = source.as_bytes();
    let types = TypeIndex::build(root, bytes).named_map();

    let mut found = Vec::new();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        // `source_file` holds `statement` -> `declaration` -> the real declaration.
        let child = unwrap(child);
        match child.kind() {
            "function_declaration" => {
                record_fn(child, None, FnKind::Declaration, bytes, &types, &mut found);
            }
            "class_declaration" | "object_declaration" => {
                let class_name = declared_name(child, bytes);
                if class_name.is_empty() {
                    continue;
                }
                if child.kind() == "class_declaration" {
                    if let Some(primary) = first_kind(child, "primary_constructor") {
                        record_constructor(primary, &class_name, bytes, &types, &mut found);
                    }
                }
                if let Some(body) = first_kind(child, "class_body") {
                    visit_class_members(body, &class_name, bytes, &types, &mut found);
                    // Companion members belong to the enclosing class, not to a
                    // synthetic "Companion" type, so callers see `Class.method`.
                    let mut companion_cursor = body.walk();
                    for member in body.named_children(&mut companion_cursor) {
                        let member = unwrap(member);
                        if member.kind() == "companion_object" {
                            if let Some(companion_body) = first_kind(member, "class_body") {
                                visit_class_members(
                                    companion_body,
                                    &class_name,
                                    bytes,
                                    &types,
                                    &mut found,
                                );
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    found.sort_by(|a, b| (a.line, &a.qualified_name).cmp(&(b.line, &b.qualified_name)));
    found.dedup_by(|a, b| a.qualified_name == b.qualified_name && a.line == b.line);
    found
}

/// Record function and constructor declarations directly inside a class body.
fn visit_class_members(
    body: Node<'_>,
    class_name: &str,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        // `class_body` wraps each member in `class_member_declaration`.
        let member = unwrap(member);
        match member.kind() {
            "function_declaration" => {
                record_fn(member, Some(class_name), FnKind::Method, bytes, types, out);
            }
            "secondary_constructor" => {
                record_constructor(member, class_name, bytes, types, out);
            }
            "object_declaration" => {
                let nested_name = declared_name(member, bytes);
                if nested_name.is_empty() {
                    continue;
                }
                if let Some(nested_body) = first_kind(member, "class_body") {
                    visit_class_members(nested_body, &nested_name, bytes, types, out);
                }
            }
            _ => {}
        }
    }
}

/// Build one `DiscoveredFn` from a Kotlin `function_declaration`.
fn record_fn(
    node: Node<'_>,
    class_name: Option<&str>,
    kind: FnKind,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let name = declared_name(node, bytes);
    if name.is_empty() {
        return;
    }
    let doc = leading_kotlin_comment(root_of(node), node.start_byte(), bytes)
        .map(|raw| parse_kotlin_doc(&raw))
        .unwrap_or_default();
    let children = children_vec(node);
    let params_node = children
        .iter()
        .find(|child| child.kind() == "function_value_parameters");
    let parameters = params_node
        .map(|list| extract_parameters(*list, bytes, &doc))
        .unwrap_or_default();
    let returns = extract_returns(&children, bytes, types);

    let qualified_name = match class_name {
        Some(class) => format!("{class}.{name}"),
        None => name.clone(),
    };
    let start = node.start_position();
    let end = node.end_position();
    out.push(DiscoveredFn {
        name,
        qualified_name,
        kind,
        line: start.row + 1,
        end_line: end.row + 1,
        source_code: node_text(node, bytes),
        parameters,
        returns,
        doc_description: doc.description.clone(),
        tags: doc.tags.clone(),
        is_async: children.iter().any(|child| {
            child.kind() == "modifiers" && node_text(*child, bytes).contains("suspend")
        }),
        class_name: class_name.map(str::to_string),
    });
}

/// Build one `DiscoveredFn` for a primary or secondary constructor.
fn record_constructor(
    node: Node<'_>,
    class_name: &str,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let doc = leading_kotlin_comment(root_of(node), node.start_byte(), bytes)
        .map(|raw| parse_kotlin_doc(&raw))
        .unwrap_or_default();
    let children = children_vec(node);
    let parameters = if node.kind() == "primary_constructor" {
        // The `-ng` grammar wraps primary-constructor parameters in `class_parameters`.
        first_kind(node, "class_parameters")
            .map(|list| extract_parameters(list, bytes, &doc))
            .unwrap_or_default()
    } else {
        children
            .iter()
            .find(|child| child.kind() == "function_value_parameters")
            .map(|list| extract_parameters(*list, bytes, &doc))
            .unwrap_or_default()
    };
    let start = node.start_position();
    let end = node.end_position();
    out.push(DiscoveredFn {
        name: "<init>".to_owned(),
        qualified_name: format!("{class_name}.<init>"),
        kind: FnKind::Method,
        line: start.row + 1,
        end_line: end.row + 1,
        source_code: node_text(node, bytes),
        parameters,
        returns: ReturnShape {
            type_: class_name.to_owned(),
            fields: resolve_fields_from_map(class_name, types),
        },
        doc_description: doc.description.clone(),
        tags: doc.tags.clone(),
        is_async: false,
        class_name: Some(class_name.to_owned()),
    });
}

/// Extract parameters from `parameter` or `class_parameter` children.
///
/// This grammar places `=` and the default expression outside the `parameter`
/// node, and literals such as `null` are anonymous tokens. The text gap up to
/// the next parameter is therefore the only reliable place to read a default.
fn extract_parameters(list: Node<'_>, bytes: &[u8], doc: &DocInfo) -> Vec<Parameter> {
    let children = children_vec(list);
    let mut params = Vec::new();
    for (index, child) in children.iter().enumerate() {
        if child.kind() != "parameter" && child.kind() != "class_parameter" {
            continue;
        }
        let name = declared_name(*child, bytes);
        if name.is_empty() {
            continue;
        }
        let type_ = type_child_text(*child, bytes).unwrap_or_else(|| "unknown".to_owned());
        let end = children
            .get(index + 1)
            .filter(|next| next.kind() == "parameter" || next.kind() == "class_parameter")
            .map(|next| next.start_byte())
            .unwrap_or(list.end_byte());
        let gap = String::from_utf8_lossy(&bytes[child.end_byte()..end]);
        let default = gap
            .find('=')
            .map(|at| clean_default(&gap[at + 1..]))
            .filter(|text| !text.is_empty())
            .map(Value::String);
        params.push(Parameter {
            name: name.clone(),
            type_,
            required: default.is_none(),
            default_value: default,
            description: doc
                .param_descriptions
                .get(&name)
                .cloned()
                .unwrap_or_default(),
        });
    }
    params
}

/// Read the declared return type that follows the parameter list.
fn extract_returns(
    children: &[Node<'_>],
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
) -> ReturnShape {
    let params_end = children
        .iter()
        .find(|child| child.kind() == "function_value_parameters")
        .map(|child| child.end_byte())
        .unwrap_or_default();
    let type_text = children
        .iter()
        .find(|child| child.start_byte() >= params_end && is_type_kind(child.kind()))
        .map(|child| normalize_line(&node_text(*child, bytes)))
        .unwrap_or_default();
    if type_text.is_empty() {
        return ReturnShape::default();
    }
    let fields = resolve_fields_from_map(&bare_lookup(&type_text), types);
    ReturnShape {
        type_: type_text,
        fields,
    }
}

/// Index object-shaped types declared in `source` for cross-file return fields.
pub fn collect_types(source: &str) -> BTreeMap<String, ReturnShape> {
    let tree = match parse_tree(source) {
        Some(tree) => tree,
        None => return BTreeMap::new(),
    };
    TypeIndex::build(tree.root_node(), source.as_bytes()).named_map()
}

/// Type declarations of one file, keyed by their declared name.
struct TypeIndex {
    named: HashMap<String, Vec<ReturnField>>,
    aliases: HashMap<String, String>,
}

impl TypeIndex {
    /// Walk the whole tree once so a type declared after its use is still indexed.
    fn build(root: Node<'_>, bytes: &[u8]) -> Self {
        let mut index = Self {
            named: HashMap::new(),
            aliases: HashMap::new(),
        };
        index.walk(root, bytes);
        index
    }

    fn walk(&mut self, node: Node<'_>, bytes: &[u8]) {
        match node.kind() {
            "class_declaration" | "object_declaration" => self.record_object(node, bytes),
            "type_alias" => self.record_alias(node, bytes),
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.walk(child, bytes);
        }
    }

    /// Index properties and primary-constructor parameters of one object type.
    fn record_object(&mut self, node: Node<'_>, bytes: &[u8]) {
        let name = declared_name(node, bytes);
        if name.is_empty() {
            return;
        }
        let mut fields = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            collect_members(child, &mut fields, bytes);
        }
        if let Some(body) = first_kind(node, "class_body") {
            let mut body_cursor = body.walk();
            for member in body.named_children(&mut body_cursor) {
                collect_members(unwrap(member), &mut fields, bytes);
            }
        }
        if !fields.is_empty() {
            self.named.insert(name, fields);
        }
    }

    /// Record a `typealias Name = Target` for later field forwarding.
    fn record_alias(&mut self, node: Node<'_>, bytes: &[u8]) {
        // The alias name is the `type` field; the target is a `type` child.
        let name = node
            .child_by_field_name("type")
            .map(|name| normalize_line(&node_text(name, bytes)))
            .unwrap_or_default();
        if name.is_empty() {
            return;
        }
        let children = children_vec(node);
        let target = children
            .iter()
            .rev()
            .find(|child| is_type_kind(child.kind()))
            .map(|child| normalize_line(&node_text(*child, bytes)))
            .unwrap_or_default();
        if !target.is_empty() {
            self.aliases.insert(name, bare_lookup(&target));
        }
    }

    /// Return the contract shape keyed by type name, after resolving local aliases.
    fn named_map(&self) -> BTreeMap<String, ReturnShape> {
        let mut resolved = self.named.clone();
        for (alias, target) in &self.aliases {
            if let Some(fields) = resolved.get(target).cloned() {
                resolved.insert(alias.clone(), fields);
            }
        }
        let mut map = BTreeMap::new();
        for (name, fields) in resolved {
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
        map
    }
}

/// Collect fields from a constructor, a primary-constructor parameter or a property.
fn collect_members(node: Node<'_>, fields: &mut Vec<ReturnField>, bytes: &[u8]) {
    match node.kind() {
        "class_parameter" => {
            push_field(node, fields, bytes);
        }
        "primary_constructor" | "class_parameters" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                let child = unwrap(child);
                if child.kind() == "class_parameter" {
                    push_field(child, fields, bytes);
                } else if child.kind() == "class_parameters" {
                    collect_members(child, fields, bytes);
                }
            }
        }
        "property_declaration" => {
            if let Some(variable) = first_kind(node, "variable_declaration") {
                push_field(variable, fields, bytes);
            }
        }
        _ => {}
    }
}

/// Add one `identifier` and its declared type as a return field.
fn push_field(node: Node<'_>, fields: &mut Vec<ReturnField>, bytes: &[u8]) {
    let name = declared_name(node, bytes);
    if name.is_empty() {
        return;
    }
    let type_ = type_child_text(node, bytes).unwrap_or_else(|| "unknown".to_owned());
    fields.push(ReturnField { name, type_ });
}

// ---------------------------------------------------------------------------
// Documentation and small helpers
// ---------------------------------------------------------------------------

/// Documentation facts read from the nearest contiguous leading comment.
#[derive(Default)]
struct DocInfo {
    description: Option<String>,
    param_descriptions: HashMap<String, String>,
    tags: Vec<String>,
}

/// Parse a Kotlin comment block into a description, `@param` texts and `@tags` values.
fn parse_kotlin_doc(raw: &str) -> DocInfo {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let mut line = line.trim();
        for prefix in ["//", "/**", "*/", "*"] {
            if let Some(rest) = line.strip_prefix(prefix) {
                line = rest.trim();
            }
        }
        if !line.is_empty() {
            lines.push(line.to_owned());
        }
    }

    let mut doc = DocInfo::default();
    let mut description = Vec::new();
    for line in lines {
        if let Some(rest) = line.strip_prefix("@param") {
            let mut words = rest.split_whitespace();
            let name = words
                .next()
                .unwrap_or_default()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_owned();
            let detail = words.collect::<Vec<_>>().join(" ");
            if !name.is_empty() {
                doc.param_descriptions.insert(name, detail);
            }
        } else if let Some(rest) = line.strip_prefix("@tags") {
            for tag in rest.split(',') {
                let tag = tag.trim();
                if !tag.is_empty() {
                    doc.tags.push(tag.to_owned());
                }
            }
        } else if !line.starts_with('@') {
            description.push(line);
        }
    }
    if !description.is_empty() {
        doc.description = Some(description.join(" "));
    }
    doc
}

/// Combine the contiguous `//` or `/** */` comments above a declaration.
fn leading_kotlin_comment(
    root: Node<'_>,
    declaration_start: usize,
    bytes: &[u8],
) -> Option<String> {
    let mut comments = Vec::new();
    collect_comments(root, declaration_start, &mut comments);
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
        raw.push_str(&node_text(comment, bytes));
        raw.push('\n');
    }
    Some(raw)
}

/// Recursive half of [`leading_kotlin_comment`]; only comments above the limit qualify.
fn collect_comments<'tree>(node: Node<'tree>, limit: usize, out: &mut Vec<Node<'tree>>) {
    if matches!(node.kind(), "line_comment" | "block_comment") && node.end_byte() <= limit {
        out.push(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_comments(child, limit, out);
    }
}

/// Walk up through wrappers so a documented declaration uses its own start byte.
fn root_of<'tree>(mut node: Node<'tree>) -> Node<'tree> {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

/// Descend through `-ng` wrapper/supertype nodes to the payload they contain.
fn unwrap(node: Node<'_>) -> Node<'_> {
    let mut current = node;
    while WRAPPER_KINDS.contains(&current.kind()) {
        let child = children_vec(current)
            .into_iter()
            .find(|child| !matches!(child.kind(), "line_comment" | "block_comment"));
        match child {
            Some(child) => current = child,
            None => return current,
        }
    }
    current
}

/// Read a declaration name from its field, falling back to its first identifier child.
fn declared_name(node: Node<'_>, bytes: &[u8]) -> String {
    node.child_by_field_name("name")
        .or_else(|| node.child_by_field_name("type"))
        .map(|name| normalize_line(&node_text(name, bytes)))
        .filter(|name| !name.is_empty())
        .or_else(|| {
            let fallback = first_kind_text(node, "identifier", bytes);
            (!fallback.is_empty()).then_some(fallback)
        })
        .unwrap_or_default()
}

/// Named children of a node, collected so the walk cursor can be released.
fn children_vec(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    children
}

/// First named child of a given kind, if the node has one.
fn first_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let children = children_vec(node);
    let found = children.into_iter().find(|child| child.kind() == kind);
    found
}

/// Normalized text of the first named child of a given kind.
fn first_kind_text(node: Node<'_>, kind: &str, bytes: &[u8]) -> String {
    first_kind(node, kind)
        .map(|child| normalize_line(&node_text(child, bytes)))
        .unwrap_or_default()
}

/// Text of the first declared-type child of a node.
fn type_child_text(node: Node<'_>, bytes: &[u8]) -> Option<String> {
    let children = children_vec(node);
    let found = children
        .into_iter()
        .find(|child| is_type_kind(child.kind()))
        .map(|child| normalize_line(&node_text(child, bytes)));
    found.filter(|text| !text.is_empty())
}

/// Whether a node kind represents a declared type in this grammar.
fn is_type_kind(kind: &str) -> bool {
    TYPE_KINDS.contains(&kind)
}

/// Reduce a declared type to the bare name used as a key in the type index.
fn bare_lookup(text: &str) -> String {
    let mut name = text.trim();
    if let Some(index) = name.find('<') {
        name = &name[..index];
    }
    if let Some(index) = name.rfind('.') {
        name = &name[index + 1..];
    }
    name.trim().to_owned()
}

/// Exact source text of a node, degrading to an empty string for invalid UTF-8.
fn node_text(node: Node<'_>, bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[node.start_byte()..node.end_byte()]).into_owned()
}

/// Collapse every whitespace run into a single space and trim the result.
fn normalize_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Remove the closing parenthesis and separator that trail a default expression.
fn clean_default(text: &str) -> String {
    normalize_line(text)
        .trim_end_matches(')')
        .trim_end_matches(',')
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A free function keeps its bare name, parameters and declared return type.
    #[test]
    fn discovers_free_function_with_signature_and_doc() {
        let source = "/** Echo returns its input.\n * @param text value to echo\n */\nfun echo(text: String): String = text";
        let found = discover(source, "Echo.kt");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "echo");
        assert_eq!(found[0].qualified_name, "echo");
        assert_eq!(found[0].kind, FnKind::Declaration);
        assert_eq!(found[0].class_name, None);
        assert_eq!(found[0].line, 4);
        assert_eq!(found[0].parameters.len(), 1);
        assert_eq!(found[0].parameters[0].name, "text");
        assert_eq!(found[0].parameters[0].type_, "String");
        assert_eq!(found[0].parameters[0].description, "value to echo");
        assert_eq!(found[0].returns.type_, "String");
        assert_eq!(
            found[0].doc_description.as_deref(),
            Some("Echo returns its input.")
        );
        assert!(found[0].source_code.starts_with("fun echo"));
    }

    /// Class members and companion methods use `Class.method` qualified names.
    #[test]
    fn discovers_class_and_companion_methods() {
        let source = r#"class Calculator(val factor: Int) {
    fun scale(value: Int): Int = value * factor
    companion object {
        fun of(value: Int): Calculator = Calculator(value)
    }
}
"#;
        let found = discover(source, "Calculator.kt");
        let names: Vec<&str> = found
            .iter()
            .map(|item| item.qualified_name.as_str())
            .collect();
        assert_eq!(
            names,
            ["Calculator.<init>", "Calculator.scale", "Calculator.of"]
        );
        let scale = found.iter().find(|item| item.name == "scale").unwrap();
        assert_eq!(scale.kind, FnKind::Method);
        assert_eq!(scale.class_name.as_deref(), Some("Calculator"));
        let of = found.iter().find(|item| item.name == "of").unwrap();
        assert_eq!(of.class_name.as_deref(), Some("Calculator"));
        assert_eq!(of.qualified_name, "Calculator.of");
    }

    /// Defaults, `suspend` and nullable types are preserved in signatures.
    #[test]
    fn discovers_parameters_defaults_and_suspend() {
        let source = "suspend fun load(id: Int, tag: String? = null): String? = tag";
        let found = discover(source, "Load.kt");
        assert_eq!(found.len(), 1);
        assert!(found[0].is_async);
        assert_eq!(found[0].parameters.len(), 2);
        assert!(found[0].parameters[0].required);
        assert_eq!(found[0].parameters[1].name, "tag");
        assert_eq!(found[0].parameters[1].type_, "String?");
        assert_eq!(
            found[0].parameters[1].default_value,
            Some(Value::String("null".to_owned()))
        );
        assert!(!found[0].parameters[1].required);
        assert_eq!(found[0].returns.type_, "String?");
    }

    /// Classes, data-class constructor fields, objects and aliases enter the index.
    #[test]
    fn collects_types_from_classes_objects_and_aliases() {
        let source = r#"data class Item(val id: Int, val label: String)
object Registry { val name: String = "r" }
typealias ItemAlias = Item
"#;
        let types = collect_types(source);
        let item = types.get("Item").expect("data class must be indexed");
        assert_eq!(item.fields[0].name, "id");
        assert_eq!(item.fields[0].type_, "Int");
        assert_eq!(item.fields[1].name, "label");
        let registry = types.get("Registry").expect("object must be indexed");
        assert_eq!(registry.fields[0].name, "name");
        let alias = types.get("ItemAlias").expect("alias must be indexed");
        assert_eq!(alias.fields, item.fields);
    }

    /// A function declared inside another function's body is not discovered.
    #[test]
    fn ignores_nested_local_functions() {
        let source = "fun outer(): Int { fun inner(): Int = 1; return inner() }";
        let found = discover(source, "Nested.kt");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "outer");
    }

    /// A secondary constructor is recorded as an `<init>` method of its class.
    #[test]
    fn discovers_secondary_constructor() {
        let source = "class Box { constructor(value: Int) { println(value) } }";
        let found = discover(source, "Box.kt");
        assert!(found.iter().any(|item| item.qualified_name == "Box.<init>"));
        let init = found.iter().find(|item| item.name == "<init>").unwrap();
        assert_eq!(init.kind, FnKind::Method);
        assert_eq!(init.parameters.len(), 1);
        assert_eq!(init.returns.type_, "Box");
    }
}
