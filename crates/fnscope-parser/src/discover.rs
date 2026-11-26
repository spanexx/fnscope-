//! Deterministic discovery of module-scoped functions and class methods.
//!
//! The pass walks only the shapes that can legally be a callable export or class member. It
//! never descends into a function body, so a nested helper is never mistaken for a top-level
//! function. Everything is derived from source text and the tree-sitter AST — no heuristics
//! that vary between runs.

use fnscope_model::{Parameter, ReturnField, ReturnShape};
use std::collections::{BTreeMap, HashMap};
use tree_sitter::{Node, Parser, Tree};

use crate::types::{DiscoveredFn, FnKind, Lang};

/// Parse one source file with the grammar selected by `lang`, or `None` on failure.
fn parse_tree(lang: Lang, source: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = match lang {
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Lang::JavaScript | Lang::Jsx => tree_sitter_javascript::LANGUAGE.into(),
        Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
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
        if lang == Lang::Rust {
            visit_rust_top_level(child, bytes, &types, &mut found);
        } else {
            visit_top_level(child, bytes, &types, &mut found);
        }
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
/// return fields for types declared elsewhere with [`resolve_return_fields`].
pub fn collect_types(lang: Lang, source: &str) -> BTreeMap<String, ReturnShape> {
    let tree = match parse_tree(lang, source) {
        Some(tree) => tree,
        None => return BTreeMap::new(),
    };
    TypeIndex::build(tree.root_node(), source.as_bytes()).named_map()
}

/// Fills empty return fields by resolving a declared type name through a project-wide type index.
///
/// The declared `returns.type_` is left untouched; only an empty `fields` list is replaced.
pub fn resolve_return_fields(found: &mut DiscoveredFn, types: &BTreeMap<String, ReturnShape>) {
    if !found.returns.fields.is_empty() {
        return;
    }
    found.returns.fields = resolve_fields_from_map(&found.returns.type_, types);
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

/// Look up return fields, first by exact declared text (inline object types), then by name.
fn resolve_fields_from_map(type_: &str, types: &BTreeMap<String, ReturnShape>) -> Vec<ReturnField> {
    if let Some(shape) = types.get(type_.trim()) {
        return shape.fields.clone();
    }
    if let Some(shape) = types.get(type_lookup_name(type_)) {
        return shape.fields.clone();
    }
    // Rust wrappers hide the payload type: report the declared text but inspect the payload.
    for name in rust_wrapper_candidates(type_) {
        if let Some(shape) = types.get(&name) {
            return shape.fields.clone();
        }
    }
    Vec::new()
}

/// Payload type names hidden inside `Result<T, E>`, `Option<T>` and `Vec<T>`.
fn rust_wrapper_candidates(type_: &str) -> Vec<String> {
    let text = type_.trim();
    let open = match text.find('<') {
        Some(index) => index,
        None => return Vec::new(),
    };
    let close = match text.rfind('>') {
        Some(index) if index > open => index,
        _ => return Vec::new(),
    };
    let wrapper = strip_rust_generics(&text[..open]).to_ascii_lowercase();
    if !matches!(wrapper.as_str(), "result" | "option" | "vec") {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    for arg in split_generic_args(&text[open + 1..close]) {
        for name in rust_wrapper_candidates(arg)
            .into_iter()
            .chain(Some(arg.to_string()))
        {
            let bare = strip_rust_generics(&name);
            if !bare.is_empty() && !candidates.contains(&bare) {
                candidates.push(bare);
            }
        }
    }
    candidates
}

/// Split generic arguments on top-level commas, ignoring commas nested in other generics.
fn split_generic_args(args: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in args.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&args[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&args[start..]);
    parts
}

/// Reduce a declared type to the bare name used as a key in the type index.
fn type_lookup_name(type_: &str) -> &str {
    let mut name = unwrap_promise(type_.trim());
    if let Some(generic_start) = name.find('<') {
        name = name[..generic_start].trim();
    }
    if let Some(dot) = name.rfind('.') {
        name = name[dot + 1..].trim();
    }
    name
}

/// Unwrap `Promise<T>` so a local type `T` can be inspected for fields.
///
/// The reported `ReturnShape::type_` keeps the full declared text (for example
/// `Promise<Role>`); only field lookup uses the unwrapped name.
fn unwrap_promise(type_: &str) -> &str {
    let type_ = type_.trim();
    if let Some(inner) = type_.strip_prefix("Promise<") {
        if let Some(inner) = inner.strip_suffix('>') {
            return inner.trim();
        }
    }
    type_
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

#[cfg(test)]
mod tests {
    use super::*;

    fn one(lang: Lang, source: &str) -> DiscoveredFn {
        let found = discover(lang, source, "src/example.ts");
        assert_eq!(
            found.len(),
            1,
            "expected exactly one function in: {}",
            source
        );
        found.into_iter().next().unwrap()
    }

    fn assert_num(actual: &Option<serde_json::Value>, expected: i64) {
        assert_eq!(actual, &Some(serde_json::Value::from(expected)));
    }

    #[test]
    fn declaration_with_required_and_optional_params_and_return_type() {
        let f = one(
            Lang::TypeScript,
            "export function foo(a: string, b?: number): boolean { return true; }",
        );
        assert_eq!(f.name, "foo");
        assert_eq!(f.qualified_name, "foo");
        assert_eq!(f.kind, FnKind::Declaration);
        assert_eq!(f.line, 1);
        assert_eq!(f.returns.type_, "boolean");

        assert_eq!(f.parameters.len(), 2);
        assert_eq!(f.parameters[0].name, "a");
        assert!(f.parameters[0].required);
        assert_eq!(f.parameters[0].type_, "string");
        assert_eq!(f.parameters[1].name, "b");
        assert!(!f.parameters[1].required);
        assert_eq!(f.parameters[1].type_, "number");
    }

    #[test]
    fn object_typed_parameter_with_optional_property_is_still_required() {
        let source = r#"
export async function evaluateGrammarRole(input: {
  sentence: string;
  targetWord: string;
  includeTeachingLayer?: boolean;
}): Promise<RoleEvaluationResult> { return Promise.resolve({}); }
"#;
        let f = one(Lang::TypeScript, source);
        assert_eq!(f.parameters.len(), 1);
        assert_eq!(f.parameters[0].name, "input");
        assert!(f.parameters[0].required);
        assert!(f.is_async);

        let optional = one(Lang::TypeScript, "export function f(b?: number) {}");
        assert!(!optional.parameters[0].required);
    }

    #[test]
    fn arrow_with_default_value() {
        let f = one(
            Lang::TypeScript,
            "export const add = (a: number, b: number = 2) => a + b;",
        );
        assert_eq!(f.name, "add");
        assert_eq!(f.kind, FnKind::Arrow);
        assert_eq!(f.parameters.len(), 2);
        assert_eq!(f.parameters[1].name, "b");
        assert!(!f.parameters[1].required);
        assert_num(&f.parameters[1].default_value, 2);
    }

    #[test]
    fn class_method_records_qualified_name_and_class() {
        let f = one(
            Lang::TypeScript,
            "class Service { public run(x: string) { return x; } }",
        );
        assert_eq!(f.name, "run");
        assert_eq!(f.qualified_name, "Service.run");
        assert_eq!(f.class_name.as_deref(), Some("Service"));
        assert_eq!(f.kind, FnKind::Method);
        assert_eq!(f.parameters[0].name, "x");
        assert_eq!(f.parameters[0].type_, "string");
    }

    #[test]
    fn function_expression_initializer_is_arrow() {
        let f = one(
            Lang::TypeScript,
            "export const make = function (a: number) { return a; };",
        );
        assert_eq!(f.name, "make");
        assert_eq!(f.kind, FnKind::Arrow);
    }

    #[test]
    fn class_field_arrow_function_is_method() {
        let f = one(
            Lang::TypeScript,
            "class Handler { handle = (x: string) => x; }",
        );
        assert_eq!(f.name, "handle");
        assert_eq!(f.qualified_name, "Handler.handle");
        assert_eq!(f.class_name.as_deref(), Some("Handler"));
        assert_eq!(f.kind, FnKind::Method);
    }

    #[test]
    fn jsdoc_description_params_and_tags_are_captured() {
        let source = r#"/**
 * Adds two numbers.
 * @param a the first number
 * @returns the sum
 * @tags nlp, pure
 */
export function add(a: number, b: number): number { return a + b; }
"#;
        let f = one(Lang::TypeScript, source);
        assert_eq!(f.doc_description.as_deref(), Some("Adds two numbers."));
        assert_eq!(f.tags, vec!["nlp".to_string(), "pure".to_string()]);
        assert_eq!(f.parameters[0].description, "the first number");
        assert!(f.parameters[1].description.is_empty());
    }

    #[test]
    fn promise_return_resolves_local_type_alias_fields() {
        let source = r#"
type Role = { id: string; label: string };
function loadRole(): Promise<Role> { return Promise.resolve({ id: "1", label: "admin" }); }
"#;
        let found = discover(Lang::TypeScript, source, "src/roles.ts");
        let f = found.iter().find(|f| f.name == "loadRole").unwrap();
        assert_eq!(f.returns.type_, "Promise<Role>");
        assert_eq!(f.returns.fields.len(), 2);
        assert_eq!(f.returns.fields[0].name, "id");
        assert_eq!(f.returns.fields[0].type_, "string");
        assert_eq!(f.returns.fields[1].name, "label");
        assert_eq!(f.returns.fields[1].type_, "string");
    }

    #[test]
    fn collect_types_finds_type_alias_fields() {
        let types = collect_types(
            Lang::TypeScript,
            "type Role = { roleId: string; label: string };",
        );
        let shape = types.get("Role").unwrap();
        assert_eq!(shape.fields.len(), 2);
        assert_eq!(shape.fields[0].name, "roleId");
        assert_eq!(shape.fields[0].type_, "string");
        assert_eq!(shape.fields[1].name, "label");
        assert_eq!(shape.fields[1].type_, "string");
    }

    #[test]
    fn resolve_return_fields_fills_promise_type_and_preserves_type_text() {
        let types = collect_types(
            Lang::TypeScript,
            "type Role = { roleId: string; label: string };",
        );
        let mut found = one(
            Lang::TypeScript,
            "export function load(): Promise<Role> { return Promise.resolve({}); }",
        );
        assert!(found.returns.fields.is_empty());
        resolve_return_fields(&mut found, &types);
        assert_eq!(found.returns.type_, "Promise<Role>");
        assert_eq!(found.returns.fields.len(), 2);
        assert_eq!(found.returns.fields[0].name, "roleId");
        assert_eq!(found.returns.fields[1].name, "label");
    }

    #[test]
    fn resolve_return_fields_keeps_existing_fields() {
        let types = collect_types(
            Lang::TypeScript,
            "type Role = { roleId: string; label: string };",
        );
        let source = r#"
type Local = { keep: number };
export function load(): Promise<Local> { return Promise.resolve({}); }
"#;
        let mut found = one(Lang::TypeScript, source);
        assert_eq!(found.returns.fields.len(), 1);
        assert_eq!(found.returns.fields[0].name, "keep");
        resolve_return_fields(&mut found, &types);
        assert_eq!(found.returns.fields.len(), 1);
        assert_eq!(found.returns.fields[0].name, "keep");
    }

    #[test]
    fn nested_functions_are_ignored_and_async_is_flagged() {
        let source = r#"
export async function outer(x: number) {
  function inner() { return x; }
  return inner();
}
"#;
        let found = discover(Lang::TypeScript, source, "src/a.ts");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "outer");
        assert!(found[0].is_async);
    }

    #[test]
    fn tsx_arrow_component_parses_without_panicking() {
        let found = discover(
            Lang::Tsx,
            "export const App = (name: string) => <div>Hello {name}</div>;",
            "src/App.tsx",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "App");
        assert_eq!(found[0].kind, FnKind::Arrow);
    }

    #[test]
    fn deterministic_order_and_line_scoped_duplicate_suppression() {
        let source = "function b() {}\nfunction a() {}\nfunction b() {}\n";
        let found = discover(Lang::JavaScript, source, "src/a.js");
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        // Dedup is keyed by (qualified_name, line), so same-name declarations on different lines are distinct.
        assert_eq!(names, vec!["b", "a", "b"]);
    }

    #[test]
    fn export_wrapper_does_not_double_count() {
        let found = discover(Lang::TypeScript, "export function once() {}", "src/a.ts");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "once");
    }

    #[test]
    fn rust_free_function_records_signature() {
        let f = one(Lang::Rust, "pub fn add(a: i32, b: i32) -> i32 { a + b }");
        assert_eq!(f.name, "add");
        assert_eq!(f.qualified_name, "add");
        assert_eq!(f.kind, FnKind::Declaration);
        assert_eq!(f.line, 1);
        assert_eq!(f.end_line, 1);
        assert_eq!(f.returns.type_, "i32");
        assert_eq!(f.parameters.len(), 2);
        assert_eq!(f.parameters[0].name, "a");
        assert_eq!(f.parameters[0].type_, "i32");
        assert!(f.parameters[0].required);
        assert!(f.source_code.starts_with("pub fn add"));
    }

    #[test]
    fn rust_impl_method_records_qualified_name_and_skips_self() {
        let f = one(
            Lang::Rust,
            "impl Service { pub fn add(&self, x: i32) -> i32 { x } }",
        );
        assert_eq!(f.name, "add");
        assert_eq!(f.qualified_name, "Service.add");
        assert_eq!(f.class_name.as_deref(), Some("Service"));
        assert_eq!(f.kind, FnKind::Method);
        assert_eq!(f.parameters.len(), 1);
        assert_eq!(f.parameters[0].name, "x");
        assert_eq!(f.parameters[0].type_, "i32");
    }

    #[test]
    fn rust_result_return_resolves_struct_fields() {
        let source = r#"
struct Label { id: i32, name: String }
struct AppError { code: String }
pub fn label(id: i32) -> Result<Label, AppError> { Ok(Label { id, name: "x".to_string() }) }
"#;
        let f = one(Lang::Rust, source);
        assert_eq!(f.returns.type_, "Result<Label, AppError>");
        assert_eq!(f.returns.fields.len(), 2);
        assert_eq!(f.returns.fields[0].name, "id");
        assert_eq!(f.returns.fields[0].type_, "i32");
        assert_eq!(f.returns.fields[1].name, "name");
        assert_eq!(f.returns.fields[1].type_, "String");
    }

    #[test]
    fn rust_doc_comment_captures_description_and_tags() {
        let source = r#"/// Adds two numbers.
/// @tags math, pure
pub fn add(a: i32, b: i32) -> i32 { a + b }
"#;
        let f = one(Lang::Rust, source);
        assert_eq!(f.doc_description.as_deref(), Some("Adds two numbers."));
        assert_eq!(f.tags, vec!["math".to_string(), "pure".to_string()]);
        assert_eq!(f.line, 3);
    }

    #[test]
    fn rust_collect_types_finds_struct_and_enum_fields() {
        let source = r#"
struct Label { id: i32, name: String }
enum AppError { NotFound, Invalid { code: String } }
"#;
        let types = collect_types(Lang::Rust, source);
        let label = types.get("Label").unwrap();
        assert_eq!(label.fields.len(), 2);
        assert_eq!(label.fields[1].name, "name");
        let error = types.get("AppError").unwrap();
        assert_eq!(error.fields.len(), 2);
        assert_eq!(error.fields[0].name, "NotFound");
        assert_eq!(error.fields[1].name, "Invalid");
    }

    #[test]
    fn rust_nested_function_is_ignored() {
        let source = r#"
pub fn outer(x: i32) -> i32 {
    fn inner(y: i32) -> i32 { y }
    inner(x)
}
"#;
        let found = discover(Lang::Rust, source, "src/lib.rs");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "outer");
    }

    #[test]
    fn rust_trait_signature_is_discovered() {
        let f = one(
            Lang::Rust,
            "trait Greet { fn greet(&self, name: &str) -> String; }",
        );
        assert_eq!(f.name, "greet");
        assert_eq!(f.kind, FnKind::Declaration);
        assert_eq!(f.returns.type_, "String");
        assert_eq!(f.parameters.len(), 1);
        assert_eq!(f.parameters[0].type_, "str");
    }
}
