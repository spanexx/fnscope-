//! Deterministic, AST-backed analysis of one Swift function body.
//!
//! The pass reads only the source slice of a discovered function: it orders the
//! top-level statements, records calls/guards/transforms/returns, extracts thrown
//! error codes, and classifies effects. No randomness, clock or map iteration
//! enters the result, so the same input always produces the same `BodyAnalysis`.

use std::collections::BTreeSet;

use fnscope_model::{ErrorSpec, Purity, Step, StepKind};
use tree_sitter::{Node, Parser};

use crate::types::{BodyAnalysis, DiscoveredFn};

/// Grammar keywords that must never be reported as referenced identifiers.
const NON_INPUT_WORDS: &[&str] = &[
    "let",
    "var",
    "return",
    "throw",
    "throws",
    "if",
    "guard",
    "else",
    "switch",
    "case",
    "default",
    "do",
    "catch",
    "func",
    "init",
    "class",
    "struct",
    "actor",
    "extension",
    "enum",
    "protocol",
    "import",
    "inout",
    "some",
    "any",
    "true",
    "false",
    "nil",
    "self",
    "Self",
    "super",
    "try",
    "await",
    "as",
    "is",
    "in",
    "where",
    "while",
    "for",
    "repeat",
    "break",
    "continue",
    "defer",
    "public",
    "private",
    "internal",
    "fileprivate",
    "static",
    "final",
    "override",
    "mutating",
    "lazy",
    "weak",
    "unowned",
    "open",
];

/// Analyses one function body and returns deterministic execution facts.
///
/// The function node is located by its exact source slice, with a line/name
/// fallback. When no node matches, the neutral `BodyAnalysis::default()` is
/// returned so a partially discovered file cannot corrupt a manifest.
pub fn analyze(found: &DiscoveredFn, source: &str, known_names: &[String]) -> BodyAnalysis {
    let language: tree_sitter::Language = tree_sitter_swift::LANGUAGE.into();

    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return BodyAnalysis::default();
    }
    let tree = match parser.parse(source, None) {
        Some(tree) => tree,
        None => return BodyAnalysis::default(),
    };
    let root = tree.root_node();

    let fn_node = match find_function_node(root, source, found) {
        Some(node) => node,
        None => return BodyAnalysis::default(),
    };
    let slice = node_text(fn_node, source);

    // Effects are classified from the whole declaration so default values and
    // inline guards are considered as well as the body.
    let (purity, permissions, timeout_ms, runtime) = classify_swift_effects(&slice);

    let body = function_body(fn_node);
    let error_sites = collect_throw_sites(body.unwrap_or(fn_node), source);
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

    let statements: Vec<Node<'_>> = match body {
        Some(body) => {
            let mut cursor = body.walk();
            let found_statements = body
                .named_children(&mut cursor)
                .find(|child| child.kind() == "statements");
            match found_statements {
                Some(statements) => {
                    let mut inner = statements.walk();
                    statements
                        .named_children(&mut inner)
                        .filter(|child| !matches!(child.kind(), "comment" | "multiline_comment"))
                        .collect()
                }
                None => Vec::new(),
            }
        }
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
// Function location
// ---------------------------------------------------------------------------

/// Whether a node kind can be the discovered function itself.
fn is_function_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration" | "init_declaration" | "protocol_function_declaration"
    )
}

/// Finds the smallest matching function node for the discovery record.
fn find_function_node<'tree>(
    root: Node<'tree>,
    source: &str,
    found: &DiscoveredFn,
) -> Option<Node<'tree>> {
    let mut best: Option<Node<'tree>> = None;
    visit_function_nodes(root, source, found, &mut best);
    best
}

/// Recursive half of [`find_function_node`].
fn visit_function_nodes<'tree>(
    node: Node<'tree>,
    source: &str,
    found: &DiscoveredFn,
    best: &mut Option<Node<'tree>>,
) {
    if is_function_kind(node.kind()) {
        let text = node_text(node, source);
        let exact = text == found.source_code || text.trim() == found.source_code.trim();
        let same_line = node.start_position().row + 1 == found.line;
        let same_name = node
            .child_by_field_name("name")
            .map(|name| node_text(name, source))
            .map(|name| name == found.name || name == found.qualified_name)
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

/// Returns the `function_body` wrapper of a declaration, if present.
fn function_body(node: Node<'_>) -> Option<Node<'_>> {
    if let Some(body) = node.child_by_field_name("body") {
        return Some(body);
    }
    let mut cursor = node.walk();
    let found = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "function_body");
    found
}

// ---------------------------------------------------------------------------
// Step construction
// ---------------------------------------------------------------------------

/// Everything needed to describe one statement, gathered in one AST walk.
struct StatementFacts {
    kind: StepKind,
    code: String,
    inputs: Vec<String>,
    assigned_name: Option<String>,
    declared_type: Option<String>,
    condition: String,
    callee: Option<String>,
    guard_error: Option<String>,
}

/// Builds one ordered step plus the raw callee names encountered in it.
fn build_step(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
    known_names: &[String],
    index: usize,
    error_sites: &[ThrowSite],
) -> (Step, Vec<String>) {
    let mut facts = statement_facts(statement, source, found, error_sites);
    let calls = collect_calls(statement, source);
    let callee_names: Vec<String> = calls.iter().map(|(_, name)| name.clone()).collect();
    let calls_fn_id = if facts.kind == StepKind::Call {
        calls
            .iter()
            .find(|(_, name)| known_names.iter().any(|known| known == name))
            .map(|(_, name)| name.clone())
    } else {
        None
    };
    if let Some(resolved) = calls_fn_id.as_deref() {
        facts.callee = Some(resolved.to_owned());
    }

    let output = match facts.kind {
        StepKind::Return => {
            if found.returns.type_.trim().is_empty() {
                "void".to_owned()
            } else {
                found.returns.type_.clone()
            }
        }
        StepKind::Branch => match facts.guard_error {
            Some(ref code) => format!("throws {} | continues", code),
            None => format!("{} | continues", facts.condition),
        },
        _ => facts
            .declared_type
            .clone()
            .unwrap_or_else(|| "void".to_owned()),
    };

    let (comment_title, comment_detail) =
        preceding_comments(source, statement.start_position().row);
    let title = comment_title.unwrap_or_else(|| fallback_title(&facts, &output));
    let detail = comment_detail.unwrap_or_else(|| fallback_detail(&facts, &output));

    let step = Step {
        id: format!("step-{}", index),
        kind: facts.kind,
        calls_fn_id,
        title,
        code: facts.code,
        inputs: facts.inputs,
        output,
        detail,
    };
    (step, callee_names)
}

/// Collects the deterministic fields of one top-level statement.
fn statement_facts(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
    error_sites: &[ThrowSite],
) -> StatementFacts {
    let code = normalize_line(&node_text(statement, source));
    let kind = match statement.kind() {
        "if_statement" | "guard_statement" | "switch_statement" | "do_statement" => {
            StepKind::Branch
        }
        "control_transfer_statement"
            if node_text(statement, source)
                .trim_start()
                .starts_with("return") =>
        {
            StepKind::Return
        }
        _ => {
            if has_call(statement) {
                StepKind::Call
            } else {
                StepKind::Transform
            }
        }
    };

    let assigned_name = assigned_name(statement, source);
    let declared_type = assigned_name
        .as_deref()
        .and_then(|name| declared_type(statement, source, found, name));

    let condition_node = statement
        .child_by_field_name("condition")
        .or_else(|| statement.child_by_field_name("expr"));
    let condition = condition_node
        .map(|node| normalize_line(&node_text(node, source)))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| {
            if statement.kind() == "do_statement" {
                "catch".to_owned()
            } else {
                "condition".to_owned()
            }
        });

    let calls = collect_calls(statement, source);
    let callee = calls.first().map(|(_, name)| name.clone());

    let guard_error = error_sites
        .iter()
        .find(|site| site.byte >= statement.start_byte() && site.byte < statement.end_byte())
        .map(|site| site.code.clone());

    StatementFacts {
        kind,
        code,
        inputs: collect_inputs(statement, source),
        assigned_name,
        declared_type,
        condition,
        callee,
        guard_error,
    }
}

/// Name written by a local declaration or assignment, if any.
fn assigned_name(statement: Node<'_>, source: &str) -> Option<String> {
    let node = if statement.kind() == "property_declaration" {
        statement.child_by_field_name("name")
    } else if statement.kind() == "assignment" {
        statement.child_by_field_name("target")
    } else {
        None
    }?;
    let text = normalize_line(&node_text(node, source));
    (!text.is_empty()).then_some(text)
}

/// Declared or conservatively inferred type label for a local binding.
fn declared_type(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
    name: &str,
) -> Option<String> {
    let annotated = {
        let mut cursor = statement.walk();
        let found = statement
            .named_children(&mut cursor)
            .find(|child| child.kind() == "type_annotation")
            .and_then(|annotation| {
                annotation
                    .child_by_field_name("type")
                    .map(|type_node| normalize_line(&node_text(type_node, source)))
            })
            .filter(|text| !text.is_empty());
        found
    };
    if let Some(annotation) = annotated {
        return Some(format!("{}: {}", name, annotation));
    }
    let value = statement
        .child_by_field_name("value")
        .or_else(|| statement.child_by_field_name("result"));
    let inferred = value
        .map(|node| infer_type(node, source, found))
        .unwrap_or_else(|| "unknown".to_owned());
    Some(format!("{}: {}", name, inferred))
}

/// Infer a display type from a literal, a known parameter or a call.
fn infer_type(value: Node<'_>, source: &str, found: &DiscoveredFn) -> String {
    match value.kind() {
        "line_string_literal" | "multi_line_string_literal" | "raw_string_literal" => {
            "String".to_owned()
        }
        "integer_literal" => "Int".to_owned(),
        "real_literal" => "Double".to_owned(),
        "boolean_literal" => "Bool".to_owned(),
        "nil_literal" => "nil".to_owned(),
        "simple_identifier" => {
            let text = node_text(value, source);
            found
                .parameters
                .iter()
                .find(|parameter| parameter.name == text && !parameter.type_.is_empty())
                .map(|parameter| parameter.type_.clone())
                .unwrap_or_else(|| "unknown".to_owned())
        }
        _ => "unknown".to_owned(),
    }
}

/// Default title when no annotated comment precedes the statement.
fn fallback_title(facts: &StatementFacts, output: &str) -> String {
    match facts.kind {
        StepKind::Call => format!("Call {}()", facts.callee.as_deref().unwrap_or("unknown")),
        StepKind::Branch => format!("Guard: {}", truncate(&facts.condition, 48)),
        StepKind::Return => format!("Return {}", output),
        StepKind::Transform => format!(
            "Transform {}",
            facts.assigned_name.as_deref().unwrap_or("value")
        ),
    }
}

/// Default detail when no second comment line precedes the statement.
fn fallback_detail(facts: &StatementFacts, output: &str) -> String {
    match facts.kind {
        StepKind::Call => format!("Calls {}().", facts.callee.as_deref().unwrap_or("unknown")),
        StepKind::Branch => format!(
            "Checks {} and may exit early.",
            truncate(&facts.condition, 48)
        ),
        StepKind::Return => format!("Returns {} to the caller.", output),
        StepKind::Transform => format!(
            "Assigns {} from the expression.",
            facts.assigned_name.as_deref().unwrap_or("value")
        ),
    }
}

/// Shorten a condition for human-readable labels without cutting mid-word badly.
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

/// Whether any call or constructor invocation occurs below this node.
fn has_call(node: Node<'_>) -> bool {
    if matches!(node.kind(), "call_expression" | "constructor_expression") {
        return true;
    }
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).any(|child| {
        !matches!(child.kind(), "function_declaration" | "lambda_literal") && has_call(child)
    });
    found
}

/// All call expressions below a node as `(start byte, bare callee)` pairs.
fn collect_calls(node: Node<'_>, source: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    visit_calls(node, source, &mut found);
    found.sort_by_key(|(byte, _)| *byte);
    found
}

/// Recursive half of [`collect_calls`].
fn visit_calls(node: Node<'_>, source: &str, out: &mut Vec<(usize, String)>) {
    if matches!(node.kind(), "call_expression" | "constructor_expression") {
        if let Some(callee) = callee_bare_name(node, source) {
            out.push((node.start_byte(), callee));
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        // A closure's calls belong to the closure, not the enclosing step.
        if matches!(child.kind(), "function_declaration" | "lambda_literal") {
            continue;
        }
        visit_calls(child, source, out);
    }
}

/// Bare name of a call target: the last identifier of `a.b.c(...)` is `c`.
fn callee_bare_name(call: Node<'_>, source: &str) -> Option<String> {
    let target = if call.kind() == "constructor_expression" {
        call.child_by_field_name("constructed_type")?
    } else {
        let mut cursor = call.walk();
        let target = call.named_children(&mut cursor).next()?;
        target
    };
    let mut identifiers = Vec::new();
    visit_identifiers(target, source, &mut identifiers);
    identifiers.pop()
}

/// Collect identifier texts inside a subtree, in source order.
fn visit_identifiers(node: Node<'_>, source: &str, out: &mut Vec<String>) {
    if matches!(
        node.kind(),
        "simple_identifier" | "type_identifier" | "field_identifier" | "identifier"
    ) {
        out.push(node_text(node, source));
        return;
    }
    if matches!(node.kind(), "comment" | "multiline_comment") {
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_identifiers(child, source, out);
    }
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

/// Recursive half of [`collect_inputs`]; declarations and type text are skipped.
fn visit_inputs(node: Node<'_>, source: &str, out: &mut Vec<(usize, String)>) {
    match node.kind() {
        "comment"
        | "multiline_comment"
        | "type_annotation"
        | "type_identifier"
        | "user_type"
        | "optional_type"
        | "array_type"
        | "dictionary_type"
        | "function_type"
        | "metatype"
        | "type_arguments"
        | "type_parameters"
        | "type_modifiers"
        | "parameter_modifiers"
        | "attribute"
        | "value_argument_label" => return,
        "property_declaration" => {
            if let Some(value) = node.child_by_field_name("value") {
                visit_inputs(value, source, out);
            }
            return;
        }
        "assignment" => {
            if let Some(result) = node.child_by_field_name("result") {
                visit_inputs(result, source, out);
            }
            return;
        }
        "navigation_expression" => {
            let path = normalize_line(&node_text(node, source)).replace(' ', "");
            if !path.is_empty() {
                out.push((node.start_byte(), path));
            }
            return;
        }
        "simple_identifier" | "field_identifier" => {
            let text = node_text(node, source);
            if !NON_INPUT_WORDS.contains(&text.as_str()) {
                out.push((node.start_byte(), text));
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
// Comments
// ---------------------------------------------------------------------------

/// Top two `//` lines of the contiguous block directly above a statement.
fn preceding_comments(source: &str, statement_row: usize) -> (Option<String>, Option<String>) {
    let lines: Vec<&str> = source.lines().collect();
    let mut block = Vec::new();
    let mut row = statement_row;
    while row > 0 {
        row -= 1;
        let line = lines.get(row).copied().unwrap_or("").trim();
        let comment = line
            .strip_prefix("///")
            .or_else(|| line.strip_prefix("//!"))
            .or_else(|| line.strip_prefix("//"));
        match comment {
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
struct ThrowSite {
    byte: usize,
    code: String,
    when: String,
}

/// Walk every descendant of the body and record throws in source order.
fn collect_throw_sites(body: Node<'_>, source: &str) -> Vec<ThrowSite> {
    let mut sites = Vec::new();
    visit_throws(body, source, &mut sites);
    sites.sort_by_key(|site| site.byte);
    sites
}

/// Recursive half of [`collect_throw_sites`].
fn visit_throws(node: Node<'_>, source: &str, out: &mut Vec<ThrowSite>) {
    if node.kind() == "control_transfer_statement" {
        let mut cursor = node.walk();
        let throws = node
            .named_children(&mut cursor)
            .any(|child| child.kind() == "throw_keyword");
        if throws {
            if let Some(code) = throw_code(node, source) {
                out.push(ThrowSite {
                    byte: node.start_byte(),
                    code,
                    when: enclosing_condition(node, source),
                });
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_throws(child, source, out);
    }
}

/// Error code carried by `throw X.code`, `throw Error(code: "CODE")` or a literal.
fn throw_code(control: Node<'_>, source: &str) -> Option<String> {
    let mut cursor = control.walk();
    let expression = control
        .named_children(&mut cursor)
        .find(|child| child.kind() != "throw_keyword")?;
    match expression.kind() {
        "call_expression" | "constructor_expression" => {
            labeled_code(expression, source).or_else(|| callee_bare_name(expression, source))
        }
        "navigation_expression" => {
            let mut identifiers = Vec::new();
            visit_identifiers(expression, source, &mut identifiers);
            identifiers.pop()
        }
        "line_string_literal" | "multi_line_string_literal" | "raw_string_literal" => {
            Some(unquote(&node_text(expression, source)))
        }
        "simple_identifier" => Some(node_text(expression, source)),
        _ => first_string_or_identifier(expression, source),
    }
}

/// Code from the `code:` argument of a thrown error object, if present.
fn labeled_code(node: Node<'_>, source: &str) -> Option<String> {
    if node.kind() == "value_argument" {
        let name = node
            .child_by_field_name("name")
            .map(|label| node_text(label, source))
            .unwrap_or_default();
        if name.trim() == "code" {
            if let Some(value) = node.child_by_field_name("value") {
                if matches!(
                    value.kind(),
                    "line_string_literal" | "multi_line_string_literal" | "raw_string_literal"
                ) {
                    return Some(unquote(&node_text(value, source)));
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(code) = labeled_code(child, source) {
            return Some(code);
        }
    }
    None
}

/// First string literal or bare identifier inside a thrown expression.
fn first_string_or_identifier(node: Node<'_>, source: &str) -> Option<String> {
    if matches!(
        node.kind(),
        "line_string_literal" | "multi_line_string_literal" | "raw_string_literal"
    ) {
        return Some(unquote(&node_text(node, source)));
    }
    if node.kind() == "simple_identifier" {
        return Some(node_text(node, source));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(code) = first_string_or_identifier(child, source) {
            return Some(code);
        }
    }
    None
}

/// Nearest enclosing `guard`/`if` condition, or `explicit throw` for a bare throw.
fn enclosing_condition(throw: Node<'_>, source: &str) -> String {
    let mut current = throw.parent();
    while let Some(node) = current {
        if matches!(node.kind(), "guard_statement" | "if_statement") {
            if let Some(condition) = node.child_by_field_name("condition") {
                let text = normalize_line(&node_text(condition, source));
                if !text.is_empty() {
                    return text;
                }
            }
        }
        current = node.parent();
    }
    "explicit throw".to_owned()
}

/// De-duplicate throw sites by code, keeping the first occurrence and status.
fn build_errors(sites: &[ThrowSite]) -> Vec<ErrorSpec> {
    let mut seen = BTreeSet::new();
    let mut errors = Vec::new();
    for site in sites {
        if !seen.insert(site.code.clone()) {
            continue;
        }
        errors.push(ErrorSpec {
            code: site.code.clone(),
            status: status_for_code(&site.code),
            when: site.when.clone(),
        });
    }
    errors
}

/// HTTP-like hint attached to a known error code.
fn status_for_code(code: &str) -> Option<u16> {
    match code {
        "TOKEN_NOT_FOUND" | "VALIDATION_ERROR" | "INVALID_INPUT" => Some(422),
        "EMPTY_TEXT" | "EMPTY_SENTENCE" | "INSECURE_PROTOCOL" | "BAD_REQUEST" | "invalid" => {
            Some(400)
        }
        "RATE_LIMITED" => Some(429),
        "NOT_FOUND" => Some(404),
        "UNAUTHORIZED" => Some(401),
        "FORBIDDEN" => Some(403),
        _ => Some(500),
    }
}

/// Remove surrounding quotes from a literal's source text.
fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2
        && matches!(bytes[0], b'\'' | b'"' | b'`')
        && bytes[bytes.len() - 1] == bytes[0]
    {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    trimmed.to_owned()
}

// ---------------------------------------------------------------------------
// Purity, permissions and budgets
// ---------------------------------------------------------------------------

/// Derive Swift purity, permissions, timeout and runtime from a declaration's text.
fn classify_swift_effects(slice: &str) -> (Purity, Vec<String>, u64, String) {
    let lower = slice.to_ascii_lowercase();
    let console =
        lower.contains("print(") || lower.contains("debugprint(") || lower.contains("os_log");
    let fs = lower.contains("filemanager") || lower.contains("data(contentsof:");
    let network = lower.contains("urlsession") || lower.contains("urlrequest");
    let env_keys = extract_environment_keys(slice);

    let mut permissions = BTreeSet::new();
    if console {
        permissions.insert("console:out".to_owned());
    }
    if fs {
        permissions.insert("fs:read".to_owned());
    }
    if network {
        permissions.insert("net:outbound".to_owned());
    }
    for key in &env_keys {
        permissions.insert(format!("secrets:read:{}", key));
    }

    let side_effect = console || fs || network || !env_keys.is_empty();
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
    // Reuse the contract's runtime labels so core and the UI need no Swift branch.
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

/// Keys read through `ProcessInfo.processInfo.environment["KEY"]`.
fn extract_environment_keys(slice: &str) -> Vec<String> {
    let lower = slice.to_ascii_lowercase();
    let marker = "processinfo.environment[";
    let mut keys = Vec::new();
    let mut offset = 0usize;
    while let Some(index) = lower[offset..].find(marker) {
        let start = offset + index + marker.len();
        let rest = &slice[start..];
        match rest.find('"') {
            Some(open) => {
                let after = &rest[open + 1..];
                match after.find('"') {
                    Some(close) => {
                        let key = &after[..close];
                        if !key.is_empty() && !keys.contains(&key.to_owned()) {
                            keys.push(key.to_owned());
                        }
                        offset = start + open + 1 + close + 1;
                    }
                    None => break,
                }
            }
            None => break,
        }
    }
    keys
}

// ---------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------

/// Source text of a node, degrading to an empty string for invalid UTF-8.
fn node_text(node: Node<'_>, source: &str) -> String {
    node.utf8_text(source.as_bytes()).unwrap_or("").to_owned()
}

/// Collapse every whitespace run into a single space and trim the result.
fn normalize_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
