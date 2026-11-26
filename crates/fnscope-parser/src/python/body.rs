//! Deterministic, AST-backed analysis of one Python function body.
//!
//! The pass reads only the source slice of a discovered function. It orders the
//! body's statements, records calls/guards/transforms/returns, extracts `raise`
//! error codes, and classifies effects. No clock, randomness or map iteration
//! enters the result, so the same input always produces the same `BodyAnalysis`.

use std::collections::BTreeSet;

use fnscope_model::{ErrorSpec, Parameter, Purity, Step, StepKind};
use tree_sitter::{Node, Parser};

use crate::types::{BodyAnalysis, DiscoveredFn};

/// Python keywords that must never be reported as referenced identifiers.
const NON_INPUT_WORDS: &[&str] = &[
    "return", "if", "elif", "else", "raise", "def", "class", "for", "while", "in", "not", "and",
    "or", "None", "True", "False", "async", "await", "with", "as", "try", "except", "finally",
    "lambda", "pass", "break", "continue", "import", "from", "yield", "global", "nonlocal",
    "assert", "del",
];

/// Analyze one discovered function into ordered, deterministic facts.
///
/// The function node is located by exact source slice with a line/name fallback.
/// When no node matches, the neutral `BodyAnalysis::default()` is returned so a
/// partially discovered file cannot corrupt a manifest.
pub fn analyze(found: &DiscoveredFn, source: &str, known_names: &[String]) -> BodyAnalysis {
    let language: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return BodyAnalysis::default();
    }
    let tree = match parser.parse(source, None) {
        Some(tree) => tree,
        None => return BodyAnalysis::default(),
    };
    let root = tree.root_node();
    let function = match find_function_node(root, source, found) {
        Some(node) => node,
        None => return BodyAnalysis::default(),
    };
    let slice = node_text(&function, source);
    let (purity, permissions, timeout_ms, runtime) = classify_python_effects(&slice);

    let body = function_body(function);
    let error_sites = collect_raise_sites(body.unwrap_or(function), source);
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
        Some(node) if node.kind() == "block" => {
            let mut cursor = node.walk();
            let body_statements: Vec<Node<'_>> = node
                .named_children(&mut cursor)
                .filter(|child| {
                    let kind = child.kind();
                    if kind == "comment" {
                        return false;
                    }
                    if kind == "expression_statement" && is_docstring(*child, source) {
                        return false;
                    }
                    true
                })
                .collect();
            body_statements
        }
        Some(node) => vec![node],
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

/// Finds the tightest `function_definition` matching the recorded slice or line+name.
fn find_function_node<'tree>(
    root: Node<'tree>,
    source: &str,
    found: &DiscoveredFn,
) -> Option<Node<'tree>> {
    let mut best: Option<Node<'tree>> = None;
    visit_function_nodes(root, source, found, &mut best);
    best
}

/// Depth-first walk that keeps the smallest matching function node.
fn visit_function_nodes<'tree>(
    node: Node<'tree>,
    source: &str,
    found: &DiscoveredFn,
    best: &mut Option<Node<'tree>>,
) {
    if node.kind() == "function_definition" {
        let text = node_text(&node, source);
        let exact = text == found.source_code || text.trim() == found.source_code.trim();
        let same_line = node.start_position().row + 1 == found.line;
        let same_name = node
            .child_by_field_name("name")
            .map(|name| {
                let name = node_text(&name, source);
                name == found.name || name == found.qualified_name
            })
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

/// The `body` field, which is a `block` for normal functions or one statement.
fn function_body(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("body")
}

/// Whether an `expression_statement` holds only a docstring literal.
fn is_docstring(node: Node<'_>, source: &str) -> bool {
    if node.kind() != "expression_statement" {
        return false;
    }
    let mut cursor = node.walk();
    let has_string = node
        .named_children(&mut cursor)
        .any(|child| matches!(child.kind(), "string" | "concatenated_string"));
    has_string
        && (node_text(&node, source).trim_start().starts_with('"')
            || node_text(&node, source).trim_start().starts_with('\''))
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

/// Look through `expression_statement` wrappers to the expression they contain.
fn statement_expression(node: Node<'_>) -> Node<'_> {
    let mut current = node;
    while current.kind() == "expression_statement" {
        let mut cursor = current.walk();
        let first = current.named_children(&mut cursor).next();
        match first {
            Some(child) => current = child,
            None => break,
        }
    }
    current
}

/// Classify a statement by its Python node kind, exactly as the contract maps it.
fn classify_statement(statement: Node<'_>) -> StepKind {
    match statement.kind() {
        "return_statement" => StepKind::Return,
        "if_statement" | "match_statement" => StepKind::Branch,
        "assignment" | "augmented_assignment" | "named_expression" | "raise_statement" => {
            StepKind::Transform
        }
        _ => {
            let core = statement_expression(statement);
            if core.kind() == "call" || has_call(statement) {
                StepKind::Call
            } else {
                StepKind::Transform
            }
        }
    }
}

/// Build one ordered step plus the raw callee names encountered in it.
fn build_step(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
    known_names: &[String],
    index: usize,
    error_sites: &[RaiseSite],
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
            Some(ref code) => format!("throws {code} | continues"),
            None => format!("{} | continues", facts.condition),
        },
        _ => facts
            .declared_type
            .clone()
            .unwrap_or_else(|| "void".to_owned()),
    };

    let (comment_title, comment_detail) =
        preceding_hash_comments(source, statement.start_position().row);
    let title = comment_title.unwrap_or_else(|| fallback_title(&facts, &output));
    let detail = comment_detail.unwrap_or_else(|| fallback_detail(&facts, &output));

    let step = Step {
        id: format!("step-{index}"),
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

/// Collect the deterministic fields of one statement.
fn statement_facts(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
    error_sites: &[RaiseSite],
) -> StatementFacts {
    let code = normalize_line(&node_text(&statement, source));
    let kind = classify_statement(statement);
    let assigned_name = assignment_left(statement, source);
    let declared_type = declared_type(statement, source, found, assigned_name.as_deref());
    let condition = condition_text(statement, source);
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

/// Left-hand target of an assignment or augmented assignment.
fn assignment_left(statement: Node<'_>, source: &str) -> Option<String> {
    if !matches!(statement.kind(), "assignment" | "augmented_assignment") {
        return None;
    }
    statement
        .child_by_field_name("left")
        .map(|node| normalize_line(&node_text(&node, source)))
        .filter(|text| !text.is_empty())
}

/// Annotation when present, otherwise a conservative inference from the value.
fn declared_type(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
    assigned_name: Option<&str>,
) -> Option<String> {
    let name = assigned_name?;
    if let Some(node) = statement.child_by_field_name("type") {
        let annotation = normalize_line(&node_text(&node, source))
            .trim_start_matches(':')
            .trim()
            .to_owned();
        if !annotation.is_empty() {
            return Some(format!("{name}: {annotation}"));
        }
    }
    let value = statement.child_by_field_name("right")?;
    Some(format!("{name}: {}", infer_type(value, source, found)))
}

/// Infer a display type from a Python literal or a parameter's annotation.
fn infer_type(value: Node<'_>, source: &str, found: &DiscoveredFn) -> String {
    match value.kind() {
        "string" | "concatenated_string" => "string".to_owned(),
        "integer" => "int".to_owned(),
        "float" => "float".to_owned(),
        "true" | "false" => "bool".to_owned(),
        "none" => "None".to_owned(),
        "list" => "list".to_owned(),
        "dictionary" => "dict".to_owned(),
        "tuple" => "tuple".to_owned(),
        "identifier" => parameter_type(&node_text(&value, source), &found.parameters)
            .unwrap_or_else(|| "unknown".to_owned()),
        _ => "unknown".to_owned(),
    }
}

/// Look up a declared parameter type for simple identifier-to-identifier copies.
fn parameter_type(name: &str, parameters: &[Parameter]) -> Option<String> {
    parameters
        .iter()
        .find(|parameter| parameter.name == name && !parameter.type_.trim().is_empty())
        .map(|parameter| parameter.type_.clone())
}

/// Condition of an `if` or subject of a `match`, normalized for display.
fn condition_text(statement: Node<'_>, source: &str) -> String {
    statement
        .child_by_field_name("condition")
        .or_else(|| statement.child_by_field_name("subject"))
        .map(|node| normalize_line(&node_text(&node, source)))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "condition".to_owned())
}

/// Default title when no annotated comment precedes the statement.
fn fallback_title(facts: &StatementFacts, output: &str) -> String {
    match facts.kind {
        StepKind::Call => format!("Call {}()", facts.callee.as_deref().unwrap_or("unknown")),
        StepKind::Branch => format!("Guard: {}", truncate(&facts.condition, 48)),
        StepKind::Return => format!("Return {output}"),
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
        StepKind::Return => format!("Returns {output} to the caller."),
        StepKind::Transform => format!(
            "Assigns {} from the expression.",
            facts.assigned_name.as_deref().unwrap_or("value")
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

/// Whether any call expression occurs below a node, excluding nested functions.
fn has_call(node: Node<'_>) -> bool {
    if node.kind() == "call" {
        return true;
    }
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).any(|child| {
        !matches!(
            child.kind(),
            "function_definition" | "lambda" | "class_definition"
        ) && has_call(child)
    });
    found
}

/// All call expressions below a node as `(start byte, bare callee name)` pairs.
fn collect_calls(node: Node<'_>, source: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    visit_calls(node, source, &mut found);
    found.sort_by_key(|(byte, _)| *byte);
    found
}

/// Recursive half of [`collect_calls`]; nested functions belong to another body.
fn visit_calls(node: Node<'_>, source: &str, out: &mut Vec<(usize, String)>) {
    if node.kind() == "call" {
        if let Some(callee) = callee_bare_name(node, source) {
            out.push((node.start_byte(), callee));
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(
            child.kind(),
            "function_definition" | "lambda" | "class_definition"
        ) {
            continue;
        }
        visit_calls(child, source, out);
    }
}

/// Bare callee of a Python call: the last identifier of `a.b.c(...)` is `c`.
fn callee_bare_name(call: Node<'_>, source: &str) -> Option<String> {
    let target = call
        .child_by_field_name("function")
        .or_else(|| call.named_child(0))?;
    let mut identifiers = Vec::new();
    visit_identifiers(target, source, &mut identifiers);
    identifiers.pop()
}

/// Collect identifier texts inside a subtree in source order.
fn visit_identifiers(node: Node<'_>, source: &str, out: &mut Vec<String>) {
    if node.kind() == "identifier" {
        out.push(node_text(&node, source));
        return;
    }
    if node.kind() == "comment" {
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

/// Recursive half of [`collect_inputs`]; literals and assignment targets are skipped.
fn visit_inputs(node: Node<'_>, source: &str, out: &mut Vec<(usize, String)>) {
    match node.kind() {
        "comment" | "string" | "concatenated_string" | "type" => return,
        "assignment" | "augmented_assignment" => {
            if let Some(right) = node.child_by_field_name("right") {
                visit_inputs(right, source, out);
            }
            return;
        }
        "attribute" => {
            let path = normalize_line(&node_text(&node, source)).replace(' ', "");
            if !path.is_empty() {
                out.push((node.start_byte(), path));
            }
            return;
        }
        "identifier" => {
            let text = node_text(&node, source);
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

/// Top two lines of the contiguous `#` block directly above a statement.
fn preceding_hash_comments(source: &str, statement_row: usize) -> (Option<String>, Option<String>) {
    let lines: Vec<&str> = source.lines().collect();
    let mut block = Vec::new();
    let mut row = statement_row;
    while row > 0 {
        row -= 1;
        let line = lines.get(row).copied().unwrap_or("").trim();
        match line.strip_prefix('#') {
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

/// One `raise` statement reduced to the facts needed by `ErrorSpec` and branches.
struct RaiseSite {
    byte: usize,
    code: String,
    when: String,
}

/// Walk every descendant of the body and record `raise` sites in source order.
fn collect_raise_sites(body: Node<'_>, source: &str) -> Vec<RaiseSite> {
    let mut sites = Vec::new();
    visit_raises(body, source, &mut sites);
    sites.sort_by_key(|site| site.byte);
    sites
}

/// Recursive half of [`collect_raise_sites`].
fn visit_raises(node: Node<'_>, source: &str, out: &mut Vec<RaiseSite>) {
    if node.kind() == "raise_statement" {
        if let Some(code) = raise_code(node, source) {
            out.push(RaiseSite {
                byte: node.start_byte(),
                code,
                when: enclosing_condition(node, source),
            });
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_raises(child, source, out);
    }
}

/// Code carried by `raise ValueError("CODE")` or `raise AppError("CODE", ...)`.
fn raise_code(node: Node<'_>, source: &str) -> Option<String> {
    if let Some(code) = first_string(node, source) {
        return Some(code);
    }
    let mut cursor = node.walk();
    let first = node.named_children(&mut cursor).next()?;
    let name = if first.kind() == "call" {
        callee_bare_name(first, source)?
    } else {
        normalize_line(&node_text(&first, source))
    };
    (!name.is_empty()).then_some(name)
}

/// First string literal inside a subtree, using source order for determinism.
fn first_string(node: Node<'_>, source: &str) -> Option<String> {
    if matches!(node.kind(), "string" | "concatenated_string") {
        return Some(unquote(&node_text(&node, source)));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(code) = first_string(child, source) {
            return Some(code);
        }
    }
    None
}

/// Nearest enclosing `if` condition, or `explicit raise` for an unguarded site.
fn enclosing_condition(raise: Node<'_>, source: &str) -> String {
    let mut current = raise.parent();
    while let Some(node) = current {
        if node.kind() == "if_statement" {
            if let Some(condition) = node.child_by_field_name("condition") {
                let text = normalize_line(&node_text(&condition, source));
                if !text.is_empty() {
                    return text;
                }
            }
        }
        current = node.parent();
    }
    "explicit raise".to_owned()
}

/// Deduplicate raise sites by code, keeping the first occurrence and status.
fn build_errors(sites: &[RaiseSite]) -> Vec<ErrorSpec> {
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
        "EMPTY_TEXT" | "EMPTY_SENTENCE" | "INSECURE_PROTOCOL" | "BAD_REQUEST" => Some(400),
        "NOT_FOUND" => Some(404),
        "UNAUTHORIZED" => Some(401),
        "FORBIDDEN" => Some(403),
        _ => Some(500),
    }
}

// ---------------------------------------------------------------------------
// Purity, permissions and budgets
// ---------------------------------------------------------------------------

/// Derive Python purity, permissions, timeout and runtime from a function's source.
fn classify_python_effects(slice: &str) -> (Purity, Vec<String>, u64, String) {
    let lower = slice.to_ascii_lowercase();
    let logging =
        lower.contains("print(") || lower.contains("logging") || lower.contains("logger.");
    let network = lower.contains("requests")
        || lower.contains("urllib")
        || lower.contains("socket")
        || lower.contains("http.client");
    let filesystem = lower.contains("open(")
        || lower.contains("pathlib")
        || lower.contains("os.")
        || lower.contains("shutil");
    let fs_write = lower.contains("write_text")
        || lower.contains("'w'")
        || lower.contains("\"w\"")
        || lower.contains("'a'")
        || lower.contains("\"a\"");
    let env_keys = extract_env_keys(slice);

    let mut permissions = BTreeSet::new();
    if logging {
        permissions.insert("console:out".to_owned());
    }
    if network {
        permissions.insert("net:outbound".to_owned());
    }
    if filesystem {
        permissions.insert("fs:read".to_owned());
    }
    if fs_write {
        permissions.insert("fs:write".to_owned());
    }
    for key in &env_keys {
        permissions.insert(format!("secrets:read:{key}"));
    }

    let side_effect = logging || network || filesystem || !env_keys.is_empty();
    let purity = if side_effect {
        Purity::SideEffect
    } else {
        Purity::Pure
    };
    // Budgets and runtime labels match the existing adapters' contract values.
    let timeout_ms = if !side_effect {
        150
    } else if network {
        5000
    } else {
        1000
    };
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

/// Environment keys read via `os.environ[...]`, `os.getenv(...)` or `environ.get(...)`.
fn extract_env_keys(slice: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let markers = [
        "os.environ[\"",
        "os.environ['",
        "environ[\"",
        "environ['",
        "getenv(\"",
        "getenv('",
        "environ.get(\"",
        "environ.get('",
    ];
    for marker in markers {
        let mut offset = 0usize;
        while let Some(index) = slice[offset..].find(marker) {
            let start = offset + index + marker.len();
            let rest = &slice[start..];
            match rest.find(|ch: char| ch == '"' || ch == '\'') {
                Some(end) => {
                    let key = &rest[..end];
                    if !key.is_empty() && !keys.contains(&key.to_owned()) {
                        keys.push(key.to_owned());
                    }
                    offset = start + end + 1;
                }
                None => break,
            }
        }
    }
    keys
}

// ---------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------

/// Source text of a node, degrading to an empty string for invalid UTF-8.
fn node_text(node: &Node<'_>, source: &str) -> String {
    node.utf8_text(source.as_bytes()).unwrap_or("").to_owned()
}

/// Collapse every whitespace run into a single space and trim the result.
fn normalize_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Remove one layer of Python string quotes, including triple-quoted literals.
fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    for quote in ["\"\"\"", "'''"] {
        if trimmed.len() >= 6 && trimmed.starts_with(quote) && trimmed.ends_with(quote) {
            return trimmed[3..trimmed.len() - 3].trim().to_owned();
        }
    }
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'\'' | b'"') && bytes[bytes.len() - 1] == bytes[0] {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    trimmed.to_owned()
}
