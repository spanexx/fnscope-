//! Deterministic, AST-backed analysis of one C++ function body.
//!
//! The pass reads only the function located by the discovery record: it orders
//! top-level statements, records calls/guards/transforms/returns, extracts throw
//! codes and classifies effects. No clock, randomness or map iteration enters
//! the result, so the same input always produces the same `BodyAnalysis`.

use std::collections::BTreeSet;

use fnscope_model::{ErrorSpec, Parameter, Purity, Step, StepKind};
use tree_sitter::Node;

use super::discover::{
    declarator_identity, find_function_declarator, node_text, normalize_line, parse_tree, unquote,
};
use crate::types::{BodyAnalysis, DiscoveredFn};

/// Grammar keywords that must never be reported as referenced identifiers.
const NON_INPUT_WORDS: &[&str] = &[
    "auto",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "delete",
    "do",
    "double",
    "else",
    "false",
    "float",
    "for",
    "goto",
    "if",
    "int",
    "long",
    "namespace",
    "new",
    "nullptr",
    "NULL",
    "operator",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "sizeof",
    "static",
    "struct",
    "switch",
    "template",
    "this",
    "throw",
    "true",
    "try",
    "typedef",
    "typename",
    "using",
    "void",
    "volatile",
    "while",
];

/// Analyse one discovered C++ function and return deterministic execution facts.
///
/// A function node is located by exact source slice, with a line/name fallback.
/// When no node matches, the neutral [`BodyAnalysis::default`] is returned so a
/// partially parsed file cannot corrupt a manifest.
pub fn analyze(found: &DiscoveredFn, source: &str, known_names: &[String]) -> BodyAnalysis {
    let tree = match parse_tree(source) {
        Some(tree) => tree,
        None => return BodyAnalysis::default(),
    };
    let root = tree.root_node();
    let fn_node = match find_function_node(root, source, found) {
        Some(node) => node,
        None => return BodyAnalysis::default(),
    };
    let slice = node_text(fn_node, source.as_bytes());

    // Effects are read from the whole function text, not just its body, so a
    // guard or default expression in the header is also considered.
    let (purity, permissions, timeout_ms, runtime) = classify_cpp_effects(&slice);

    let body = function_body(fn_node);
    let scope = body.unwrap_or(fn_node);
    let error_sites = collect_error_sites(scope, source);
    let errors = build_errors(&error_sites);

    let mut analysis = BodyAnalysis {
        steps: Vec::new(),
        errors,
        purity,
        permissions,
        callee_names: Vec::new(),
        timeout_ms,
        runtime,
    };

    let statements = match body {
        Some(node) => top_statements(node),
        None => Vec::new(),
    };
    for (index, statement) in statements.iter().enumerate() {
        let (step, calls) = build_step(
            *statement,
            source,
            found,
            known_names,
            index + 1,
            &error_sites,
        );
        analysis.steps.push(step);
        analysis.callee_names.extend(calls);
    }

    analysis
}

// ---------------------------------------------------------------------------
// Function location and statement order
// ---------------------------------------------------------------------------

/// Whether a node kind can be the discovered C++ function itself.
fn is_function_kind(node: Node<'_>) -> bool {
    if node.kind() == "function_definition" {
        return true;
    }
    matches!(node.kind(), "field_declaration" | "declaration")
        && find_function_declarator(node).is_some()
}

/// Find the smallest C++ function node matching the discovery record.
fn find_function_node<'tree>(
    root: Node<'tree>,
    source: &str,
    found: &DiscoveredFn,
) -> Option<Node<'tree>> {
    let mut best: Option<Node<'tree>> = None;
    visit_function_nodes(root, source, found, &mut best);
    best
}

/// Recursive half of [`find_function_node`], keeping the tightest matching node.
fn visit_function_nodes<'tree>(
    node: Node<'tree>,
    source: &str,
    found: &DiscoveredFn,
    best: &mut Option<Node<'tree>>,
) {
    if is_function_kind(node) {
        let text = node_text(node, source.as_bytes());
        let exact = text == found.source_code || text.trim() == found.source_code.trim();
        let same_line = node.start_position().row + 1 == found.line;
        let same_name = find_function_declarator(node)
            .map(|declarator| declarator_identity(declarator, source.as_bytes()).0 == found.name)
            .unwrap_or(false);
        if exact || (same_line && same_name) {
            let better = best.map_or(true, |current| {
                node_range_len(node) < node_range_len(current)
            });
            if better {
                *best = Some(node);
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_function_nodes(child, source, found, best);
    }
}

/// Byte span of a node, used only as a deterministic tie-breaker.
fn node_range_len(node: Node<'_>) -> usize {
    node.end_byte().saturating_sub(node.start_byte())
}

/// Return the body block of a function definition, if it has one.
fn function_body(node: Node<'_>) -> Option<Node<'_>> {
    if let Some(body) = node.child_by_field_name("body") {
        if body.kind() == "compound_statement" {
            return Some(body);
        }
    }
    let mut cursor = node.walk();
    // Bind the consumer: a tail-expression iterator would outlive `cursor`.
    let block = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "compound_statement");
    block
}

/// Top-level statements of a compound body in source order, comments excluded.
///
/// Tree-sitter-cpp 0.23.4 has no `statement_list` wrapper, but the defensive
/// lookup keeps this pass correct if a future grammar reintroduces one.
fn top_statements<'tree>(body: Node<'tree>) -> Vec<Node<'tree>> {
    let container = {
        let mut cursor = body.walk();
        // Bind the consumer so the iterator temporary drops before `cursor`.
        let list = body
            .named_children(&mut cursor)
            .find(|child| child.kind() == "statement_list");
        list.unwrap_or(body)
    };
    let mut cursor = container.walk();
    let items: Vec<Node<'tree>> = container
        .named_children(&mut cursor)
        .filter(|child| child.kind() != "comment")
        .collect();
    items
}

// ---------------------------------------------------------------------------
// Step construction
// ---------------------------------------------------------------------------

/// Everything needed to describe one C++ statement, gathered in one AST walk.
struct StatementFacts {
    kind: StepKind,
    condition: String,
    callee: Option<String>,
    guard_error: Option<String>,
}

/// One assignment target and its declared or inferred display type.
struct Declaration {
    name: String,
    type_: Option<String>,
}

/// Build one ordered step plus the raw callee names it contains.
fn build_step(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
    known_names: &[String],
    index: usize,
    error_sites: &[ErrorSite],
) -> (Step, Vec<String>) {
    let code = normalize_line(&node_text(statement, source.as_bytes()));
    let calls = collect_calls(statement, source);
    let callee_names: Vec<String> = calls.iter().map(|(_, name)| name.clone()).collect();
    let kind = step_kind(statement, source);
    let declaration = first_declaration(statement, source, found);
    let condition = branch_condition(statement, source);
    let guard_error = error_sites
        .iter()
        .find(|site| site.byte >= statement.start_byte() && site.byte < statement.end_byte())
        .map(|site| site.code.clone());
    let callee = calls.first().map(|(_, name)| name.clone());
    let calls_fn_id = if kind == StepKind::Call {
        calls
            .iter()
            .find(|(_, name)| known_names.iter().any(|known| known == name))
            .map(|(_, name)| name.clone())
    } else {
        None
    };

    let output = match kind {
        StepKind::Return => {
            if found.returns.type_.trim().is_empty() {
                "void".to_owned()
            } else {
                found.returns.type_.clone()
            }
        }
        StepKind::Branch => match guard_error.as_deref() {
            Some(code) => format!("throws {code} | continues"),
            None => format!("{condition} | continues"),
        },
        StepKind::Call => declaration
            .as_ref()
            .and_then(|item| item.type_.clone())
            .unwrap_or_else(|| "void".to_owned()),
        StepKind::Transform => declaration
            .as_ref()
            .and_then(|item| item.type_.clone())
            .or_else(|| declaration.as_ref().map(|item| item.name.clone()))
            .unwrap_or_else(|| "value".to_owned()),
    };

    let facts = StatementFacts {
        kind,
        condition,
        callee,
        guard_error,
    };
    let (comment_title, comment_detail) =
        preceding_comments(source, statement.start_position().row);
    let title =
        comment_title.unwrap_or_else(|| fallback_title(&facts, declaration.as_ref(), &output));
    let detail =
        comment_detail.unwrap_or_else(|| fallback_detail(&facts, declaration.as_ref(), &output));

    let step = Step {
        id: format!("step-{index}"),
        kind: facts.kind,
        calls_fn_id,
        title,
        code,
        inputs: collect_inputs(statement, source),
        output,
        detail,
    };
    (step, callee_names)
}

/// Classify a statement into the four contract step kinds.
fn step_kind(statement: Node<'_>, source: &str) -> StepKind {
    match statement.kind() {
        "return_statement" => StepKind::Return,
        "if_statement" | "switch_statement" | "try_statement" => StepKind::Branch,
        _ => {
            if collect_calls(statement, source).is_empty() {
                StepKind::Transform
            } else {
                StepKind::Call
            }
        }
    }
}

/// Default title when no annotated comment precedes the statement.
fn fallback_title(
    facts: &StatementFacts,
    declaration: Option<&Declaration>,
    output: &str,
) -> String {
    match facts.kind {
        StepKind::Call => format!("Call {}()", facts.callee.as_deref().unwrap_or("unknown")),
        StepKind::Branch => {
            let label = match facts.guard_error.as_deref() {
                Some(code) => format!("throws {code}"),
                None => facts.condition.clone(),
            };
            format!("Guard: {}", truncate(&label, 48))
        }
        StepKind::Return => format!("Return {output}"),
        StepKind::Transform => format!(
            "Transform {}",
            declaration
                .map(|item| item.name.as_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("value")
        ),
    }
}

/// Default detail when no second comment line precedes the statement.
fn fallback_detail(
    facts: &StatementFacts,
    declaration: Option<&Declaration>,
    output: &str,
) -> String {
    match facts.kind {
        StepKind::Call => format!("Calls {}().", facts.callee.as_deref().unwrap_or("unknown")),
        StepKind::Branch => {
            let label = match facts.guard_error.as_deref() {
                Some(code) => format!("throws {code}"),
                None => facts.condition.clone(),
            };
            format!("Checks {label} and may exit early.")
        }
        StepKind::Return => format!("Returns {output} to the caller."),
        StepKind::Transform => format!(
            "Assigns {} from the expression.",
            declaration
                .map(|item| item.name.as_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("value")
        ),
    }
}

/// Shorten a condition for human-readable labels without cutting mid-code badly.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

// ---------------------------------------------------------------------------
// Calls and inputs
// ---------------------------------------------------------------------------

/// All call expressions below a node, as `(start byte, bare callee name)` pairs.
fn collect_calls(node: Node<'_>, source: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    visit_calls(node, source, &mut found);
    found.sort_by_key(|(byte, _)| *byte);
    found
}

/// Recursive half of [`collect_calls`], including stream writes and constructors.
fn visit_calls(node: Node<'_>, source: &str, out: &mut Vec<(usize, String)>) {
    if node.kind() == "call_expression" {
        if let Some(name) = callee_name(node, source) {
            out.push((node.start_byte(), name));
        }
    } else if node.kind() == "binary_expression" {
        if let Some(name) = stream_callee(node, source) {
            out.push((node.start_byte(), name));
        }
    } else if node.kind() == "init_declarator" {
        if let Some(value) = node.child_by_field_name("value") {
            if value.kind() == "argument_list" {
                let name = node
                    .parent()
                    .and_then(|parent| parent.child_by_field_name("type"))
                    .map(|type_node| {
                        last_identifier(type_node, source).unwrap_or_else(|| {
                            normalize_line(&node_text(type_node, source.as_bytes()))
                        })
                    })
                    .unwrap_or_default();
                if !name.is_empty() {
                    out.push((node.start_byte(), name));
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_calls(child, source, out);
    }
}

/// Bare name of a call target: the last identifier of `a.b.c(...)` is `c`.
fn callee_name(call: Node<'_>, source: &str) -> Option<String> {
    let target = call
        .child_by_field_name("function")
        .or_else(|| call.named_child(0))?;
    last_identifier(target, source)
}

/// Last identifier below a subtree, used for qualified call and stream targets.
fn last_identifier(node: Node<'_>, source: &str) -> Option<String> {
    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "namespace_identifier" | "type_identifier"
    ) {
        let text = node_text(node, source.as_bytes());
        if !text.trim().is_empty() {
            return Some(text.trim().to_owned());
        }
    }
    if matches!(node.kind(), "comment" | "string_literal" | "char_literal") {
        return None;
    }
    let mut cursor = node.walk();
    let mut found = None;
    for child in node.named_children(&mut cursor) {
        if let Some(name) = last_identifier(child, source) {
            found = Some(name);
        }
    }
    found
}

/// Recognize `std::cout << ...` as a side-effecting call-like step.
fn stream_callee(node: Node<'_>, source: &str) -> Option<String> {
    let operator = node.child_by_field_name("operator")?;
    if node_text(operator, source.as_bytes()).trim() != "<<" {
        return None;
    }
    let left = node.child_by_field_name("left")?;
    let name = last_identifier(left, source)?;
    matches!(name.as_str(), "cout" | "cerr" | "clog").then_some(name)
}

/// Referenced names of a statement, de-duplicated in first-seen source order.
fn collect_inputs(statement: Node<'_>, source: &str) -> Vec<String> {
    let mut ordered = Vec::new();
    visit_inputs(statement, source, &mut ordered);
    ordered.sort_by_key(|(byte, _)| *byte);
    let mut names = Vec::new();
    for (_, name) in ordered {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// Recursive half of [`collect_inputs`]; declarations contribute only their values.
fn visit_inputs(node: Node<'_>, source: &str, out: &mut Vec<(usize, String)>) {
    match node.kind() {
        "comment"
        | "string_literal"
        | "char_literal"
        | "number_literal"
        | "primitive_type"
        | "placeholder_type_specifier"
        | "type_descriptor"
        | "type_qualifier"
        | "type_identifier"
        | "namespace_identifier"
        | "template_type"
        | "trailing_return_type"
        | "operator_name"
        | "destructor_name" => return,
        "qualified_identifier" => {
            let path = normalize_line(&node_text(node, source.as_bytes())).replace(' ', "");
            if !path.is_empty() {
                out.push((node.start_byte(), path));
            }
            return;
        }
        "declaration" => {
            if let Some(value) = node.child_by_field_name("value") {
                visit_inputs(value, source, out);
            }
            let mut cursor = node.walk();
            for declarator in node.children_by_field_name("declarator", &mut cursor) {
                if let Some(value) = declarator.child_by_field_name("value") {
                    visit_inputs(value, source, out);
                }
            }
            return;
        }
        "identifier" | "field_identifier" => {
            let name = node_text(node, source.as_bytes());
            if !name.is_empty() && !NON_INPUT_WORDS.contains(&name.as_str()) {
                out.push((node.start_byte(), name));
            }
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_inputs(child, source, out);
    }
}

// ---------------------------------------------------------------------------
// Conditions and declarations
// ---------------------------------------------------------------------------

/// The condition of a branch, or the switched value when no condition is named.
fn branch_condition(statement: Node<'_>, source: &str) -> String {
    if let Some(condition) = statement.child_by_field_name("condition") {
        let value = condition.child_by_field_name("value").unwrap_or(condition);
        let text = normalize_line(&node_text(value, source.as_bytes()));
        if !text.is_empty() {
            return text;
        }
    }
    "condition".to_owned()
}

/// First assignment target and its declared or inferred display type.
fn first_declaration(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
) -> Option<Declaration> {
    match statement.kind() {
        "declaration" => {
            let mut cursor = statement.walk();
            let init = statement
                .children_by_field_name("declarator", &mut cursor)
                .find(|declarator| declarator.kind() == "init_declarator")?;
            let name = init
                .child_by_field_name("declarator")
                .and_then(|declarator| last_identifier(declarator, source))
                .unwrap_or_default();
            if name.is_empty() {
                return None;
            }
            let type_ = statement
                .child_by_field_name("type")
                .map(|type_node| normalize_line(&node_text(type_node, source.as_bytes())))
                .or_else(|| {
                    init.child_by_field_name("value")
                        .map(|value| infer_cpp_type(&node_text(value, source.as_bytes()), found))
                });
            Some(Declaration { name, type_ })
        }
        "expression_statement" => {
            let call = find_first_call(statement)?;
            Some(Declaration {
                name: callee_name(call, source).unwrap_or_default(),
                type_: None,
            })
        }
        _ => None,
    }
}

/// Conservative display type inferred from a literal or a declared parameter.
fn infer_cpp_type(text: &str, found: &DiscoveredFn) -> String {
    let text = text.trim();
    if text.starts_with('"') || text.starts_with('\'') {
        return "string".to_owned();
    }
    match text {
        "true" | "false" => return "bool".to_owned(),
        "NULL" | "nullptr" => return "pointer".to_owned(),
        _ => {}
    }
    if !text.is_empty() && text.chars().all(|ch| ch.is_ascii_digit()) {
        return "int".to_owned();
    }
    if text.parse::<f64>().is_ok() {
        return "float".to_owned();
    }
    parameter_type(text, &found.parameters).unwrap_or_else(|| "unknown".to_owned())
}

/// Look up a declared parameter type for a simple identifier-to-identifier copy.
fn parameter_type(name: &str, parameters: &[Parameter]) -> Option<String> {
    parameters
        .iter()
        .find(|parameter| parameter.name == name && !parameter.type_.trim().is_empty())
        .map(|parameter| parameter.type_.clone())
}

// ---------------------------------------------------------------------------
// Comments
// ---------------------------------------------------------------------------

/// Top two lines of the contiguous `//` block directly above a statement.
fn preceding_comments(source: &str, statement_row: usize) -> (Option<String>, Option<String>) {
    let lines: Vec<&str> = source.lines().collect();
    let mut block = Vec::new();
    let mut row = statement_row;
    while row > 0 {
        row -= 1;
        let line = lines.get(row).copied().unwrap_or("").trim();
        match line.strip_prefix("//") {
            Some(comment) => block.push(comment.trim().to_owned()),
            None => break,
        }
    }
    block.reverse();
    let title = block.first().map(|line| strip_numbering(line));
    let detail = block.get(1).cloned();
    (title, detail)
}

/// Remove a leading enumerator such as `1.` or `2)` from a comment line.
fn strip_numbering(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index > 0 && index + 1 < bytes.len() && (bytes[index] == b'.' || bytes[index] == b')') {
        let rest = text[index + 1..].trim_start();
        if !rest.is_empty() {
            return rest.to_owned();
        }
    }
    text.to_owned()
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// One throw site reduced to the facts needed by `ErrorSpec` and branches.
struct ErrorSite {
    byte: usize,
    code: String,
    when: String,
    status: Option<u16>,
}

/// Walk every descendant of the body and record throw sites in source order.
fn collect_error_sites(body: Node<'_>, source: &str) -> Vec<ErrorSite> {
    let mut sites = Vec::new();
    visit_error_sites(body, source, &mut sites);
    sites.sort_by_key(|site| site.byte);
    sites
}

/// Recursive half of [`collect_error_sites`].
fn visit_error_sites(node: Node<'_>, source: &str, out: &mut Vec<ErrorSite>) {
    if node.kind() == "throw_statement" {
        if let Some(site) = cpp_throw_site(node, source) {
            out.push(site);
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_error_sites(child, source, out);
    }
}

/// First call expression below a node, used to read a thrown error's payload.
fn find_first_call(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "call_expression" {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = find_first_call(child) {
            return Some(found);
        }
    }
    None
}

/// Code carried by `throw X("CODE")` or `throw std::runtime_error("CODE")`.
fn cpp_throw_site(node: Node<'_>, source: &str) -> Option<ErrorSite> {
    let call = find_first_call(node)?;
    let arguments = call.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let first = arguments.named_children(&mut cursor).next()?;
    if first.kind() != "string_literal" {
        return None;
    }
    let code = unquote(&node_text(first, source.as_bytes()));
    if code.is_empty() {
        return None;
    }
    Some(ErrorSite {
        byte: node.start_byte(),
        code: code.clone(),
        when: enclosing_condition(node, source),
        status: status_for_code(&code),
    })
}

/// Nearest enclosing `if` condition, or a deterministic fallback.
fn enclosing_condition(node: Node<'_>, source: &str) -> String {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "if_statement" {
            if let Some(clause) = parent.child_by_field_name("condition") {
                let value = clause.child_by_field_name("value").unwrap_or(clause);
                let text = normalize_line(&node_text(value, source.as_bytes()));
                if !text.is_empty() {
                    return text;
                }
            }
        }
        current = parent.parent();
    }
    "explicit throw".to_owned()
}

/// Deduplicate throw sites by code, keeping the first occurrence and its status.
fn build_errors(sites: &[ErrorSite]) -> Vec<ErrorSpec> {
    let mut seen = BTreeSet::new();
    let mut errors = Vec::new();
    for site in sites {
        if !seen.insert(site.code.clone()) {
            continue;
        }
        errors.push(ErrorSpec {
            code: site.code.clone(),
            status: site.status,
            when: site.when.clone(),
        });
    }
    errors
}

/// HTTP-like hint attached to a known error code.
fn status_for_code(code: &str) -> Option<u16> {
    match code {
        "TOKEN_NOT_FOUND" | "VALIDATION_ERROR" | "INVALID_INPUT" => Some(422),
        "EMPTY_TEXT" | "EMPTY_SENTENCE" | "BAD_REQUEST" | "NEG" | "NEGATIVE_INPUT" => Some(400),
        "NOT_FOUND" => Some(404),
        "UNAUTHORIZED" => Some(401),
        "FORBIDDEN" => Some(403),
        _ => Some(500),
    }
}

// ---------------------------------------------------------------------------
// Purity, permissions and budgets
// ---------------------------------------------------------------------------

/// Derive C++ purity, permissions, timeout and runtime from a function's text.
fn classify_cpp_effects(slice: &str) -> (Purity, Vec<String>, u64, String) {
    let lower = slice.to_ascii_lowercase();
    let console = lower.contains("std::cout")
        || lower.contains("std::cerr")
        || lower.contains("std::clog")
        || lower.contains("printf(")
        || lower.contains("cout <<")
        || lower.contains("cerr <<");
    let fs = lower.contains("fstream") || lower.contains("fopen");
    let fs_write = lower.contains("std::ofstream")
        || lower.contains("ios::out")
        || lower.contains("\"w\"")
        || lower.contains("\"a\"");
    let network = lower.contains("curl_") || lower.contains("socket(");
    let env_keys = extract_env_keys(slice);

    let mut permissions = BTreeSet::new();
    if console {
        permissions.insert("console:out".to_owned());
    }
    if fs {
        permissions.insert("fs:read".to_owned());
        if fs_write {
            permissions.insert("fs:write".to_owned());
        }
    }
    if network {
        permissions.insert("net:outbound".to_owned());
    }
    for key in env_keys {
        permissions.insert(format!("secrets:read:{key}"));
    }

    let side_effect = !permissions.is_empty();
    let purity = if side_effect {
        Purity::SideEffect
    } else {
        Purity::Pure
    };
    let timeout_ms = if !side_effect {
        150
    } else if network {
        5000
    } else {
        1000
    };
    // Reuse the contract's runtime labels so core and the UI need no C++ branch.
    let runtime = if side_effect {
        "node20".to_owned()
    } else {
        "universal".to_owned()
    };
    (
        purity,
        permissions.into_iter().collect(),
        timeout_ms,
        runtime,
    )
}

/// Environment variable names read through `std::getenv("KEY")` or `getenv("KEY")`.
fn extract_env_keys(slice: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for needle in ["std::getenv(\"", "getenv(\""] {
        let mut offset = 0usize;
        while let Some(index) = slice[offset..].find(needle) {
            let start = offset + index + needle.len();
            if let Some(end) = slice[start..].find('"') {
                let key = &slice[start..start + end];
                if !key.is_empty() && !keys.contains(&key.to_owned()) {
                    keys.push(key.to_owned());
                }
                offset = start + end + 1;
            } else {
                break;
            }
        }
    }
    keys
}

// ---------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpp::discover::discover;

    /// Call, branch, transform-like declaration and return steps are ordered.
    #[test]
    fn cpp_analyzes_call_branch_stream_and_return_steps() {
        let source = r#"int normalize(int n) { return n + 1; }
int analyze(int n) {
  // Guard input.
  if (n < 0) {
    return 0;
  }
  int cleaned = normalize(n);
  std::cout << cleaned;
  return cleaned;
}
"#;
        let found = discover(source, "p.cpp");
        let analyze_fn = found
            .iter()
            .find(|item| item.name == "analyze")
            .expect("analyze must be discovered");
        let analysis = analyze_body(analyze_fn, source, &["normalize".to_owned()]);
        let kinds: Vec<StepKind> = analysis.steps.iter().map(|step| step.kind).collect();
        assert_eq!(
            kinds,
            [
                StepKind::Branch,
                StepKind::Call,
                StepKind::Call,
                StepKind::Return
            ]
        );
        assert_eq!(analysis.steps[0].title, "Guard input.");
        assert!(analysis.steps[1]
            .code
            .starts_with("int cleaned = normalize(n);"));
        assert_eq!(analysis.steps[1].calls_fn_id.as_deref(), Some("normalize"));
        assert_eq!(analysis.callee_names, ["normalize", "cout"]);
        assert_eq!(analysis.steps[3].output, "int");
    }

    /// A thrown string literal becomes a deduplicated code with status and `when`.
    #[test]
    fn cpp_extracts_throw_codes_with_status_and_when() {
        let source = r#"int check(int n) {
  if (n < 0) {
    throw std::runtime_error("NEGATIVE_INPUT");
  }
  if (n == 0) {
    throw MyError("NEGATIVE_INPUT");
  }
  return n;
}
"#;
        let found = discover(source, "p.cpp");
        let analysis = analyze_body(&found[0], source, &[]);
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "NEGATIVE_INPUT");
        assert_eq!(analysis.errors[0].status, Some(400));
        assert_eq!(analysis.errors[0].when, "n < 0");
        assert!(analysis.steps.iter().any(|step| {
            step.kind == StepKind::Branch && step.output == "throws NEGATIVE_INPUT | continues"
        }));
    }

    /// `std::cout <<` and `printf` map to console output and a side-effect budget.
    #[test]
    fn cpp_flags_console_writes_as_side_effects() {
        let source = "void report(int n) { std::cout << n; printf(\"done\"); }";
        let found = discover(source, "p.cpp");
        let analysis = analyze_body(&found[0], source, &[]);
        assert_eq!(analysis.purity, Purity::SideEffect);
        assert!(analysis
            .permissions
            .iter()
            .any(|item| item == "console:out"));
        assert_eq!(analysis.timeout_ms, 1000);
        assert_eq!(analysis.runtime, "node20");
    }

    /// Filesystem, network and secret reads map to the documented permissions.
    #[test]
    fn cpp_maps_fs_net_and_secret_effects() {
        let source = r#"int effects() {
  const char* k = std::getenv("API_KEY");
  std::fstream f("x");
  FILE* fp = fopen("x", "w");
  curl_easy_perform(NULL);
  socket(1, 2, 3);
  return 0;
}
"#;
        let found = discover(source, "p.cpp");
        let analysis = analyze_body(&found[0], source, &[]);
        assert_eq!(analysis.purity, Purity::SideEffect);
        assert!(analysis
            .permissions
            .contains(&"secrets:read:API_KEY".to_owned()));
        assert!(analysis.permissions.contains(&"fs:read".to_owned()));
        assert!(analysis.permissions.contains(&"fs:write".to_owned()));
        assert!(analysis.permissions.contains(&"net:outbound".to_owned()));
        assert_eq!(analysis.timeout_ms, 5000);
    }

    /// A bare throw statement is a Call step and still yields an error fact.
    #[test]
    fn cpp_classifies_throw_as_call_step() {
        let source = "void fail() { throw MyError(\"BAD\"); }";
        let found = discover(source, "p.cpp");
        let analysis = analyze_body(&found[0], source, &[]);
        assert_eq!(analysis.steps.len(), 1);
        assert_eq!(analysis.steps[0].kind, StepKind::Call);
        assert_eq!(analysis.callee_names, ["MyError"]);
        assert_eq!(analysis.errors[0].code, "BAD");
        assert_eq!(analysis.errors[0].when, "explicit throw");
    }

    /// Repeating an analysis on the same source produces byte-identical facts.
    #[test]
    fn cpp_analysis_is_deterministic_on_repeat() {
        let source = r#"int ping(int n) {
  if (n < 1) { return 0; }
  int out = n + 1;
  return out;
}
"#;
        let found = discover(source, "p.cpp");
        let first = analyze_body(&found[0], source, &[]);
        let second = analyze_body(&found[0], source, &[]);
        assert_eq!(first, second);
        assert_eq!(first.steps.len(), 3);
        assert_eq!(first.steps[0].id, "step-1");
        assert_eq!(first.steps[2].id, "step-3");
    }

    /// A pure function keeps the contract's default timeout and runtime labels.
    #[test]
    fn cpp_pure_function_keeps_default_budget() {
        let source = "int add(int a, int b) { return a + b; }";
        let found = discover(source, "p.cpp");
        let analysis = analyze_body(&found[0], source, &[]);
        assert_eq!(analysis.purity, Purity::Pure);
        assert!(analysis.permissions.is_empty());
        assert_eq!(analysis.timeout_ms, 150);
        assert_eq!(analysis.runtime, "universal");
    }

    /// Call the public entry point so the test exercises the shipped signature.
    fn analyze_body(found: &DiscoveredFn, source: &str, known_names: &[String]) -> BodyAnalysis {
        analyze(found, source, known_names)
    }
}
