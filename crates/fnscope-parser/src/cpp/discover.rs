//! Deterministic discovery of C++ free functions, class members and type aliases.
//!
//! Tree-sitter-cpp 0.23.4 emits no `method_definition` node: a member function is
//! a `function_definition` inside a `field_declaration_list`, a bodyless method is
//! a `field_declaration`, and a constructor declaration is a `declaration`. All
//! three are handled here. The pass never descends into a `compound_statement`,
//! so a nested helper cannot be mistaken for a top-level callable.

use std::collections::{BTreeMap, HashMap};

use fnscope_model::{Parameter, ReturnField, ReturnShape};
use serde_json::Value;
use tree_sitter::{Node, Parser, Tree};

use crate::resolve::resolve_fields_from_map;
use crate::types::{DiscoveredFn, FnKind};

/// Parse one C++ source file, or `None` when the grammar cannot be loaded.
pub(crate) fn parse_tree(source: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_cpp::LANGUAGE.into();
    parser.set_language(&language).ok()?;
    parser.parse(source, None)
}

/// Discover every callable in one C++ file, sorted by `(line, qualified_name)`.
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
    let types = TypeIndex::build(root, bytes).named_map();

    let mut found = Vec::new();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        match child.kind() {
            "function_definition" => record_top_level(child, bytes, &types, &mut found),
            "class_specifier" | "struct_specifier" => {
                if let Some(class) = specifier_name(child, bytes) {
                    visit_class_body(child, &class, bytes, &types, &mut found);
                }
            }
            _ => {}
        }
    }

    found.sort_by(|a, b| (a.line, &a.qualified_name).cmp(&(b.line, &b.qualified_name)));
    found.dedup_by(|a, b| a.qualified_name == b.qualified_name && a.line == b.line);
    found
}

/// Record a top-level definition as a free function or a scoped class member.
fn record_top_level(
    node: Node<'_>,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let declarator = match find_function_declarator(node) {
        Some(declarator) => declarator,
        None => return,
    };
    let (name, scope) = declarator_identity(declarator, bytes);
    if name.is_empty() {
        return;
    }
    match scope {
        Some(class) => record_fn(node, name, FnKind::Method, Some(&class), bytes, types, out),
        None => record_fn(node, name, FnKind::Declaration, None, bytes, types, out),
    }
}

/// Visit the members of one `field_declaration_list`, skipping nested bodies.
fn visit_class_body(
    class_node: Node<'_>,
    class: &str,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let body = match class_node.child_by_field_name("body") {
        Some(body) if body.kind() == "field_declaration_list" => body,
        _ => return,
    };
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        let recordable = match member.kind() {
            "function_definition" => true,
            // A bodyless method has a return type (`field_declaration`); a
            // constructor has none (`declaration`). Both wrap a function declarator.
            "field_declaration" | "declaration" => find_function_declarator(member).is_some(),
            _ => false,
        };
        if !recordable {
            continue;
        }
        let declarator = match find_function_declarator(member) {
            Some(declarator) => declarator,
            None => continue,
        };
        let (name, _) = declarator_identity(declarator, bytes);
        record_fn(member, name, FnKind::Method, Some(class), bytes, types, out);
    }
}

/// Build one `DiscoveredFn` from a C++ definition or member declaration.
fn record_fn(
    node: Node<'_>,
    name: String,
    kind: FnKind,
    class_name: Option<&str>,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    if name.is_empty() {
        return;
    }
    let declarator = find_function_declarator(node);
    let doc_start = node.start_byte();
    let doc = leading_cpp_comment(root_of(node), doc_start, bytes)
        .map(|raw| parse_cpp_doc(&raw))
        .unwrap_or_default();
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
        parameters: extract_parameters(node, bytes, &doc),
        returns: extract_return(node, declarator, bytes, types),
        doc_description: doc.description.clone(),
        tags: doc.tags.clone(),
        // C++ coroutines are statements, not a different function kind.
        is_async: false,
        class_name: class_name.map(str::to_string),
    });
}

/// Find the `function_declarator` below a node without entering its body.
pub(crate) fn find_function_declarator(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "function_declarator" {
        return Some(node);
    }
    if matches!(node.kind(), "compound_statement" | "parameter_list") {
        return None;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = find_function_declarator(child) {
            return Some(found);
        }
    }
    None
}

/// Read `(member name, optional owning class)` from a function declarator.
pub(crate) fn declarator_identity(node: Node<'_>, bytes: &[u8]) -> (String, Option<String>) {
    let node = if node.kind() == "function_declarator" {
        match node.child_by_field_name("declarator") {
            Some(inner) => inner,
            None => return (String::new(), None),
        }
    } else {
        node
    };

    if node.kind() == "qualified_identifier" {
        let name = {
            let mut cursor = node.walk();
            // Bind the consumer so the iterator temporary drops before `cursor`.
            let found = node
                .children_by_field_name("name", &mut cursor)
                .last()
                .map(|last| bare_cpp_name(&node_text(last, bytes)))
                .unwrap_or_default();
            found
        };
        let scope = node
            .child_by_field_name("scope")
            .map(|scope| bare_cpp_name(&node_text(scope, bytes)))
            .filter(|scope| !scope.is_empty());
        return (name, scope);
    }

    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "type_identifier" | "operator_name" | "destructor_name"
    ) {
        return (node_text(node, bytes).trim().to_owned(), None);
    }

    if matches!(
        node.kind(),
        "pointer_declarator"
            | "reference_declarator"
            | "parenthesized_declarator"
            | "attributed_declarator"
            | "array_declarator"
    ) {
        if let Some(inner) = node.child_by_field_name("declarator") {
            return declarator_identity(inner, bytes);
        }
    }

    if node.kind() == "template_function" {
        if let Some(name) = node.child_by_field_name("name") {
            return (bare_cpp_name(&node_text(name, bytes)), None);
        }
    }

    (node_text(node, bytes).trim().to_owned(), None)
}

/// The declared name of a `class_specifier` or `struct_specifier`.
fn specifier_name(node: Node<'_>, bytes: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .map(|name| bare_cpp_name(&node_text(name, bytes)))
        .filter(|name| !name.is_empty())
}

/// Collect named parameters, including defaults on `optional_parameter_declaration`.
fn extract_parameters(node: Node<'_>, bytes: &[u8], doc: &DocInfo) -> Vec<Parameter> {
    let declarator = match find_function_declarator(node) {
        Some(declarator) => declarator,
        None => return Vec::new(),
    };
    let list = match declarator.child_by_field_name("parameters") {
        Some(list) => list,
        None => return Vec::new(),
    };
    let mut params = Vec::new();
    let mut cursor = list.walk();
    for declaration in list.named_children(&mut cursor) {
        if !matches!(
            declaration.kind(),
            "parameter_declaration" | "optional_parameter_declaration"
        ) {
            continue;
        }
        let name = declaration
            .child_by_field_name("declarator")
            .map(|declarator| declarator_identity(declarator, bytes).0)
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let type_ = declaration
            .child_by_field_name("type")
            .map(|type_node| normalize_line(&node_text(type_node, bytes)))
            .unwrap_or_else(|| "unknown".to_owned());
        let (required, default_value) = if declaration.kind() == "optional_parameter_declaration" {
            (
                false,
                declaration
                    .child_by_field_name("default_value")
                    .and_then(|value| default_value_json(value, bytes)),
            )
        } else {
            (true, None)
        };
        params.push(Parameter {
            name: name.clone(),
            type_,
            required,
            default_value,
            description: doc
                .param_descriptions
                .get(&name)
                .cloned()
                .unwrap_or_default(),
        });
    }
    params
}

/// Convert a default-expression node into a stable JSON value.
fn default_value_json(node: Node<'_>, bytes: &[u8]) -> Option<Value> {
    let text = node_text(node, bytes);
    let trimmed = text.trim();
    match node.kind() {
        "string_literal" | "char_literal" => Some(Value::String(unquote(trimmed))),
        "number_literal" => serde_json::from_str::<Value>(trimmed).ok(),
        _ if trimmed == "true" || trimmed == "false" => serde_json::from_str::<Value>(trimmed).ok(),
        _ if trimmed.parse::<f64>().is_ok() => serde_json::from_str::<Value>(trimmed).ok(),
        _ if trimmed.is_empty() => None,
        _ => Some(Value::String(trimmed.to_owned())),
    }
}

/// Read the declared return type, including a trailing `-> Type` annotation.
fn extract_return(
    node: Node<'_>,
    declarator: Option<Node<'_>>,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
) -> ReturnShape {
    let mut type_ = node
        .child_by_field_name("type")
        .map(|type_node| normalize_line(&node_text(type_node, bytes)))
        .unwrap_or_default();
    if type_.is_empty() || type_ == "auto" {
        if let Some(declarator) = declarator {
            let mut cursor = declarator.walk();
            for child in declarator.named_children(&mut cursor) {
                if child.kind() == "trailing_return_type" {
                    let raw = normalize_line(&node_text(child, bytes));
                    type_ = raw.trim_start_matches("->").trim().to_owned();
                    break;
                }
            }
        }
    }
    if type_.is_empty() || type_ == "auto" {
        return ReturnShape::default();
    }
    if type_ == "void" {
        return ReturnShape {
            type_: "void".to_owned(),
            fields: Vec::new(),
        };
    }
    let fields = resolve_fields_from_map(&cpp_type_lookup_name(&type_), types);
    ReturnShape { type_, fields }
}

/// Index class/struct fields and `typedef`/`using` aliases for cross-file returns.
///
/// The map is keyed by declared type name and can be shared across files so a
/// return type declared elsewhere still resolves its fields.
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
            "class_specifier" | "struct_specifier" => self.record_specifier(node, bytes),
            "type_definition" => self.record_type_definition(node, bytes),
            "alias_declaration" => self.record_using(node, bytes),
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.walk(child, bytes);
        }
    }

    fn record_specifier(&mut self, node: Node<'_>, bytes: &[u8]) {
        let name = match specifier_name(node, bytes) {
            Some(name) => name,
            None => return,
        };
        let fields = self.fields_from_body(node, bytes);
        if !fields.is_empty() {
            self.named.insert(name, fields);
        }
    }

    /// Named fields of a `field_declaration_list`, skipping methods and functions.
    fn fields_from_body(&self, node: Node<'_>, bytes: &[u8]) -> Vec<ReturnField> {
        let body = match node.child_by_field_name("body") {
            Some(body) if body.kind() == "field_declaration_list" => body,
            _ => return Vec::new(),
        };
        let mut fields = Vec::new();
        let mut cursor = body.walk();
        for member in body.named_children(&mut cursor) {
            if member.kind() != "field_declaration" || find_function_declarator(member).is_some() {
                continue;
            }
            let type_ = member
                .child_by_field_name("type")
                .map(|type_node| normalize_line(&node_text(type_node, bytes)))
                .unwrap_or_default();
            if type_.is_empty() {
                continue;
            }
            let mut declarators = member.walk();
            for declarator in member.children_by_field_name("declarator", &mut declarators) {
                for name in declarator_names(declarator, bytes) {
                    fields.push(ReturnField {
                        name,
                        type_: type_.clone(),
                    });
                }
            }
        }
        fields
    }

    fn record_type_definition(&mut self, node: Node<'_>, bytes: &[u8]) {
        let mut declarators = node.walk();
        let alias = node
            .children_by_field_name("declarator", &mut declarators)
            .map(|declarator| bare_cpp_name(&node_text(declarator, bytes)))
            .find(|name| !name.is_empty());
        let alias = match alias {
            Some(alias) => alias,
            None => return,
        };
        if let Some(type_node) = node.child_by_field_name("type") {
            if matches!(type_node.kind(), "class_specifier" | "struct_specifier") {
                let fields = self.fields_from_body(type_node, bytes);
                if !fields.is_empty() {
                    self.named.insert(alias, fields);
                    return;
                }
            }
            self.aliases
                .insert(alias, cpp_type_lookup_name(&node_text(type_node, bytes)));
        }
    }

    fn record_using(&mut self, node: Node<'_>, bytes: &[u8]) {
        let name = node
            .child_by_field_name("name")
            .map(|name| bare_cpp_name(&node_text(name, bytes)))
            .unwrap_or_default();
        let target = node
            .child_by_field_name("type")
            .map(|type_node| cpp_type_lookup_name(&node_text(type_node, bytes)))
            .unwrap_or_default();
        if !name.is_empty() && !target.is_empty() {
            self.aliases.insert(name, target);
        }
    }

    /// Resolve aliases to fields, then expose only named types with known fields.
    fn named_map(&self) -> BTreeMap<String, ReturnShape> {
        let mut resolved = self.named.clone();
        for _ in 0..4 {
            let mut changed = false;
            for (alias, target) in &self.aliases {
                if !resolved.contains_key(alias) {
                    if let Some(fields) = resolved.get(target).cloned() {
                        resolved.insert(alias.clone(), fields);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
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

/// Recursively collect field names from one declarator, ignoring method declarators.
fn declarator_names(node: Node<'_>, bytes: &[u8]) -> Vec<String> {
    if matches!(node.kind(), "field_identifier" | "identifier") {
        return vec![node_text(node, bytes).trim().to_owned()]
            .into_iter()
            .filter(|name| !name.is_empty())
            .collect();
    }
    if node.kind() == "function_declarator" {
        return Vec::new();
    }
    let mut names = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        names.extend(declarator_names(child, bytes));
    }
    names
}

// ---------------------------------------------------------------------------
// Documentation and small helpers
// ---------------------------------------------------------------------------

/// Documentation facts read from the nearest contiguous leading `//` comment.
#[derive(Default)]
struct DocInfo {
    description: Option<String>,
    param_descriptions: HashMap<String, String>,
    tags: Vec<String>,
}

/// Parse a C++ comment block into a description, `@param` texts and `@tags` values.
fn parse_cpp_doc(raw: &str) -> DocInfo {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let line = line.trim().trim_start_matches("//").trim();
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
                .trim_end_matches(':')
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

/// Combine the contiguous `//` comments immediately above a declaration.
fn leading_cpp_comment(root: Node<'_>, declaration_start: usize, bytes: &[u8]) -> Option<String> {
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
        let raw = node_text(comment, bytes);
        if !raw.trim_start().starts_with("//") {
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

/// Recursive half of [`leading_cpp_comment`]; only comments above the limit qualify.
fn collect_comments<'tree>(node: Node<'tree>, limit: usize, out: &mut Vec<Node<'tree>>) {
    if node.kind() == "comment" && node.end_byte() <= limit {
        out.push(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_comments(child, limit, out);
    }
}

/// Walk up through wrappers so a documented declaration uses its own start byte.
fn root_of(mut node: Node<'_>) -> Node<'_> {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

/// Reduce a C++ name to its bare identifier for index lookups.
fn bare_cpp_name(text: &str) -> String {
    cpp_type_lookup_name(text)
}

/// Reduce a declared C++ type to the bare name used as a key in the type index.
pub(crate) fn cpp_type_lookup_name(text: &str) -> String {
    let mut name = text.trim();
    for prefix in ["const ", "volatile ", "struct ", "class ", "typename "] {
        if let Some(rest) = name.strip_prefix(prefix) {
            name = rest.trim_start();
        }
    }
    name = name
        .trim_end_matches(|ch: char| ch == '*' || ch == '&' || ch == ' ')
        .trim();
    if let Some((_, last)) = name.rsplit_once("::") {
        name = last.trim();
    }
    if let Some(index) = name.find('<') {
        name = &name[..index];
    }
    name.trim().to_owned()
}

/// Exact source text of a node, degrading to an empty string for invalid UTF-8.
pub(crate) fn node_text(node: Node<'_>, bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[node.start_byte()..node.end_byte()]).into_owned()
}

/// Collapse every whitespace run into a single space and trim the result.
pub(crate) fn normalize_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Remove surrounding quotes from a C++ string or character literal's source text.
pub(crate) fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'"' | b'\'') && bytes[bytes.len() - 1] == bytes[0] {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    trimmed.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A free function keeps its bare name, ordered parameters and declared result.
    #[test]
    fn cpp_discovers_free_function_with_parameters_and_result() {
        let source = "// Add two values.\nint add(int a, int b) { return a + b; }";
        let found = discover(source, "add.cpp");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "add");
        assert_eq!(found[0].qualified_name, "add");
        assert_eq!(found[0].kind, FnKind::Declaration);
        assert_eq!(found[0].class_name, None);
        assert_eq!(found[0].line, 2);
        assert_eq!(found[0].returns.type_, "int");
        assert_eq!(found[0].doc_description.as_deref(), Some("Add two values."));
        assert!(found[0].source_code.starts_with("int add("));
    }

    /// Members are recorded as `Class.method`, including a constructor declaration.
    #[test]
    fn cpp_discovers_class_methods_and_constructor() {
        let source = r#"class Calc {
public:
  // Scale input.
  int scale(int n) { return n * 2; }
  Calc(int seed);
  void reset() const;
};
Calc::Calc(int seed) { (void)seed; }
"#;
        let found = discover(source, "calc.cpp");
        let names: Vec<&str> = found
            .iter()
            .map(|item| item.qualified_name.as_str())
            .collect();
        assert_eq!(
            names,
            ["Calc.scale", "Calc.Calc", "Calc.reset", "Calc.Calc"]
        );
        assert!(found
            .iter()
            .all(|item| item.kind == FnKind::Method && item.class_name.as_deref() == Some("Calc")));
        let scale = found.iter().find(|item| item.name == "scale").unwrap();
        assert_eq!(scale.parameters.len(), 1);
        assert_eq!(scale.parameters[0].name, "n");
    }

    /// Optional defaults and a trailing return type are read from the declarator.
    #[test]
    fn cpp_reads_parameter_defaults_and_trailing_return() {
        let source = "auto combine(int a = 5, const char* b = \"x\") -> long { return a; }";
        let found = discover(source, "defaults.cpp");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].returns.type_, "long");
        assert_eq!(found[0].parameters.len(), 2);
        assert_eq!(found[0].parameters[0].name, "a");
        assert!(!found[0].parameters[0].required);
        assert_eq!(
            found[0].parameters[0].default_value,
            Some(Value::Number(5.into()))
        );
        assert_eq!(found[0].parameters[1].type_, "char");
        assert_eq!(
            found[0].parameters[1].default_value,
            Some(Value::String("x".to_owned()))
        );
    }

    /// Classes, structs, typedefs and using aliases form one type index.
    #[test]
    fn cpp_collect_types_indexes_fields_and_aliases() {
        let source = r#"struct Point { int x; double y; };
class Box { public: int width; };
typedef struct { int a; double b; } Pair;
using Alias = Point;
"#;
        let types = collect_types(source);
        let point = types.get("Point").expect("struct must be indexed");
        assert_eq!(point.fields[0].name, "x");
        assert_eq!(point.fields[0].type_, "int");
        assert_eq!(point.fields[1].name, "y");
        let box_ = types.get("Box").expect("class must be indexed");
        assert_eq!(box_.fields[0].name, "width");
        let pair = types.get("Pair").expect("typedef must be indexed");
        assert_eq!(pair.fields[0].name, "a");
        assert_eq!(pair.fields[1].type_, "double");
        let alias = types.get("Alias").expect("using alias must be indexed");
        assert_eq!(alias.fields, point.fields);
    }

    /// A method has no async flag and nested bodies never leak new callables.
    #[test]
    fn cpp_ignores_nested_function_bodies() {
        let source = "int outer() { auto inner = [](int x) { return x; }; return inner(1); }";
        let found = discover(source, "nested.cpp");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "outer");
        assert!(!found[0].is_async);
    }
}
