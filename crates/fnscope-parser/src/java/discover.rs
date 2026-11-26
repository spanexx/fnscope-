//! Deterministic discovery of Java callables and object-shaped types.
//!
//! Only methods and constructors directly inside a top-level class, interface or
//! record become callables. The `class_body`/`interface_body` wrappers are
//! unwrapped explicitly; classes nested inside a method body are never visited,
//! so local classes and their methods cannot leak into the manifest.

use std::collections::{BTreeMap, HashMap};

use fnscope_model::{Parameter, ReturnField, ReturnShape};
use tree_sitter::{Node, Parser, Tree};

use crate::resolve::resolve_fields_from_map;
use crate::types::{DiscoveredFn, FnKind};

/// Parse one Java source file, or `None` when no tree can be produced.
fn parse_tree(source: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_java::LANGUAGE.into();
    parser.set_language(&language).ok()?;
    parser.parse(source, None)
}

/// Discover every callable in one Java file, sorted by `(line, qualified_name)`.
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
        // Local classes live inside a method's `block`, never directly under
        // `program`, so walking only these three top-level kinds is the guard
        // against nested classes and their functions.
        if !matches!(
            child.kind(),
            "class_declaration" | "interface_declaration" | "record_declaration"
        ) {
            continue;
        }
        let class_name = child
            .child_by_field_name("name")
            .map(|name| node_text(name, bytes))
            .unwrap_or_default();
        if class_name.is_empty() {
            continue;
        }
        // Wrapper nodes: the `body` field is a `class_body` (class/record) or an
        // `interface_body`. Methods are its direct named children.
        let body = match child.child_by_field_name("body") {
            Some(body) => body,
            None => continue,
        };
        let mut body_cursor = body.walk();
        for member in body.named_children(&mut body_cursor) {
            match member.kind() {
                "method_declaration" | "constructor_declaration" => {
                    record_method(member, &class_name, bytes, &types, &mut found);
                }
                // A `static_initializer` wraps a `block`, not a callable, and
                // nested `class_declaration`s are deliberately not descended.
                _ => {}
            }
        }
    }
    found.sort_by(|a, b| (a.line, &a.qualified_name).cmp(&(b.line, &b.qualified_name)));
    found.dedup_by(|a, b| a.qualified_name == b.qualified_name && a.line == b.line);
    found
}

/// Build one `DiscoveredFn` from a Java method or constructor declaration.
fn record_method(
    node: Node<'_>,
    class_name: &str,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let name = node
        .child_by_field_name("name")
        .map(|name| node_text(name, bytes))
        .unwrap_or_default();
    if name.is_empty() {
        return;
    }
    let doc = leading_line_comment(root_of(node), node.start_byte(), bytes)
        .map(|raw| parse_java_doc(&raw))
        .unwrap_or_default();
    let start = node.start_position();
    let end = node.end_position();
    // Java has no module-level functions: every callable belongs to a type.
    let qualified_name = format!("{class_name}.{name}");
    // A constructor has no declared `type` field, so it returns `void`.
    let returns = if node.kind() == "method_declaration" {
        extract_java_returns(node, bytes, types)
    } else {
        ReturnShape::default()
    };

    out.push(DiscoveredFn {
        name,
        qualified_name,
        kind: FnKind::Method,
        line: start.row + 1,
        end_line: end.row + 1,
        source_code: String::from_utf8_lossy(&bytes[node.start_byte()..node.end_byte()])
            .into_owned(),
        parameters: extract_java_parameters(node, bytes, &doc),
        returns,
        doc_description: doc.description.clone(),
        tags: doc.tags.clone(),
        // Java method declarations have no `async` modifier; concurrency is a
        // statement or library call, not a function kind.
        is_async: false,
        class_name: Some(class_name.to_owned()),
    });
}

/// Collect parameters from `formal_parameter` and `spread_parameter` nodes.
fn extract_java_parameters(node: Node<'_>, bytes: &[u8], doc: &DocInfo) -> Vec<Parameter> {
    let mut params = Vec::new();
    // Wrapper node: `formal_parameters` holds the parameter declarations.
    let list = match node.child_by_field_name("parameters") {
        Some(list) => list,
        None => return params,
    };
    let mut cursor = list.walk();
    for declaration in list.named_children(&mut cursor) {
        match declaration.kind() {
            "formal_parameter" => {
                let name = declaration
                    .child_by_field_name("name")
                    .map(|name| normalize_line(&node_text(name, bytes)))
                    .unwrap_or_default();
                if name.is_empty() {
                    continue;
                }
                let type_ = declaration
                    .child_by_field_name("type")
                    .map(|type_node| normalize_line(&node_text(type_node, bytes)))
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| "unknown".to_owned());
                params.push(Parameter {
                    name: name.clone(),
                    type_,
                    required: true,
                    default_value: None,
                    description: doc
                        .param_descriptions
                        .get(&name)
                        .cloned()
                        .unwrap_or_default(),
                });
            }
            // `spread_parameter` (e.g. `String... args`) has no `name`/`type`
            // fields: its name lives in a `variable_declarator` wrapper and its
            // type is the other named child.
            "spread_parameter" => {
                let mut inner_cursor = declaration.walk();
                let children: Vec<Node<'_>> =
                    declaration.named_children(&mut inner_cursor).collect();
                let mut name = String::new();
                let mut type_ = String::new();
                for child in children {
                    match child.kind() {
                        "variable_declarator" => {
                            name = child
                                .child_by_field_name("name")
                                .map(|name| normalize_line(&node_text(name, bytes)))
                                .unwrap_or_default();
                        }
                        "modifiers" | "annotation" | "marker_annotation" => {}
                        _ => {
                            let text = normalize_line(&node_text(child, bytes));
                            if !text.is_empty() {
                                type_ = text;
                            }
                        }
                    }
                }
                if name.is_empty() {
                    continue;
                }
                if type_.is_empty() {
                    type_ = "unknown".to_owned();
                }
                params.push(Parameter {
                    name: name.clone(),
                    type_,
                    required: true,
                    default_value: None,
                    description: doc
                        .param_descriptions
                        .get(&name)
                        .cloned()
                        .unwrap_or_default(),
                });
            }
            // An explicit `receiver_parameter` is not a caller-supplied value.
            _ => {}
        }
    }
    params
}

/// Read the declared result of a method and resolve fields from the type index.
fn extract_java_returns(
    node: Node<'_>,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
) -> ReturnShape {
    let type_node = match node.child_by_field_name("type") {
        Some(type_node) => type_node,
        None => return ReturnShape::default(),
    };
    let type_ = normalize_line(&node_text(type_node, bytes));
    if type_.is_empty() {
        return ReturnShape::default();
    }
    let fields = resolve_fields_from_map(&type_, types);
    ReturnShape { type_, fields }
}

/// Index class fields and record components for cross-file return fields.
pub fn collect_types(source: &str) -> BTreeMap<String, ReturnShape> {
    let tree = match parse_tree(source) {
        Some(tree) => tree,
        None => return BTreeMap::new(),
    };
    TypeIndex::build(tree.root_node(), source.as_bytes()).named_map()
}

/// Type declarations of one file, keyed by their simple declared name.
struct TypeIndex {
    named: HashMap<String, Vec<ReturnField>>,
}

impl TypeIndex {
    /// Walk the whole tree once so a type declared after its use is still indexed.
    fn build(root: Node<'_>, bytes: &[u8]) -> Self {
        let mut index = Self {
            named: HashMap::new(),
        };
        index.walk(root, bytes);
        index
    }

    fn walk(&mut self, node: Node<'_>, bytes: &[u8]) {
        match node.kind() {
            "class_declaration" => self.record_class(node, bytes),
            "record_declaration" => self.record_record(node, bytes),
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.walk(child, bytes);
        }
    }

    /// Index `field_declaration` members of the `class_body` wrapper.
    fn record_class(&mut self, node: Node<'_>, bytes: &[u8]) {
        let name = match node.child_by_field_name("name") {
            Some(name) => node_text(name, bytes),
            None => return,
        };
        if name.is_empty() {
            return;
        }
        let body = match node.child_by_field_name("body") {
            Some(body) => body,
            None => return,
        };
        let mut fields = Vec::new();
        let mut cursor = body.walk();
        for member in body.named_children(&mut cursor) {
            if member.kind() == "field_declaration" {
                fields.extend(field_declaration_fields(member, bytes));
            }
        }
        if !fields.is_empty() {
            self.named.insert(name, fields);
        }
    }

    /// Index the `formal_parameter` components of a `record_declaration`.
    fn record_record(&mut self, node: Node<'_>, bytes: &[u8]) {
        let name = match node.child_by_field_name("name") {
            Some(name) => node_text(name, bytes),
            None => return,
        };
        if name.is_empty() {
            return;
        }
        let parameters = match node.child_by_field_name("parameters") {
            Some(parameters) => parameters,
            None => return,
        };
        let mut fields = Vec::new();
        // Wrapper node: `formal_parameters` holds the record components.
        let mut cursor = parameters.walk();
        for component in parameters.named_children(&mut cursor) {
            if component.kind() != "formal_parameter" {
                continue;
            }
            let field_name = component
                .child_by_field_name("name")
                .map(|name| normalize_line(&node_text(name, bytes)))
                .unwrap_or_default();
            if field_name.is_empty() {
                continue;
            }
            let field_type = component
                .child_by_field_name("type")
                .map(|type_node| normalize_line(&node_text(type_node, bytes)))
                .unwrap_or_else(|| "unknown".to_owned());
            fields.push(ReturnField {
                name: field_name,
                type_: field_type,
            });
        }
        if !fields.is_empty() {
            self.named.insert(name, fields);
        }
    }

    /// Return the contract shape keyed by type name; empty entries are omitted.
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
}

/// Named fields of a `field_declaration`, including `int a, b;` declarators.
fn field_declaration_fields(node: Node<'_>, bytes: &[u8]) -> Vec<ReturnField> {
    let type_ = node
        .child_by_field_name("type")
        .map(|type_node| normalize_line(&node_text(type_node, bytes)))
        .unwrap_or_default();
    if type_.is_empty() {
        return Vec::new();
    }
    let mut fields = Vec::new();
    // Wrapper node: each declarator is a `variable_declarator`.
    let mut cursor = node.walk();
    for declarator in node.children_by_field_name("declarator", &mut cursor) {
        let name = declarator
            .child_by_field_name("name")
            .map(|name| normalize_line(&node_text(name, bytes)))
            .unwrap_or_default();
        if !name.is_empty() {
            fields.push(ReturnField {
                name,
                type_: type_.clone(),
            });
        }
    }
    fields
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

/// Parse a Java `//` comment block into description, `@param` texts and tags.
fn parse_java_doc(raw: &str) -> DocInfo {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let mut line = line.trim();
        if let Some(rest) = line.strip_prefix("//") {
            line = rest.trim();
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
            let name = words.next().unwrap_or_default().to_owned();
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

/// Combine contiguous `line_comment` (`//`) nodes immediately above a declaration.
fn leading_line_comment(root: Node<'_>, declaration_start: usize, bytes: &[u8]) -> Option<String> {
    let mut comments = Vec::new();
    collect_line_comments(root, declaration_start, &mut comments);
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

/// Recursive half of [`leading_line_comment`]; only comments above the limit qualify.
fn collect_line_comments<'tree>(node: Node<'tree>, limit: usize, out: &mut Vec<Node<'tree>>) {
    if node.kind() == "line_comment" && node.end_byte() <= limit {
        out.push(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_line_comments(child, limit, out);
    }
}

/// Walk up through wrappers so a documented declaration uses its own start byte.
fn root_of(mut node: Node<'_>) -> Node<'_> {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

/// Exact source text of a node, degrading to an empty string for invalid UTF-8.
fn node_text(node: Node<'_>, bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[node.start_byte()..node.end_byte()]).into_owned()
}

/// Collapse every whitespace run into a single space and trim the result.
fn normalize_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A class method is qualified by its class and keeps its doc comment.
    #[test]
    fn java_discovers_class_method_with_doc_and_kind() {
        let source = r#"
public class Service {
    // normalize trims the supplied text.
    public String normalize(String text) { return text.trim(); }
}
"#;
        let found = discover(source, "Service.java");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "normalize");
        assert_eq!(found[0].qualified_name, "Service.normalize");
        assert_eq!(found[0].kind, FnKind::Method);
        assert_eq!(found[0].class_name.as_deref(), Some("Service"));
        assert_eq!(
            found[0].doc_description.as_deref(),
            Some("normalize trims the supplied text.")
        );
        assert!(found[0].source_code.starts_with("public String normalize"));
    }

    /// A constructor is a method-shaped callable returning `void`.
    #[test]
    fn java_discovers_constructor_as_method() {
        let source = "class Box {\n  Box(int size) { this.size = size; }\n  int size;\n}";
        let found = discover(source, "Box.java");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Box");
        assert_eq!(found[0].qualified_name, "Box.Box");
        assert_eq!(found[0].kind, FnKind::Method);
        assert_eq!(found[0].class_name.as_deref(), Some("Box"));
        assert_eq!(found[0].returns.type_, "void");
        assert_eq!(found[0].parameters.len(), 1);
        assert_eq!(found[0].parameters[0].type_, "int");
    }

    /// Formal parameters, varargs and the declared return type are preserved.
    #[test]
    fn java_discovers_parameters_and_return_type() {
        let source = r#"
class Api {
    java.util.List<String> load(String key, String... rest) { return null; }
}
"#;
        let found = discover(source, "Api.java");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].parameters.len(), 2);
        assert_eq!(found[0].parameters[0].name, "key");
        assert_eq!(found[0].parameters[0].type_, "String");
        assert_eq!(found[0].parameters[1].name, "rest");
        assert_eq!(found[0].parameters[1].type_, "String");
        assert_eq!(found[0].returns.type_, "java.util.List<String>");
    }

    /// Interface method signatures are callables even without a body.
    #[test]
    fn java_discovers_interface_method_signature() {
        let source = "interface Reader {\n  // Read one item.\n  String read(int id);\n}";
        let found = discover(source, "Reader.java");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].qualified_name, "Reader.read");
        assert_eq!(found[0].kind, FnKind::Method);
        assert_eq!(found[0].doc_description.as_deref(), Some("Read one item."));
    }

    /// Local classes and their methods inside a body are not discovered.
    #[test]
    fn java_ignores_local_classes_inside_method_bodies() {
        let source = r#"
class Outer {
    void run() {
        class Inner { void hidden() {} }
        Inner local = new Inner();
    }
}
"#;
        let found = discover(source, "Outer.java");
        let names: Vec<&str> = found
            .iter()
            .map(|item| item.qualified_name.as_str())
            .collect();
        assert_eq!(names, ["Outer.run"]);
    }

    /// Class fields and record components form one BTreeMap-shaped type index.
    #[test]
    fn java_collect_types_indexes_class_fields_and_record_components() {
        let source = r#"
class RoleResult { String role; int score; }
record Pair(String left, int right) {}
"#;
        let types = collect_types(source);
        let result = types.get("RoleResult").expect("class must be indexed");
        assert_eq!(result.fields[0].name, "role");
        assert_eq!(result.fields[0].type_, "String");
        assert_eq!(result.fields[1].name, "score");
        let pair = types.get("Pair").expect("record must be indexed");
        assert_eq!(pair.fields[0].name, "left");
        assert_eq!(pair.fields[0].type_, "String");
        assert_eq!(pair.fields[1].name, "right");
    }

    /// Same-file return fields resolve during discovery; cross-file ones do not.
    #[test]
    fn java_resolves_same_file_return_fields_only() {
        let source = r#"
class Item { String id; }
class Repo { Item load() { return null; } }
"#;
        let found = discover(source, "Repo.java");
        let load = found
            .iter()
            .find(|item| item.name == "load")
            .expect("load must be discovered");
        assert_eq!(load.returns.type_, "Item");
        assert_eq!(load.returns.fields.len(), 1);
        assert_eq!(load.returns.fields[0].name, "id");

        let other = discover("class Repo { Item load() { return null; } }", "Other.java");
        assert!(other[0].returns.fields.is_empty());
    }
}
