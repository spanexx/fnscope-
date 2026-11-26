//! Deterministic discovery of C functions and record types.
//!
//! Only `function_definition` nodes directly under the translation unit become
//! callables. A prototype is a `declaration` wrapping a `function_declarator`,
//! and a nested definition lives inside a `compound_statement`, so neither is
//! visited: one implementation has one entry and no duplicate declarations.
//! `static` functions are ordinary `function_definition` nodes and are included.
//!
//! Wrapper nodes handled (verified in `tree-sitter-c-0.24.2/src/node-types.json`):
//! `pointer_declarator` and `parenthesized_declarator` wrap a
//! `function_declarator`, and `field_declaration_list` wraps struct/union
//! members. This grammar has no `statement_list` wrapper around body
//! statements; `body.rs` still tolerates one defensively.

use std::collections::{BTreeMap, HashMap};

use fnscope_model::{Parameter, ReturnField, ReturnShape};
use tree_sitter::{Node, Parser, Tree};

use crate::resolve::resolve_fields_from_map;
use crate::types::{DiscoveredFn, FnKind};

/// Declarator wrappers that can sit between a definition and its name.
const DECLARATOR_WRAPPERS: &[&str] = &[
    "pointer_declarator",
    "parenthesized_declarator",
    "attributed_declarator",
    "array_declarator",
];

/// Parse one C source file, or `None` when no tree can be produced.
fn parse_tree(source: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_c::LANGUAGE.into();
    parser.set_language(&language).ok()?;
    parser.parse(source, None)
}

/// Discover every top-level function in one C file, sorted by `(line, name)`.
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
        if child.kind() == "function_definition" {
            record_c_fn(child, bytes, &types, &mut found);
        }
    }
    found.sort_by(|a, b| (a.line, &a.qualified_name).cmp(&(b.line, &b.qualified_name)));
    found.dedup_by(|a, b| a.qualified_name == b.qualified_name && a.line == b.line);
    found
}

/// Build one `DiscoveredFn` from a C `function_definition`.
fn record_c_fn(
    node: Node<'_>,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
    out: &mut Vec<DiscoveredFn>,
) {
    let declarator = match node
        .child_by_field_name("declarator")
        .and_then(find_function_declarator)
    {
        Some(declarator) => declarator,
        None => return,
    };
    let name = declarator
        .child_by_field_name("declarator")
        .map(|inner| bare_identifier(inner, bytes))
        .unwrap_or_default();
    if name.is_empty() {
        return;
    }
    let doc = leading_c_comment(root_of(node), node.start_byte(), bytes)
        .map(|raw| parse_c_doc(&raw))
        .unwrap_or_default();
    let start = node.start_position();
    let end = node.end_position();

    out.push(DiscoveredFn {
        qualified_name: name.clone(),
        name,
        kind: FnKind::Declaration,
        line: start.row + 1,
        end_line: end.row + 1,
        source_code: node_text(node, bytes),
        parameters: extract_c_parameters(declarator, bytes, &doc),
        returns: extract_c_returns(node, declarator, bytes, types),
        doc_description: doc.description.clone(),
        tags: doc.tags.clone(),
        // C has no async keyword; a function is always a plain declaration.
        is_async: false,
        class_name: None,
    });
}

/// Descend declarator wrappers until the `function_declarator` carrying params.
fn find_function_declarator(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = node;
    loop {
        if current.kind() == "function_declarator" {
            return Some(current);
        }
        if !DECLARATOR_WRAPPERS.contains(&current.kind()) {
            return None;
        }
        current = current.child_by_field_name("declarator")?;
    }
}

/// Count pointer wrappers on the return declarator, e.g. `char *f()` has one.
fn declarator_pointer_depth(mut node: Node<'_>) -> usize {
    let mut depth = 0usize;
    loop {
        match node.kind() {
            "pointer_declarator" => depth += 1,
            "parenthesized_declarator" | "attributed_declarator" | "array_declarator" => {}
            _ => break,
        }
        match node.child_by_field_name("declarator") {
            Some(inner) => node = inner,
            None => break,
        }
    }
    depth
}

/// First identifier under a declarator, ignoring nested type/qualifier nodes.
fn bare_identifier(node: Node<'_>, bytes: &[u8]) -> String {
    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "type_identifier"
    ) {
        return normalize_line(&node_text(node, bytes));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let found = bare_identifier(child, bytes);
        if !found.is_empty() {
            return found;
        }
    }
    String::new()
}

/// Named parameters of a C `parameter_list`, in source order.
///
/// A bare `void` list and variadic `...` entries have no contract name and are
/// skipped. Pointer declarators contribute a `*` suffix to the declared type.
fn extract_c_parameters(declarator: Node<'_>, bytes: &[u8], doc: &DocInfo) -> Vec<Parameter> {
    let mut params = Vec::new();
    let list = match declarator.child_by_field_name("parameters") {
        Some(list) => list,
        None => return params,
    };
    let mut cursor = list.walk();
    for declaration in list.named_children(&mut cursor) {
        if declaration.kind() != "parameter_declaration" {
            continue;
        }
        let inner = declaration.child_by_field_name("declarator");
        let name = inner
            .map(|node| bare_identifier(node, bytes))
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let base = inner
            .map(|node| type_prefix(declaration, node, bytes))
            .unwrap_or_else(|| {
                declaration
                    .child_by_field_name("type")
                    .map(|node| normalize_line(&node_text(node, bytes)))
                    .unwrap_or_else(|| "unknown".to_owned())
            });
        let stars = inner.map(declarator_pointer_depth).unwrap_or(0);
        let type_ = if stars > 0 {
            format!("{base} {}", "*".repeat(stars))
        } else {
            base
        };
        params.push(Parameter {
            description: doc
                .param_descriptions
                .get(&name)
                .cloned()
                .unwrap_or_default(),
            name,
            type_,
            required: true,
            default_value: None,
        });
    }
    params
}

/// Declared return type, with fields resolved through the shared type index.
fn extract_c_returns(
    node: Node<'_>,
    declarator: Node<'_>,
    bytes: &[u8],
    types: &BTreeMap<String, ReturnShape>,
) -> ReturnShape {
    let base = type_prefix(node, declarator, bytes);
    if base.is_empty() {
        return ReturnShape::default();
    }
    let stars = declarator_pointer_depth(declarator);
    let type_ = if stars > 0 {
        format!("{base} {}", "*".repeat(stars))
    } else {
        base
    };
    let fields = resolve_fields_from_map(&c_type_lookup_name(&type_), types);
    ReturnShape { type_, fields }
}

/// Index named C structs, unions and typedefs for cross-file return fields.
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
    /// Walk the whole tree once so a type declared after its use still indexes.
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
            "struct_specifier" | "union_specifier" => self.record_specifier(node, bytes),
            "type_definition" => self.record_typedef(node, bytes),
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child, bytes);
        }
    }

    fn record_specifier(&mut self, node: Node<'_>, bytes: &[u8]) {
        let name = match node.child_by_field_name("name") {
            Some(name) => normalize_line(&node_text(name, bytes)),
            None => return,
        };
        if name.is_empty() {
            return;
        }
        let fields = self.fields_from_body(node, bytes);
        if !fields.is_empty() {
            self.named.insert(name, fields);
        }
    }

    /// Named members of a struct/union body, through the `field_declaration_list`.
    fn fields_from_body(&self, node: Node<'_>, bytes: &[u8]) -> Vec<ReturnField> {
        let mut fields = Vec::new();
        let container = match node.child_by_field_name("body") {
            Some(body) => body,
            None => return fields,
        };
        let mut cursor = container.walk();
        for member in container.named_children(&mut cursor) {
            if member.kind() != "field_declaration" {
                continue;
            }
            let type_ = match member.child_by_field_name("type") {
                Some(node) => normalize_line(&node_text(node, bytes)),
                None => continue,
            };
            if type_.is_empty() {
                continue;
            }
            let mut declarators = member.walk();
            for declarator in member.children_by_field_name("declarator", &mut declarators) {
                let name = bare_identifier(declarator, bytes);
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

    /// A typedef indexes either the inline record body or its named target.
    fn record_typedef(&mut self, node: Node<'_>, bytes: &[u8]) {
        let mut declarators = node.walk();
        let name = match node
            .children_by_field_name("declarator", &mut declarators)
            .next()
        {
            Some(declarator) => bare_identifier(declarator, bytes),
            None => return,
        };
        if name.is_empty() {
            return;
        }
        let target = match node.child_by_field_name("type") {
            Some(target) => target,
            None => return,
        };
        if matches!(target.kind(), "struct_specifier" | "union_specifier") {
            let fields = self.fields_from_body(target, bytes);
            if !fields.is_empty() {
                self.named.insert(name, fields);
                return;
            }
            if let Some(named) = target.child_by_field_name("name") {
                let named = normalize_line(&node_text(named, bytes));
                if !named.is_empty() {
                    self.aliases.insert(name, named);
                }
            }
            return;
        }
        let target_name = c_type_lookup_name(&normalize_line(&node_text(target, bytes)));
        if !target_name.is_empty() {
            self.aliases.insert(name, target_name);
        }
    }

    /// Resolve aliases after all declarations are indexed and drop empty entries.
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

/// Documentation facts read from the nearest contiguous leading comment.
#[derive(Default)]
struct DocInfo {
    description: Option<String>,
    param_descriptions: HashMap<String, String>,
    tags: Vec<String>,
}

/// Parse a C comment block into a description, `@param` texts and `@tags` values.
fn parse_c_doc(raw: &str) -> DocInfo {
    let mut lines = Vec::new();
    let mut in_block = false;
    for line in raw.lines() {
        let mut line = line.trim();
        if line.starts_with("/*") {
            in_block = true;
        }
        let closes_block = line.ends_with("*/");
        for prefix in ["//", "/*", "*/", "*"] {
            if let Some(rest) = line.strip_prefix(prefix) {
                line = rest.trim();
            }
        }
        // A single-line `/* text */` keeps its closing delimiter after the
        // leading prefixes are removed; a block's last text line can too.
        if in_block {
            if let Some(rest) = line.strip_suffix("*/") {
                line = rest.trim();
            }
        }
        if closes_block {
            in_block = false;
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

/// Combine the contiguous `//` or `/* */` comments above a declaration.
fn leading_c_comment(root: Node<'_>, declaration_start: usize, bytes: &[u8]) -> Option<String> {
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

/// Recursive half of [`leading_c_comment`]; only comments above the limit qualify.
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

/// Reduce a C return type to the bare name used as a key in the type index.
fn c_type_lookup_name(type_: &str) -> String {
    let mut name = type_.trim();
    for prefix in ["struct ", "union ", "enum "] {
        if let Some(rest) = name.strip_prefix(prefix) {
            name = rest.trim_start();
            break;
        }
    }
    let name = name.trim_end_matches('*').trim();
    name.to_owned()
}

/// Declared type text before a declarator, keeping qualifiers but not storage.
fn type_prefix(node: Node<'_>, declarator: Node<'_>, bytes: &[u8]) -> String {
    if declarator.start_byte() <= node.start_byte() {
        return normalize_line(&node_text(node, bytes));
    }
    let raw = String::from_utf8_lossy(&bytes[node.start_byte()..declarator.start_byte()]);
    let words = raw
        .split_whitespace()
        .filter(|word| {
            !matches!(
                *word,
                "static" | "extern" | "register" | "inline" | "auto" | "_Thread_local"
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    words
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

    /// A top-level definition keeps its bare name, 1-based lines and doc text.
    #[test]
    fn c_discovers_top_level_function_with_doc() {
        let source = "/* add returns the sum of two values. */\nint add(int a, int b) {\n  return a + b;\n}\n";
        let found = discover(source, "add.c");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "add");
        assert_eq!(found[0].qualified_name, "add");
        assert_eq!(found[0].kind, FnKind::Declaration);
        assert_eq!(found[0].class_name, None);
        assert!(!found[0].is_async);
        assert_eq!(found[0].line, 2);
        assert_eq!(found[0].end_line, 4);
        assert_eq!(
            found[0].doc_description.as_deref(),
            Some("add returns the sum of two values.")
        );
        assert!(found[0].source_code.starts_with("int add("));
    }

    /// Named parameters, a pointer return and the bare `void` list are handled.
    #[test]
    fn c_extracts_parameters_and_return_type() {
        let source = r#"
// dup copies a string.
char *dup(const char *text, int count) {
  return 0;
}
int nothing(void) { return 0; }
"#;
        let found = discover(source, "dup.c");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "dup");
        assert_eq!(found[0].returns.type_, "char *");
        let names: Vec<&str> = found[0]
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        assert_eq!(names, ["text", "count"]);
        assert_eq!(found[0].parameters[0].type_, "const char *");
        assert_eq!(found[0].parameters[1].type_, "int");
        assert!(found[0].parameters.iter().all(|item| item.required));
        assert_eq!(found[1].name, "nothing");
        assert!(found[1].parameters.is_empty());
        assert_eq!(found[1].returns.type_, "int");
    }

    /// Structs, unions and typedefs index named fields through their wrappers.
    #[test]
    fn c_indexes_struct_union_and_typedef_fields() {
        let source = r#"
struct Point { int x; int y; };
union Value { int i; float f; };
typedef struct { int width; int height; } Size;
typedef struct Point Location;
"#;
        let types = collect_types(source);
        let point = types.get("Point").expect("struct must be indexed");
        assert_eq!(point.fields[0].name, "x");
        assert_eq!(point.fields[0].type_, "int");
        assert_eq!(point.fields[1].name, "y");
        let value = types.get("Value").expect("union must be indexed");
        assert_eq!(value.fields[0].name, "i");
        assert_eq!(value.fields[1].type_, "float");
        let size = types.get("Size").expect("typedef must be indexed");
        assert_eq!(size.fields[0].name, "width");
        assert_eq!(size.fields[1].name, "height");
        let location = types.get("Location").expect("alias must be indexed");
        assert_eq!(location.fields, point.fields);
    }

    /// Prototypes and nested definitions are not separate callables; same-file
    /// record fields resolve into a function's return shape during discovery.
    #[test]
    fn c_skips_prototypes_and_resolves_same_file_fields() {
        let source = r#"
typedef struct Point { int x; int y; } Point;
Point make(void);
Point make(void) { Point p; return p; }
"#;
        let found = discover(source, "make.c");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "make");
        assert_eq!(found[0].returns.type_, "Point");
        assert_eq!(found[0].returns.fields.len(), 2);
        assert_eq!(found[0].returns.fields[0].name, "x");
    }

    /// Multi-line block docs drop `*` decoration and the closing delimiter.
    #[test]
    fn c_parses_multiline_block_doc_without_delimiters() {
        let source = "/*\n * make builds a point.\n * @param x horizontal.\n * @tags geometry\n */\nPoint make(int x) { Point p; return p; }\n";
        let found = discover(source, "make.c");
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].doc_description.as_deref(),
            Some("make builds a point.")
        );
        assert_eq!(found[0].tags, ["geometry"]);
        assert_eq!(found[0].parameters[0].description, "horizontal.");
    }
}
