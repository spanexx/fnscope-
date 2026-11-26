//! Deterministic discovery of Go functions, methods and type declarations.
//!
//! Only `function_declaration` and `method_declaration` nodes directly under the
//! source file become callables. A Go function literal (`func_literal`) inside a
//! body is never visited, so nested closures cannot leak into the manifest.

use std::collections::{BTreeMap, HashMap};

use fnscope_model::{Parameter, ReturnField, ReturnShape};
use tree_sitter::{Node, Parser, Tree};

use crate::resolve::resolve_fields_from_map;
use crate::types::{DiscoveredFn, FnKind};

/// Parse one Go source file, or `None` when no tree can be produced.
fn parse_tree(source: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_go::LANGUAGE.into();
    parser.set_language(&language).ok()?;
    parser.parse(source, None)
}

/// Discover every callable in one Go file, sorted by `(line, qualified_name)`.
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
        match child.kind() {
            "function_declaration" => {
                record_go_fn(child, FnKind::Declaration, None, bytes, &types, &mut found);
            }
            "method_declaration" => {
                let receiver = receiver_type(child, bytes);
                if receiver.is_empty() {
                    continue;
                }
                record_go_fn(
                    child,
                    FnKind::Method,
                    Some(&receiver),
                    bytes,
                    &types,
                    &mut found,
                );
            }
            _ => {}
        }
    }
    found.sort_by(|a, b| (a.line, &a.qualified_name).cmp(&(b.line, &b.qualified_name)));
    found.dedup_by(|a, b| a.qualified_name == b.qualified_name && a.line == b.line);
    found
}

/// The bare receiver type of a method, so `func (s *Service)` indexes as `Service`.
fn receiver_type(node: Node<'_>, bytes: &[u8]) -> String {
    let receiver = match node.child_by_field_name("receiver") {
        Some(receiver) => receiver,
        None => return String::new(),
    };
    let mut cursor = receiver.walk();
    for declaration in receiver.named_children(&mut cursor) {
        if declaration.kind() != "parameter_declaration" {
            continue;
        }
        if let Some(type_node) = declaration.child_by_field_name("type") {
            let bare = bare_go_type(&node_text(type_node, bytes));
            if !bare.is_empty() {
                return bare;
            }
        }
    }
    String::new()
}

/// Build one `DiscoveredFn` from a Go function or method declaration.
fn record_go_fn(
    node: Node<'_>,
    kind: FnKind,
    class_name: Option<&str>,
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
    let doc = leading_go_comment(root_of(node), node.start_byte(), bytes)
        .map(|raw| parse_go_doc(&raw))
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
        parameters: extract_go_parameters(node, bytes, &doc),
        returns: extract_go_returns(node, bytes, types),
        doc_description: doc.description.clone(),
        tags: doc.tags.clone(),
        // Go has no async keyword; goroutines are statements, not function kinds.
        is_async: false,
        class_name: class_name.map(str::to_string),
    });
}

/// Collect named parameters, expanding grouped declarations such as `a, b int`.
fn extract_go_parameters(node: Node<'_>, bytes: &[u8], doc: &DocInfo) -> Vec<Parameter> {
    let mut params = Vec::new();
    let list = match node.child_by_field_name("parameters") {
        Some(list) => list,
        None => return params,
    };
    let mut cursor = list.walk();
    for declaration in list.named_children(&mut cursor) {
        if declaration.kind() != "parameter_declaration" {
            continue;
        }
        let type_ = declaration
            .child_by_field_name("type")
            .map(|type_node| normalize_line(&node_text(type_node, bytes)))
            .unwrap_or_else(|| "unknown".to_owned());
        let mut names_cursor = declaration.walk();
        for name_node in declaration.children_by_field_name("name", &mut names_cursor) {
            let name = normalize_line(&node_text(name_node, bytes));
            if name.is_empty() {
                continue; // an unnamed Go parameter has no contract name to report
            }
            let description = doc
                .param_descriptions
                .get(&name)
                .cloned()
                .unwrap_or_default();
            params.push(Parameter {
                name,
                type_: type_.clone(),
                required: true,
                default_value: None,
                description,
            });
        }
    }
    params
}

/// Read the declared result: one type, a parenthesized list, or named results.
fn extract_go_returns(
    node: Node<'_>,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
) -> ReturnShape {
    let result = match node.child_by_field_name("result") {
        Some(result) => result,
        None => return ReturnShape::default(),
    };
    if result.kind() == "parameter_list" {
        let mut fields = Vec::new();
        let mut type_texts = Vec::new();
        let mut cursor = result.walk();
        for declaration in result.named_children(&mut cursor) {
            if declaration.kind() != "parameter_declaration" {
                continue;
            }
            let type_ = declaration
                .child_by_field_name("type")
                .map(|type_node| normalize_line(&node_text(type_node, bytes)))
                .unwrap_or_else(|| "unknown".to_owned());
            type_texts.push(type_.clone());
            let mut names_cursor = declaration.walk();
            for name_node in declaration.children_by_field_name("name", &mut names_cursor) {
                let name = normalize_line(&node_text(name_node, bytes));
                if !name.is_empty() {
                    fields.push(ReturnField {
                        name,
                        type_: type_.clone(),
                    });
                }
            }
        }
        let type_ = if type_texts.is_empty() {
            "void".to_owned()
        } else {
            format!("({})", type_texts.join(", "))
        };
        return ReturnShape { type_, fields };
    }

    let type_ = normalize_line(&node_text(result, bytes));
    if type_.is_empty() {
        return ReturnShape::default();
    }
    // Hand Go-specific prefixes to the shared resolver, which keeps the full
    // declared text while looking up fields by the bare type name.
    let fields = resolve_fields_from_map(&go_type_lookup_name(&type_), types);
    ReturnShape { type_, fields }
}

/// Index Go structs, interfaces and simple aliases for cross-file return fields.
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
        if node.kind() == "type_spec" {
            self.record_type_spec(node, bytes);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child, bytes);
        }
    }

    fn record_type_spec(&mut self, node: Node<'_>, bytes: &[u8]) {
        let (name, target) = match (
            node.child_by_field_name("name"),
            node.child_by_field_name("type"),
        ) {
            (Some(name), Some(target)) => (node_text(name, bytes), target),
            _ => return,
        };
        let name = bare_go_type(&name);
        if name.is_empty() {
            return;
        }
        match target.kind() {
            "struct_type" => {
                let fields = self.fields_from_struct(target, bytes);
                self.named.insert(name, fields);
            }
            "interface_type" => {
                let fields = self.fields_from_interface(target, bytes);
                self.named.insert(name, fields);
            }
            _ => {
                self.aliases
                    .insert(name, go_type_lookup_name(&node_text(target, bytes)));
            }
        }
    }

    /// Named fields of a Go struct, including grouped `a, b int` declarations.
    fn fields_from_struct(&self, node: Node<'_>, bytes: &[u8]) -> Vec<ReturnField> {
        let mut fields = Vec::new();
        // Go 0.25 wraps struct fields in a `field_declaration_list`.
        let mut cursor = node.walk();
        let container = node
            .named_children(&mut cursor)
            .find(|child| child.kind() == "field_declaration_list")
            .unwrap_or(node);
        let mut fields_cursor = container.walk();
        for member in container.named_children(&mut fields_cursor) {
            if member.kind() != "field_declaration" {
                continue;
            }
            let type_ = member
                .child_by_field_name("type")
                .map(|type_node| normalize_line(&node_text(type_node, bytes)))
                .unwrap_or_default();
            if type_.is_empty() {
                continue; // an embedded field has no contract-visible name
            }
            let mut names_cursor = member.walk();
            for name_node in member.children_by_field_name("name", &mut names_cursor) {
                let name = normalize_line(&node_text(name_node, bytes));
                if !name.is_empty() {
                    fields.push(ReturnField {
                        name,
                        type_: type_.clone(),
                    });
                }
            }
        }
        fields
    }

    /// Interface methods become fields whose type is a compact Go signature.
    fn fields_from_interface(&self, node: Node<'_>, bytes: &[u8]) -> Vec<ReturnField> {
        let mut fields = Vec::new();
        let mut cursor = node.walk();
        for member in node.named_children(&mut cursor) {
            if member.kind() != "method_elem" {
                continue;
            }
            let name = member
                .child_by_field_name("name")
                .map(|name| normalize_line(&node_text(name, bytes)))
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            fields.push(ReturnField {
                name,
                type_: go_method_signature(member, bytes),
            });
        }
        fields
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

/// Compact signature of an interface method, used as a `ReturnField` type.
fn go_method_signature(node: Node<'_>, bytes: &[u8]) -> String {
    let params = node
        .child_by_field_name("parameters")
        .map(|list| parameter_types(list, bytes).join(", "))
        .unwrap_or_default();
    let result = match node.child_by_field_name("result") {
        Some(result) if result.kind() == "parameter_list" => {
            let types = parameter_types(result, bytes);
            if types.is_empty() {
                String::new()
            } else {
                format!(" ({})", types.join(", "))
            }
        }
        Some(result) => {
            let text = normalize_line(&node_text(result, bytes));
            if text.is_empty() {
                String::new()
            } else {
                format!(" {text}")
            }
        }
        None => String::new(),
    };
    format!("func({params}){result}")
}

/// Declared types of a Go `parameter_list`, in source order.
fn parameter_types(list: Node<'_>, bytes: &[u8]) -> Vec<String> {
    let mut types = Vec::new();
    let mut cursor = list.walk();
    for declaration in list.named_children(&mut cursor) {
        if declaration.kind() != "parameter_declaration" {
            continue;
        }
        if let Some(type_node) = declaration.child_by_field_name("type") {
            let text = normalize_line(&node_text(type_node, bytes));
            if !text.is_empty() {
                types.push(text);
            }
        }
    }
    types
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

/// Parse a Go comment block into a description, `@param` texts and `@tags` values.
fn parse_go_doc(raw: &str) -> DocInfo {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let mut line = line.trim();
        for prefix in ["//", "/*", "*/", "*"] {
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

/// Combine the contiguous `//` comments immediately above a declaration.
fn leading_go_comment(root: Node<'_>, declaration_start: usize, bytes: &[u8]) -> Option<String> {
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

/// Recursive half of [`leading_go_comment`]; only comments above the limit qualify.
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

/// Reduce a Go receiver or type name to the bare identifier used as an index key.
fn bare_go_type(text: &str) -> String {
    let mut name = text.trim();
    while let Some(rest) = name.strip_prefix('*') {
        name = rest.trim_start();
    }
    if let Some(index) = name.find('[') {
        name = &name[..index];
    }
    if let Some(index) = name.rfind('.') {
        name = &name[index + 1..];
    }
    name.trim().to_string()
}

/// Normalize Go-only type prefixes before handing a name to the shared resolver.
fn go_type_lookup_name(text: &str) -> String {
    let mut name = text.trim();
    loop {
        if let Some(rest) = name.strip_prefix('*') {
            name = rest.trim_start();
            continue;
        }
        if let Some(rest) = name.strip_prefix("[]") {
            name = rest.trim_start();
            continue;
        }
        break;
    }
    let bare = bare_go_type(name);
    if bare.is_empty() {
        name.trim().to_owned()
    } else {
        bare
    }
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

    /// A free function keeps its bare name, ordered parameters and single result.
    #[test]
    fn go_discovers_free_function_with_parameters_and_result() {
        let source = "// Echo returns its input.\nfunc Echo(text string) string { return text }";
        let found = discover(source, "echo.go");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Echo");
        assert_eq!(found[0].qualified_name, "Echo");
        assert_eq!(found[0].kind, FnKind::Declaration);
        assert_eq!(found[0].class_name, None);
        assert_eq!(found[0].line, 2);
        assert_eq!(found[0].parameters.len(), 1);
        assert_eq!(found[0].parameters[0].name, "text");
        assert_eq!(found[0].parameters[0].type_, "string");
        assert!(found[0].parameters[0].required);
        assert_eq!(found[0].returns.type_, "string");
        assert_eq!(
            found[0].doc_description.as_deref(),
            Some("Echo returns its input.")
        );
        assert!(found[0].source_code.starts_with("func Echo"));
    }

    /// A pointer receiver is stripped so the method is indexed by its type name.
    #[test]
    fn go_discovers_method_with_pointer_receiver() {
        let source = "type Service struct{}\nfunc (s *Service) Run(n int) int { return n }";
        let found = discover(source, "service.go");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Run");
        assert_eq!(found[0].qualified_name, "Service.Run");
        assert_eq!(found[0].kind, FnKind::Method);
        assert_eq!(found[0].class_name.as_deref(), Some("Service"));
        assert_eq!(found[0].parameters.len(), 1);
    }

    /// Grouped parameter names and named results are expanded in source order.
    #[test]
    fn go_expands_grouped_parameters_and_named_results() {
        let source = "func merge(a, b int) (total int, err error) { return a + b, nil }";
        let found = discover(source, "merge.go");
        assert_eq!(found.len(), 1);
        let names: Vec<&str> = found[0]
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        assert_eq!(names, ["a", "b"]);
        assert!(found[0]
            .parameters
            .iter()
            .all(|parameter| parameter.type_ == "int"));
        assert_eq!(found[0].returns.type_, "(int, error)");
        let fields: Vec<(&str, &str)> = found[0]
            .returns
            .fields
            .iter()
            .map(|field| (field.name.as_str(), field.type_.as_str()))
            .collect();
        assert_eq!(fields, [("total", "int"), ("err", "error")]);
    }

    /// Structs, interfaces and aliases form one BTreeMap-shaped type index.
    #[test]
    fn go_collect_types_indexes_struct_interface_and_alias() {
        let source = r#"package p
type Item struct { id int; label string }
type Reader interface { Read() (int, error) }
type Alias Item
"#;
        let types = collect_types(source);
        let item = types.get("Item").expect("struct must be indexed");
        assert_eq!(item.fields[0].name, "id");
        assert_eq!(item.fields[0].type_, "int");
        assert_eq!(item.fields[1].name, "label");
        let reader = types.get("Reader").expect("interface must be indexed");
        assert_eq!(reader.fields[0].name, "Read");
        assert_eq!(reader.fields[0].type_, "func() (int, error)");
        let alias = types.get("Alias").expect("alias must be indexed");
        assert_eq!(alias.fields, item.fields);
    }

    /// Function literals inside a body are not discovered as separate callables.
    #[test]
    fn go_ignores_nested_function_literals() {
        let source = "func outer() { inner := func() { }; _ = inner }";
        let found = discover(source, "nested.go");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "outer");
    }
}
