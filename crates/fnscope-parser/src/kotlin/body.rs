//! Deterministic, AST-backed analysis of one Kotlin function body.
//!
//! The pass reads only the function located by the discovery record: it orders
//! top-level statements, records calls/branches/transforms/returns, extracts
//! error codes and classifies effects. No clock, randomness or map iteration
//! enters the result, so the same input always produces the same `BodyAnalysis`.

use std::collections::BTreeSet;

use fnscope_model::{ErrorSpec, Purity, Step, StepKind};
use tree_sitter::{Node, Parser};

use crate::kotlin::kotlin_language;
use crate::types::{BodyAnalysis, DiscoveredFn};

/// Grammar keywords that must never be reported as referenced identifiers.
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

const NON_INPUT_WORDS: &[&str] = &[
    "break",
    "catch",
    "class",
    "companion",
    "constructor",
    "continue",
    "do",
    "else",
    "false",
    "finally",
    "for",
    "fun",
    "if",
    "in",
    "is",
    "it",
    "null",
    "object",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "val",
    "var",
    "when",
    "while",
];

/// Analyze one discovered Kotlin function and return deterministic execution facts.
///
/// A function node is located by exact source slice, with a line/name fallback.
/// When no node matches, the neutral [`BodyAnalysis::default`] is returned so a
/// partially parsed file cannot corrupt a manifest.
pub fn analyze(found: &DiscoveredFn, source: &str, known_names: &[String]) -> BodyAnalysis {
    let mut parser = Parser::new();
    let language = kotlin_language();
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

    // Effects are read from the whole function text so a guard or default
    // expression in the header is also considered.
    let (purity, permissions, timeout_ms, runtime) = classify_kotlin_effects(&slice);

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

    let statements: Vec<Node<'_>> = match body {
        Some(node) => {
            let mut cursor = node.walk();
            let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
            // In the `-ng` grammar a `function_body` wraps a `block`, whose
            // children are `statement` nodes; an expression body holds its
            // expression directly and has no `block`.
            let container = if node.kind() == "function_body" {
                children
                    .iter()
                    .find(|child| child.kind() == "block")
                    .copied()
                    .unwrap_or(node)
            } else {
                node
            };
            let mut statement_cursor = container.walk();
            container
                .named_children(&mut statement_cursor)
                .filter(|child| {
                    !matches!(
                        child.kind(),
                        "line_comment" | "block_comment" | "label" | "annotation"
                    )
                })
                .map(unwrap)
                .filter(|child| child.kind() != "line_comment" && child.kind() != "block_comment")
                .collect()
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

/// Whether a node kind can be the discovered Kotlin function itself.
fn is_function_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration" | "secondary_constructor" | "primary_constructor"
    )
}

/// Find the smallest Kotlin function node matching the discovery record.
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
    if is_function_kind(node.kind()) {
        let text = node_text(node, source);
        let exact = text == found.source_code || text.trim() == found.source_code.trim();
        let same_line = node.start_position().row + 1 == found.line;
        let name_matches = match node.kind() {
            "function_declaration" => declared_name(node, source) == found.name,
            "primary_constructor" | "secondary_constructor" => found.name == "<init>",
            _ => false,
        };
        if exact || (same_line && name_matches) {
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

/// Return the body wrapper of a function or constructor.
///
/// Secondary constructors hold a `block` directly; declarations hold a
/// `function_body` that in turn holds a `block` or a bare expression.
fn function_body(node: Node<'_>) -> Option<Node<'_>> {
    if matches!(node.kind(), "secondary_constructor" | "primary_constructor") {
        return children_vec(node)
            .into_iter()
            .find(|child| child.kind() == "block");
    }
    children_vec(node)
        .into_iter()
        .find(|child| child.kind() == "function_body")
}

// ---------------------------------------------------------------------------
// Step construction
// ---------------------------------------------------------------------------

/// Everything needed to describe one Kotlin statement, gathered in one AST walk.
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
        StepKind::Call => facts
            .declared_type
            .clone()
            .unwrap_or_else(|| "void".to_owned()),
        StepKind::Transform => facts
            .declared_type
            .clone()
            .or_else(|| facts.assigned_name.clone())
            .unwrap_or_else(|| "value".to_owned()),
    };

    let (comment_title, comment_detail) =
        preceding_comments(source, statement.start_position().row);
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

/// Collect the deterministic fields of one Kotlin statement.
fn statement_facts(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
    error_sites: &[ErrorSite],
) -> StatementFacts {
    let code = normalize_line(&node_text(statement, source));
    let kind = step_kind(statement, source);
    let declaration = first_declaration(statement, source, found);
    let assigned_name = declaration.as_ref().map(|item| item.name.clone());
    let declared_type = declaration.as_ref().and_then(|item| item.type_.clone());
    let condition = branch_condition(statement, source);
    let callee = collect_calls(statement, source)
        .first()
        .map(|(_, name)| name.clone());
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

/// Classify a statement into the four contract step kinds.
fn step_kind(statement: Node<'_>, source: &str) -> StepKind {
    match statement.kind() {
        "return_expression" => StepKind::Return,
        // A `throw_expression` exits with an error, captured in `errors` rather
        // than presented as a value-producing return step.
        "throw_expression" => StepKind::Transform,
        "if_expression" | "when_expression" | "try_expression" => StepKind::Branch,
        "property_declaration"
        | "variable_declaration"
        | "multi_variable_declaration"
        | "assignment" => {
            if has_call(statement) {
                StepKind::Call
            } else {
                StepKind::Transform
            }
        }
        "call_expression" => {
            // `require`/`check` are guard clauses, not ordinary calls: the
            // spec treats a guard that may exit early as a branch.
            if guard_call(statement, source).is_some() {
                StepKind::Branch
            } else {
                StepKind::Call
            }
        }
        _ => {
            if has_call(statement) {
                StepKind::Call
            } else {
                StepKind::Transform
            }
        }
    }
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

/// Whether any call expression occurs below this node, excluding nested functions.
fn has_call(node: Node<'_>) -> bool {
    if node.kind() == "call_expression" {
        return true;
    }
    children_vec(node).into_iter().any(|child| {
        !matches!(
            child.kind(),
            "function_declaration"
                | "lambda_literal"
                | "anonymous_function"
                | "class_declaration"
                | "object_declaration"
        ) && has_call(child)
    })
}

/// All call expressions below a node, as `(start byte, bare callee name)` pairs.
fn collect_calls(node: Node<'_>, source: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    visit_calls(node, source, &mut found);
    found.sort_by_key(|(byte, _)| *byte);
    found
}

/// Recursive half of [`collect_calls`].
fn visit_calls(node: Node<'_>, source: &str, out: &mut Vec<(usize, String)>) {
    if node.kind() == "call_expression" {
        if let Some(callee) = callee_bare_name(node, source) {
            out.push((node.start_byte(), callee));
        }
    }
    for child in children_vec(node) {
        if matches!(
            child.kind(),
            "function_declaration"
                | "lambda_literal"
                | "anonymous_function"
                | "class_declaration"
                | "object_declaration"
        ) {
            continue;
        }
        visit_calls(child, source, out);
    }
}

/// Bare name of a call target: the last identifier of `a.b.c(...)` is `c`.
///
/// A trailing lambda makes `require(x) { "CODE" }` an outer `call_expression`
/// whose first child is the inner `call_expression`, not the `require`
/// identifier. Descend such wrappers instead of collecting every identifier in
/// the callee subtree: the old code reported `x` and lost the guard's error site.
fn callee_bare_name(call: Node<'_>, source: &str) -> Option<String> {
    let mut target = children_vec(call).into_iter().next()?;
    loop {
        if matches!(
            target.kind(),
            "expression" | "primary_expression" | "index_expression" | "annotated_expression"
        ) || target.kind() == "call_expression"
        {
            target = children_vec(target).into_iter().next()?;
            continue;
        }
        if target.kind() == "identifier" {
            return Some(normalize_line(&node_text(target, source)));
        }
        let mut identifiers = Vec::new();
        visit_identifiers(target, source, &mut identifiers);
        return identifiers.pop();
    }
}

/// Collect identifier texts inside a subtree, in source order.
fn visit_identifiers(node: Node<'_>, source: &str, out: &mut Vec<String>) {
    if node.kind() == "identifier" {
        out.push(node_text(node, source));
        return;
    }
    if matches!(
        node.kind(),
        "function_declaration" | "lambda_literal" | "anonymous_function" | "comment"
    ) {
        return;
    }
    for child in children_vec(node) {
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

/// Recursive half of [`collect_inputs`]; declarations and type names are skipped.
fn visit_inputs(node: Node<'_>, source: &str, out: &mut Vec<(usize, String)>) {
    match node.kind() {
        "line_comment"
        | "block_comment"
        | "type"
        | "user_type"
        | "nullable_type"
        | "non_nullable_type"
        | "parenthesized_type"
        | "function_type"
        | "type_arguments"
        | "function_declaration"
        | "lambda_literal"
        | "anonymous_function" => return,
        "identifier" => {
            let text = node_text(node, source);
            if !NON_INPUT_WORDS.contains(&text.as_str()) {
                out.push((node.start_byte(), text));
            }
            return;
        }
        _ => {}
    }
    for child in children_vec(node) {
        visit_inputs(child, source, out);
    }
}

// ---------------------------------------------------------------------------
// Declarations and branches
// ---------------------------------------------------------------------------

/// The condition of a branch, or a deterministic fallback when none is named.
fn branch_condition(statement: Node<'_>, source: &str) -> String {
    let children = children_vec(statement);
    let node = match statement.kind() {
        "if_expression" => statement.child_by_field_name("condition"),
        "when_expression" => children
            .iter()
            .find(|child| child.kind() == "when_subject")
            .copied(),
        _ => None,
    };
    match node {
        Some(node) => {
            let text = normalize_line(&node_text(node, source));
            if text.is_empty() {
                "condition".to_owned()
            } else {
                text
            }
        }
        None if statement.kind() == "try_expression" => "exception".to_owned(),
        None => "condition".to_owned(),
    }
}

/// First assignment target and its declared or inferred display type.
fn first_declaration(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
) -> Option<Declaration> {
    match statement.kind() {
        "property_declaration" => {
            let variable = children_vec(statement)
                .into_iter()
                .find(|child| child.kind() == "variable_declaration")?;
            let name = declared_name(variable, source);
            if name.is_empty() {
                return None;
            }
            let annotated = children_vec(variable)
                .into_iter()
                .find(|child| {
                    matches!(
                        child.kind(),
                        "type"
                            | "user_type"
                            | "nullable_type"
                            | "non_nullable_type"
                            | "function_type"
                    )
                })
                .map(|child| normalize_line(&node_text(child, source)));
            let inferred = infer_kotlin_type(&node_text(statement, source), found);
            Some(Declaration {
                name,
                type_: annotated.or(inferred),
            })
        }
        "assignment" => {
            let name = declared_name(statement, source);
            if name.is_empty() {
                return None;
            }
            Some(Declaration { name, type_: None })
        }
        _ => None,
    }
}

/// Conservative display type inferred from a literal or a declared parameter.
fn infer_kotlin_type(text: &str, found: &DiscoveredFn) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.starts_with('"') {
        return Some("String".to_owned());
    }
    if matches!(trimmed, "true" | "false") {
        return Some("Boolean".to_owned());
    }
    if trimmed.chars().all(|ch| ch.is_ascii_digit()) && !trimmed.is_empty() {
        return Some("Int".to_owned());
    }
    found
        .parameters
        .iter()
        .find(|parameter| trimmed.contains(parameter.name.as_str()))
        .map(|parameter| parameter.type_.clone())
}

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
    let mut index = 0usize;
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

/// Which Kotlin construct produced an error fact, used for the `when` fallback.
#[derive(Clone, Copy)]
enum ErrorKind {
    Throw,
    Guard,
}

/// One error site reduced to the facts needed by `ErrorSpec` and branches.
struct ErrorSite {
    byte: usize,
    code: String,
    when: String,
}

/// Walk every descendant of the body and record error sites in source order.
fn collect_error_sites(body: Node<'_>, source: &str) -> Vec<ErrorSite> {
    let mut sites = Vec::new();
    visit_error_sites(body, source, &mut sites);
    sites.sort_by_key(|site| site.byte);
    sites
}

/// Recursive half of [`collect_error_sites`].
fn visit_error_sites(node: Node<'_>, source: &str, out: &mut Vec<ErrorSite>) {
    let found = match node.kind() {
        "throw_expression" => throw_code(node, source).map(|code| (code, ErrorKind::Throw)),
        "call_expression" | "constructor_invocation" => guard_call(node, source),
        _ => None,
    };
    if let Some((code, kind)) = found {
        out.push(ErrorSite {
            byte: node.start_byte(),
            code,
            when: enclosing_condition(node, source, kind),
        });
    }
    for child in children_vec(node) {
        visit_error_sites(child, source, out);
    }
}

/// Code carried by `throw X("CODE")`.
fn throw_code(node: Node<'_>, source: &str) -> Option<String> {
    let text = node_text(node, source);
    if !text.trim_start().starts_with("throw") {
        return None;
    }
    first_string_literal(node, source)
}

/// Code carried by `require("CODE")` / `check("CODE")` guards.
fn guard_call(node: Node<'_>, source: &str) -> Option<(String, ErrorKind)> {
    let callee = callee_bare_name(node, source).unwrap_or_default();
    let trimmed = node_text(node, source);
    let trimmed = trimmed.trim_start();
    let by_name = matches!(
        callee.as_str(),
        "require" | "check" | "requireNotNull" | "checkNotNull"
    );
    // A trailing lambda can wrap the guard in an extra `call_expression`, so
    // also accept the literal keyword at the start of the call's own text.
    let by_text = ["require(", "check(", "requireNotNull(", "checkNotNull("]
        .iter()
        .any(|prefix| trimmed.starts_with(prefix));
    if !by_name && !by_text {
        return None;
    }
    // Kotlin guards often put the message in a trailing lambda, so search the
    // whole call subtree rather than only direct arguments.
    first_string_literal(node, source).map(|code| (code, ErrorKind::Guard))
}

/// First quoted string literal found below a node, in source order.
fn first_string_literal(node: Node<'_>, source: &str) -> Option<String> {
    if node.kind() == "string_literal" {
        let code = unquote(&node_text(node, source));
        if !code.is_empty() {
            return Some(code);
        }
    }
    for child in children_vec(node) {
        if let Some(code) = first_string_literal(child, source) {
            return Some(code);
        }
    }
    None
}

/// Strip surrounding quotes from a Kotlin string literal.
fn unquote(text: &str) -> String {
    let text = text.trim();
    if text.len() >= 2 && (text.starts_with('"') && text.ends_with('"')) {
        return text[1..text.len() - 1].to_owned();
    }
    text.to_owned()
}

/// Nearest enclosing `if`/`when` condition, or a kind-specific deterministic fallback.
fn enclosing_condition(node: Node<'_>, source: &str, kind: ErrorKind) -> String {
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(parent.kind(), "if_expression" | "when_expression") {
            let text = branch_condition(parent, source);
            if !text.is_empty() && text != "condition" {
                return text;
            }
        }
        current = parent.parent();
    }
    match kind {
        ErrorKind::Throw => "explicit throw".to_owned(),
        ErrorKind::Guard => "guard check".to_owned(),
    }
}

/// Deduplicate error sites by code, keeping the first occurrence and its status.
fn build_errors(sites: &[ErrorSite]) -> Vec<ErrorSpec> {
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
        "EMPTY_TEXT" | "EMPTY_SENTENCE" | "BAD_REQUEST" => Some(400),
        "NOT_FOUND" => Some(404),
        "UNAUTHORIZED" => Some(401),
        "FORBIDDEN" => Some(403),
        _ => Some(500),
    }
}

// ---------------------------------------------------------------------------
// Purity, permissions and budgets
// ---------------------------------------------------------------------------

/// Derive Kotlin purity, permissions, timeout and runtime from a function's text.
fn classify_kotlin_effects(slice: &str) -> (Purity, Vec<String>, u64, String) {
    let lower = slice.to_ascii_lowercase();
    let mut permissions = BTreeSet::new();

    let console = lower.contains("println(") || lower.contains("print(");
    if console {
        permissions.insert("console:out".to_owned());
    }
    let filesystem = lower.contains("java.io")
        || lower.contains("file(")
        || lower.contains("files.")
        || lower.contains("fileinputstream");
    if filesystem {
        permissions.insert("fs".to_owned());
    }
    let network = lower.contains("urlconnection") || lower.contains("httpclient");
    if network {
        permissions.insert("net:outbound".to_owned());
    }
    for key in env_keys(slice) {
        permissions.insert(format!("secrets:read:{key}"));
    }

    let side_effect = !permissions.is_empty();
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

/// Uppercase environment keys read through `System.getenv("KEY")`.
fn env_keys(text: &str) -> Vec<String> {
    let needle = "System.getenv(";
    let mut keys = Vec::new();
    let mut rest = text;
    while let Some(index) = rest.find(needle) {
        let after = &rest[index + needle.len()..];
        let end = after.find(')').unwrap_or(after.len());
        let key = after[..end]
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim();
        if !key.is_empty()
            && key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            keys.push(key.to_owned());
        }
        rest = if end < after.len() {
            &after[end + 1..]
        } else {
            ""
        };
    }
    keys
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

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
fn declared_name(node: Node<'_>, source: &str) -> String {
    node.child_by_field_name("name")
        .or_else(|| node.child_by_field_name("type"))
        .map(|name| normalize_line(&node_text(name, source)))
        .filter(|name| !name.is_empty())
        .or_else(|| {
            let fallback = first_kind_text(node, "identifier", source);
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
fn first_kind_text(node: Node<'_>, kind: &str, source: &str) -> String {
    first_kind(node, kind)
        .map(|child| normalize_line(&node_text(child, source)))
        .unwrap_or_default()
}

/// Exact source text of a node, degrading to an empty string for invalid UTF-8.
fn node_text(node: Node<'_>, source: &str) -> String {
    String::from_utf8_lossy(&source.as_bytes()[node.start_byte()..node.end_byte()]).into_owned()
}

/// Collapse every whitespace run into a single space and trim the result.
fn normalize_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::LanguageAdapter;
    use crate::kotlin::KotlinAdapter;
    use crate::types::FnKind;

    /// One function exposes call, branch, transform and return steps in order.
    #[test]
    fn analyzes_ordered_step_kinds() {
        let source = r#"fun evaluate(value: Int): Int {
    val doubled = value * 2
    if (doubled > 0) {
        return doubled
    }
    println(doubled)
    return 0
}
"#;
        let adapter = KotlinAdapter::new();
        let found = adapter.discover(source, "Evaluate.kt");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, FnKind::Declaration);
        let analysis = adapter.analyze(&found[0], source, &["helper".to_owned()]);
        let kinds: Vec<StepKind> = analysis.steps.iter().map(|step| step.kind).collect();
        assert_eq!(
            kinds,
            [
                StepKind::Transform,
                StepKind::Branch,
                StepKind::Call,
                StepKind::Return,
            ]
        );
        assert_eq!(analysis.steps[2].code, "println(doubled)");
        assert!(analysis.callee_names.contains(&"println".to_owned()));
    }

    /// A declaration with a call initializer is a Call step, not a Transform.
    #[test]
    fn declaration_with_call_initializer_is_a_call() {
        let source = "fun run(): String { val text = helper(\"x\"); return text }";
        let adapter = KotlinAdapter::new();
        let found = adapter.discover(source, "Run.kt");
        let analysis = adapter.analyze(&found[0], source, &["helper".to_owned()]);
        assert_eq!(analysis.steps[0].kind, StepKind::Call);
        assert_eq!(analysis.steps[0].calls_fn_id.as_deref(), Some("helper"));
        assert_eq!(analysis.callee_names, ["helper"]);
    }

    /// `throw X("CODE")` and `require("CODE")` produce de-duplicated error specs.
    #[test]
    fn extracts_throw_and_guard_errors() {
        let source = r#"fun check(input: Int): Int {
    require(input >= 0) { "INVALID_INPUT" }
    if (input == 0) {
        throw IllegalArgumentException("EMPTY_SENTENCE")
    }
    throw IllegalArgumentException("EMPTY_SENTENCE")
    return input
}
"#;
        let adapter = KotlinAdapter::new();
        let found = adapter.discover(source, "Check.kt");
        let analysis = adapter.analyze(&found[0], source, &[]);
        let codes: Vec<&str> = analysis
            .errors
            .iter()
            .map(|error| error.code.as_str())
            .collect();
        assert_eq!(codes, ["INVALID_INPUT", "EMPTY_SENTENCE"]);
        assert_eq!(analysis.errors[0].status, Some(422));
        assert_eq!(analysis.errors[1].status, Some(400));
        assert!(analysis.errors[0].when.contains("guard"));
    }

    /// Console, filesystem, network and secret reads map to contract permissions.
    #[test]
    fn classifies_side_effects_and_permissions() {
        let source = r#"fun report(): String {
    println("hello")
    val key = System.getenv("API_TOKEN")
    return key
}
"#;
        let adapter = KotlinAdapter::new();
        let found = adapter.discover(source, "Report.kt");
        let analysis = adapter.analyze(&found[0], source, &[]);
        assert_eq!(analysis.purity, Purity::SideEffect);
        assert_eq!(
            analysis.permissions,
            ["console:out", "secrets:read:API_TOKEN"]
        );
        assert_eq!(analysis.timeout_ms, 1000);
        assert_eq!(analysis.runtime, "node20");

        let network = "fun fetchData(): Unit { val c: java.net.URLConnection? = null }";
        let found = adapter.discover(network, "Net.kt");
        let analysis = adapter.analyze(&found[0], network, &[]);
        assert!(analysis.permissions.contains(&"net:outbound".to_owned()));
        assert_eq!(analysis.timeout_ms, 5000);
    }

    /// A pure function keeps the contract's default timeout and runtime labels.
    #[test]
    fn pure_function_keeps_contract_defaults() {
        let source = "fun identity(value: Int): Int = value";
        let adapter = KotlinAdapter::new();
        let found = adapter.discover(source, "Identity.kt");
        let analysis = adapter.analyze(&found[0], source, &[]);
        assert_eq!(analysis.purity, Purity::Pure);
        assert!(analysis.permissions.is_empty());
        assert_eq!(analysis.timeout_ms, 150);
        assert_eq!(analysis.runtime, "universal");
    }

    /// Repeated analysis of the same function returns identical facts.
    #[test]
    fn analysis_is_deterministic_on_repeat() {
        let source = r#"fun repeatable(value: Int): Int {
    val result = value + 1
    if (result > 2) {
        return result
    }
    return 0
}
"#;
        let adapter = KotlinAdapter::new();
        let found = adapter.discover(source, "Repeatable.kt");
        let first = adapter.analyze(&found[0], source, &[]);
        let second = adapter.analyze(&found[0], source, &[]);
        assert_eq!(first, second);
    }
}
