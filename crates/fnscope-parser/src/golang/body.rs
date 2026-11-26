//! Deterministic, AST-backed analysis of one Go function body.
//!
//! The pass reads only the function located by the discovery record: it orders
//! top-level statements, records calls/guards/transforms/returns, extracts error
//! codes and classifies effects. No clock, randomness or map iteration enters
//! the result, so the same input always produces the same `BodyAnalysis`.

use std::collections::BTreeSet;

use fnscope_model::{ErrorSpec, Parameter, Purity, Step, StepKind};
use tree_sitter::{Node, Parser};

use crate::types::{BodyAnalysis, DiscoveredFn};

/// Grammar keywords that must never be reported as referenced identifiers.
const NON_INPUT_WORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "false",
    "for",
    "func",
    "go",
    "if",
    "import",
    "interface",
    "iota",
    "map",
    "nil",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "true",
    "type",
    "var",
];

/// Analyse one discovered Go function and return deterministic execution facts.
///
/// A function node is located by exact source slice, with a line/name fallback.
/// When no node matches, the neutral [`BodyAnalysis::default`] is returned so a
/// partially parsed file cannot corrupt a manifest.
pub fn analyze(found: &DiscoveredFn, source: &str, known_names: &[String]) -> BodyAnalysis {
    let language: tree_sitter::Language = tree_sitter_go::LANGUAGE.into();
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

    // Effects are read from the whole function text, not just its body, so a
    // guard or default expression in the header is also considered.
    let (purity, permissions, timeout_ms, runtime) = classify_go_effects(&slice);

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
        Some(node) if node.kind() == "block" => {
            // Go 0.25 wraps a block's statements in a `statement_list`.
            let mut block_cursor = node.walk();
            let container = node
                .named_children(&mut block_cursor)
                .find(|child| child.kind() == "statement_list")
                .unwrap_or(node);
            let mut cursor = container.walk();
            let items: Vec<Node<'_>> = container
                .named_children(&mut cursor)
                .filter(|child| child.kind() != "comment")
                .collect();
            items
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

/// Whether a node kind can be the discovered Go function itself.
fn is_function_kind(kind: &str) -> bool {
    matches!(kind, "function_declaration" | "method_declaration")
}

/// Find the smallest Go function node matching the discovery record.
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
        let same_name = node
            .child_by_field_name("name")
            .map(|name| {
                let name = node_text(name, source);
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

/// Return the body block of a function declaration or method declaration.
fn function_body(node: Node<'_>) -> Option<Node<'_>> {
    if let Some(body) = node.child_by_field_name("body") {
        return Some(body);
    }
    let mut cursor = node.walk();
    let block = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "block");
    block
}

// ---------------------------------------------------------------------------
// Step construction
// ---------------------------------------------------------------------------

/// Everything needed to describe one Go statement, gathered in one AST walk.
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

/// Collect the deterministic fields of one Go statement.
fn statement_facts(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
    error_sites: &[ErrorSite],
) -> StatementFacts {
    let code = normalize_line(&node_text(statement, source));
    let kind = step_kind(statement);
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
fn step_kind(statement: Node<'_>) -> StepKind {
    match statement.kind() {
        "return_statement" => StepKind::Return,
        "if_statement"
        | "expression_switch_statement"
        | "type_switch_statement"
        | "select_statement" => StepKind::Branch,
        "go_statement" | "defer_statement" | "expression_statement" => {
            if has_call(statement) {
                StepKind::Call
            } else {
                StepKind::Transform
            }
        }
        "short_var_declaration" | "var_declaration" | "assignment_statement" => {
            if has_call(statement) {
                StepKind::Call
            } else {
                StepKind::Transform
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
    let mut cursor = node.walk();
    let found = node
        .named_children(&mut cursor)
        .any(|child| child.kind() != "func_literal" && has_call(child));
    found
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
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        // A closure's calls belong to the closure, not to this step.
        if child.kind() == "func_literal" {
            continue;
        }
        visit_calls(child, source, out);
    }
}

/// Bare name of a call target: the last identifier of `a.b.c(...)` is `c`.
fn callee_bare_name(call: Node<'_>, source: &str) -> Option<String> {
    let target = call
        .child_by_field_name("function")
        .or_else(|| call.named_child(0))?;
    let mut identifiers = Vec::new();
    visit_identifiers(target, source, &mut identifiers);
    identifiers.pop()
}

/// Collect identifier texts inside a subtree, in source order.
fn visit_identifiers(node: Node<'_>, source: &str, out: &mut Vec<String>) {
    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "package_identifier"
    ) {
        out.push(node_text(node, source));
        return;
    }
    if node.kind() == "func_literal" || node.kind() == "comment" {
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

/// Recursive half of [`collect_inputs`]; declarations and type names are skipped.
fn visit_inputs(node: Node<'_>, source: &str, out: &mut Vec<(usize, String)>) {
    match node.kind() {
        "comment" | "type_identifier" | "package_identifier" | "qualified_type"
        | "pointer_type" | "slice_type" | "array_type" | "map_type" | "interface_type"
        | "struct_type" | "generic_type" | "function_type" | "channel_type" | "type_arguments"
        | "func_literal" => return,
        "selector_expression" => {
            let path = normalize_line(&node_text(node, source)).replace(' ', "");
            if !path.is_empty() {
                out.push((node.start_byte(), path));
            }
            return;
        }
        "short_var_declaration" | "assignment_statement" => {
            if let Some(right) = node.child_by_field_name("right") {
                visit_inputs(right, source, out);
            }
            return;
        }
        "var_spec" => {
            if let Some(value) = node.child_by_field_name("value") {
                visit_inputs(value, source, out);
            }
            return;
        }
        "identifier" | "field_identifier" => {
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
// Declarations
// ---------------------------------------------------------------------------

/// The condition of a branch, or the switched value when no condition is named.
fn branch_condition(statement: Node<'_>, source: &str) -> String {
    let node = match statement
        .child_by_field_name("condition")
        .or_else(|| statement.child_by_field_name("value"))
    {
        Some(node) => node,
        None => return "condition".to_owned(),
    };
    let text = normalize_line(&node_text(node, source));
    if text.is_empty() {
        "condition".to_owned()
    } else {
        text
    }
}

/// First assignment target and its declared or inferred display type.
fn first_declaration(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
) -> Option<Declaration> {
    match statement.kind() {
        "short_var_declaration" => {
            let name = statement
                .child_by_field_name("left")
                .and_then(|left| first_expression_text(left, source))?;
            let type_ = statement
                .child_by_field_name("right")
                .and_then(|right| first_expression_text(right, source))
                .map(|text| infer_go_type(&text, found));
            Some(Declaration { name, type_ })
        }
        "var_declaration" => {
            let mut cursor = statement.walk();
            let spec = statement
                .named_children(&mut cursor)
                .find(|child| child.kind() == "var_spec")?;
            let name = spec
                .child_by_field_name("name")
                .map(|name| normalize_line(&node_text(name, source)))
                .filter(|name| !name.is_empty())?;
            let annotated = spec
                .child_by_field_name("type")
                .map(|type_node| normalize_line(&node_text(type_node, source)))
                .filter(|text| !text.is_empty());
            let inferred = spec
                .child_by_field_name("value")
                .and_then(|value| first_expression_text(value, source))
                .map(|text| infer_go_type(&text, found));
            Some(Declaration {
                name,
                type_: annotated.or(inferred),
            })
        }
        "assignment_statement" => {
            let name = statement
                .child_by_field_name("left")
                .and_then(|left| first_expression_text(left, source))?;
            Some(Declaration { name, type_: None })
        }
        _ => None,
    }
}

/// Text of the first expression in an `expression_list`, normalized to one line.
fn first_expression_text(node: Node<'_>, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    let first = node.named_children(&mut cursor).next()?;
    let text = normalize_line(&node_text(first, source));
    (!text.is_empty()).then_some(text)
}

/// Conservative display type inferred from a literal or a declared parameter.
fn infer_go_type(text: &str, found: &DiscoveredFn) -> String {
    if text.starts_with('"') || text.starts_with('`') {
        return "string".to_owned();
    }
    match text {
        "true" | "false" => return "bool".to_owned(),
        "nil" => return "nil".to_owned(),
        _ => {}
    }
    if !text.is_empty() && text.chars().all(|ch| ch.is_ascii_digit()) {
        return "int".to_owned();
    }
    if text.parse::<f64>().is_ok() {
        return "float64".to_owned();
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

/// Which Go construct produced an error fact, used for the `when` fallback.
#[derive(Clone, Copy)]
enum ErrorKind {
    Panic,
    Constructor,
    Propagated,
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
        "call_expression" => go_error_call(node, source),
        "return_statement" => {
            go_error_return(node, source).map(|code| (code, ErrorKind::Propagated))
        }
        _ => None,
    };
    if let Some((code, kind)) = found {
        out.push(ErrorSite {
            byte: node.start_byte(),
            code,
            when: enclosing_condition(node, source, kind),
        });
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_error_sites(child, source, out);
    }
}

/// Code carried by `panic("CODE")`, `errors.New("CODE")` or `fmt.Errorf("CODE")`.
fn go_error_call(node: Node<'_>, source: &str) -> Option<(String, ErrorKind)> {
    let function = node.child_by_field_name("function")?;
    let target = normalize_line(&node_text(function, source)).replace(' ', "");
    let kind = match target.as_str() {
        "panic" => ErrorKind::Panic,
        "errors.New" | "fmt.Errorf" => ErrorKind::Constructor,
        _ => return None,
    };
    let arguments = node.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let first = arguments.named_children(&mut cursor).next()?;
    if !matches!(
        first.kind(),
        "interpreted_string_literal" | "raw_string_literal"
    ) {
        return None;
    }
    let code = unquote(&node_text(first, source));
    if code.is_empty() {
        return None;
    }
    Some((code, kind))
}

/// Recognize the Go idiom `return value, err` (or `return err`) as an error site.
fn go_error_return(node: Node<'_>, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if is_err_expression(child, source) {
            return Some("ERR".to_owned());
        }
    }
    None
}

/// True when an expression is the bare `err` identifier, including the last
/// element of a multi-value Go return list.
fn is_err_expression(node: Node<'_>, source: &str) -> bool {
    match node.kind() {
        "identifier" => node_text(node, source).trim() == "err",
        "expression_list" => {
            let mut cursor = node.walk();
            let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
            children
                .last()
                .map(|last| is_err_expression(*last, source))
                .unwrap_or(false)
        }
        _ => false,
    }
}

/// Nearest enclosing `if` condition, or a kind-specific deterministic fallback.
fn enclosing_condition(node: Node<'_>, source: &str, kind: ErrorKind) -> String {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "if_statement" {
            if let Some(condition) = parent.child_by_field_name("condition") {
                let text = normalize_line(&node_text(condition, source));
                if !text.is_empty() {
                    return text;
                }
            }
        }
        current = parent.parent();
    }
    match kind {
        ErrorKind::Panic => "explicit panic".to_owned(),
        ErrorKind::Constructor => "explicit error".to_owned(),
        ErrorKind::Propagated => "propagated error".to_owned(),
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

/// Derive Go purity, permissions, timeout and runtime from a function's text.
fn classify_go_effects(slice: &str) -> (Purity, Vec<String>, u64, String) {
    let lower = slice.to_ascii_lowercase();
    let console = lower.contains("fmt.print") || lower.contains("log.");
    let write_fs = lower.contains("os.writefile")
        || lower.contains("os.create")
        || lower.contains("ioutil.writefile");
    let read_fs = lower.contains("os.open")
        || lower.contains("os.readfile")
        || lower.contains("ioutil.read")
        || (lower.contains("ioutil.") && !write_fs);
    let network = lower.contains("net/http")
        || lower.contains("http.get")
        || lower.contains("http.post")
        || lower.contains("http.head")
        || lower.contains("http.client")
        || lower.contains("http.newrequest");
    let env_keys = extract_go_env_keys(slice);

    let mut permissions = BTreeSet::new();
    if console {
        permissions.insert("console:out".to_owned());
    }
    if read_fs {
        permissions.insert("fs:read".to_owned());
    }
    if write_fs {
        permissions.insert("fs:write".to_owned());
    }
    if network {
        permissions.insert("net:outbound".to_owned());
    }
    for key in &env_keys {
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
    // Reuse the contract's runtime labels so core and the UI need no Go branch.
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

/// Environment variable names read through `os.Getenv("KEY")`.
fn extract_go_env_keys(slice: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let needle = "os.Getenv(";
    let mut offset = 0usize;
    while let Some(index) = slice[offset..].find(needle) {
        let start = offset + index + needle.len();
        let rest = &slice[start..];
        let quote = match rest.chars().next() {
            Some(quote @ ('"' | '`')) => quote,
            _ => {
                offset = start;
                continue;
            }
        };
        let after = &rest[quote.len_utf8()..];
        let end = match after.find(quote) {
            Some(end) => end,
            None => break,
        };
        let key = &after[..end];
        if !key.is_empty() && !keys.contains(&key.to_owned()) {
            keys.push(key.to_owned());
        }
        offset = start + quote.len_utf8() + end + quote.len_utf8();
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

/// Remove surrounding quotes from a Go string literal's source text.
fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'"' | b'`') && bytes[bytes.len() - 1] == bytes[0] {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    trimmed.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::golang::discover::discover;
    use crate::types::FnKind;

    /// Call, branch, transform and return statements are ordered and classified.
    #[test]
    fn go_analyzes_call_branch_transform_return_steps() {
        let source = r#"package p
func normalize(text string) string { return text }
func analyze(text string) string {
	// Guard against empty input.
	if text == "" {
		return ""
	}
	cleaned := normalize(text)
	fmt.Println(cleaned)
	return cleaned
}
"#;
        let found = discover(source, "p.go");
        let analyze_fn = found
            .iter()
            .find(|item| item.name == "analyze")
            .expect("analyze must be discovered");
        assert_eq!(analyze_fn.kind, FnKind::Declaration);

        let analysis = analyze_body(analyze_fn, source, &["normalize".to_owned()]);
        let kinds: Vec<StepKind> = analysis.steps.iter().map(|step| step.kind).collect();
        // Spec §4 step 3 and the locked schema example
        // `const tokens = tokenize(...)` => `call` govern the initializer call.
        // Only top-level statements count; the `return ""` inside the guard is
        // part of the branch step, not a separate step.
        assert_eq!(
            kinds,
            [
                StepKind::Branch,
                StepKind::Call,
                StepKind::Call,
                StepKind::Return
            ]
        );
        assert_eq!(analysis.steps[0].title, "Guard against empty input.");
        assert!(analysis.steps[1]
            .code
            .starts_with("cleaned := normalize(text)"));
        assert_eq!(analysis.steps[2].kind, StepKind::Call);
        assert_eq!(analysis.callee_names, ["normalize", "Println"]);
    }

    /// `panic` and `errors.New` produce deduplicated codes with status and `when`.
    #[test]
    fn go_extracts_panic_and_error_constructor_codes() {
        let source = r#"package p
import "errors"
func check(text string) string {
	if text == "" {
		panic("EMPTY_TEXT")
	}
	if text == "x" {
		return "", errors.New("EMPTY_TEXT")
	}
	return text
}
"#;
        let found = discover(source, "p.go");
        let analysis = analyze_body(&found[0], source, &[]);
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "EMPTY_TEXT");
        assert_eq!(analysis.errors[0].status, Some(400));
        assert_eq!(analysis.errors[0].when, "text == \"\"");
        assert!(analysis
            .steps
            .iter()
            .any(|step| step.kind == StepKind::Branch
                && step.output == "throws EMPTY_TEXT | continues"));
    }

    /// A propagated `err` is reported with a stable fallback code.
    #[test]
    fn go_extracts_propagated_error_return() {
        let source = "package p\nfunc load() (string, error) { value, err := read(); if err != nil { return \"\", err }; return value, nil }";
        let found = discover(source, "p.go");
        let analysis = analyze_body(&found[0], source, &[]);
        assert!(analysis.errors.iter().any(|error| error.code == "ERR"));
    }

    /// `go` and `defer` call statements are Call steps with raw callee names.
    #[test]
    fn go_classifies_go_and_defer_as_calls() {
        let source = "package p\nfunc work() { go run(); defer stop() }";
        let found = discover(source, "p.go");
        let analysis = analyze_body(&found[0], source, &["run".to_owned(), "stop".to_owned()]);
        assert_eq!(analysis.steps.len(), 2);
        assert_eq!(analysis.steps[0].kind, StepKind::Call);
        assert_eq!(analysis.steps[0].calls_fn_id.as_deref(), Some("run"));
        assert_eq!(analysis.steps[1].kind, StepKind::Call);
        assert_eq!(analysis.steps[1].calls_fn_id.as_deref(), Some("stop"));
    }

    /// `fmt.Println` is a console side effect with a contract runtime and timeout.
    #[test]
    fn go_flags_fmt_println_as_console_side_effect() {
        let source = "package p\nfunc report(text string) { fmt.Println(text) }";
        let found = discover(source, "p.go");
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
    fn go_maps_effects_to_permissions() {
        let source = r#"package p
import (
	"fmt"
	"os"
)
func read() string {
	value := os.Getenv("API_KEY")
	file, err := os.Open("data.txt")
	if err != nil { fmt.Println(err) }
	resp, err := http.Get("https://example.com")
	_, _ = file, resp
	return value
}
"#;
        let found = discover(source, "p.go");
        let analysis = analyze_body(&found[0], source, &[]);
        assert_eq!(analysis.purity, Purity::SideEffect);
        assert!(analysis
            .permissions
            .contains(&"secrets:read:API_KEY".to_owned()));
        assert!(analysis.permissions.contains(&"fs:read".to_owned()));
        assert!(analysis.permissions.contains(&"net:outbound".to_owned()));
        assert_eq!(analysis.timeout_ms, 5000);
    }

    /// Repeating an analysis on the same source produces byte-identical facts.
    #[test]
    fn go_analysis_is_deterministic_on_repeat() {
        let source = r#"package p
func ping(n int) int {
	if n < 1 { return 0 }
	out := n + 1
	return out
}
"#;
        let found = discover(source, "p.go");
        let first = analyze_body(&found[0], source, &[]);
        let second = analyze_body(&found[0], source, &[]);
        assert_eq!(first, second);
        assert_eq!(first.steps.len(), 3);
        assert_eq!(first.steps[0].id, "step-1");
        assert_eq!(first.steps[2].id, "step-3");
    }

    /// A pure function keeps the contract's default timeout and runtime labels.
    #[test]
    fn go_pure_function_keeps_default_budget() {
        let source = "package p\nfunc add(a, b int) int { return a + b }";
        let found = discover(source, "p.go");
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
