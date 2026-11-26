//! Deterministic, AST-backed analysis of one C function body.
//!
//! The pass reads only the function located by the discovery record: it orders
//! top-level statements, records calls/guards/transforms/returns, extracts C
//! error idioms and classifies effects. No clock, randomness or map iteration
//! enters the result, so the same input always produces the same `BodyAnalysis`.
//!
//! `compound_statement` holds statements directly in tree-sitter-c 0.24.2; a
//! `statement_list` wrapper is unwrapped defensively if a future grammar adds
//! one, mirroring the Go adapter's handling of that wrapper.

use std::collections::BTreeSet;

use fnscope_model::{ErrorSpec, Purity, Step, StepKind};
use tree_sitter::{Node, Parser};

use crate::types::{BodyAnalysis, DiscoveredFn};

/// Declarator wrappers that can sit between a definition and its name.
const DECLARATOR_WRAPPERS: &[&str] = &[
    "pointer_declarator",
    "parenthesized_declarator",
    "attributed_declarator",
    "array_declarator",
];

/// Grammar keywords and constants that must not be reported as inputs.
const NON_INPUT_WORDS: &[&str] = &[
    "break", "case", "char", "const", "continue", "default", "do", "double", "else", "enum",
    "extern", "false", "float", "for", "goto", "if", "inline", "int", "long", "NULL", "register",
    "restrict", "return", "short", "signed", "sizeof", "static", "struct", "switch", "true",
    "typedef", "union", "unsigned", "void", "volatile", "while",
];

/// Analyse one discovered C function and return deterministic execution facts.
///
/// A function node is located by exact source slice, with a line/name fallback.
/// When no node matches, the neutral [`BodyAnalysis::default`] is returned so a
/// partially parsed file cannot corrupt a manifest.
pub fn analyze(found: &DiscoveredFn, source: &str, known_names: &[String]) -> BodyAnalysis {
    let language: tree_sitter::Language = tree_sitter_c::LANGUAGE.into();
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

    let (purity, permissions, timeout_ms, runtime) = classify_c_effects(fn_node, source);

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
        Some(node) if node.kind() == "compound_statement" => {
            // Defensive wrapper unwrap; 0.24.2 stores statements directly.
            let mut block_cursor = node.walk();
            let container = node
                .named_children(&mut block_cursor)
                .find(|child| child.kind() == "statement_list")
                .unwrap_or(node);
            let mut cursor = container.walk();
            container
                .named_children(&mut cursor)
                .filter(|child| child.kind() != "comment")
                .collect()
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

/// Whether a node kind can be the discovered C function itself.
fn is_function_kind(kind: &str) -> bool {
    kind == "function_definition"
}

/// Find the smallest C function node matching the discovery record.
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
        let same_name = function_name(node, source)
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

/// Descend declarator wrappers until the `function_declarator` carrying a name.
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

/// Bare name of a C function definition, found through its declarator chain.
fn function_name(node: Node<'_>, source: &str) -> Option<String> {
    let declarator = find_function_declarator(node.child_by_field_name("declarator")?)?;
    let inner = declarator.child_by_field_name("declarator")?;
    let name = bare_identifier(inner, source);
    (!name.is_empty()).then_some(name)
}

/// First identifier under a declarator, ignoring nested type/qualifier nodes.
fn bare_identifier(node: Node<'_>, source: &str) -> String {
    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "type_identifier"
    ) {
        return normalize_line(&node_text(node, source));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let found = bare_identifier(child, source);
        if !found.is_empty() {
            return found;
        }
    }
    String::new()
}

/// Return the body block of a C function definition.
fn function_body(node: Node<'_>) -> Option<Node<'_>> {
    if let Some(body) = node.child_by_field_name("body") {
        return Some(body);
    }
    let mut cursor = node.walk();
    let body = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "compound_statement");
    body
}

// ---------------------------------------------------------------------------
// Step construction
// ---------------------------------------------------------------------------

/// Everything needed to describe one C statement, gathered in one AST walk.
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

/// Collect the deterministic fields of one C statement.
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

/// Classify a statement into the four contract step kinds (spec §4 step 3).
fn step_kind(statement: Node<'_>) -> StepKind {
    match statement.kind() {
        "return_statement" => StepKind::Return,
        "if_statement" | "switch_statement" => StepKind::Branch,
        // A declaration with a call initializer is a Call step, following the
        // locked `const tokens = tokenize(...)` => `call` example.
        "declaration" => {
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

/// Whether any call expression occurs below this node.
fn has_call(node: Node<'_>) -> bool {
    if node.kind() == "call_expression" {
        return true;
    }
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).any(has_call);
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
    if matches!(node.kind(), "identifier" | "field_identifier") {
        out.push(node_text(node, source));
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

/// Recursive half of [`collect_inputs`]; types and declaration targets are skipped.
fn visit_inputs(node: Node<'_>, source: &str, out: &mut Vec<(usize, String)>) {
    match node.kind() {
        "comment"
        | "type_identifier"
        | "primitive_type"
        | "sized_type_specifier"
        | "type_qualifier"
        | "storage_class_specifier"
        | "struct_specifier"
        | "union_specifier"
        | "enum_specifier"
        | "parameter_list" => return,
        "declaration" => {
            let mut cursor = node.walk();
            for declarator in node.children_by_field_name("declarator", &mut cursor) {
                visit_inputs(declarator, source, out);
            }
            return;
        }
        "init_declarator" => {
            if let Some(value) = node.child_by_field_name("value") {
                visit_inputs(value, source, out);
            }
            return;
        }
        "assignment_expression" => {
            if let Some(right) = node.child_by_field_name("right") {
                visit_inputs(right, source, out);
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

/// The condition of a branch, or `condition` when no condition is named.
fn branch_condition(statement: Node<'_>, source: &str) -> String {
    let node = match statement.child_by_field_name("condition") {
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

/// One assignment target and its declared or inferred display type.
struct Declaration {
    name: String,
    type_: Option<String>,
}

/// First assignment target of a statement, if any.
fn first_declaration(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
) -> Option<Declaration> {
    match statement.kind() {
        "declaration" => {
            let mut cursor = statement.walk();
            let declarator = statement
                .children_by_field_name("declarator", &mut cursor)
                .next()?;
            let name = bare_identifier(declarator, source);
            if name.is_empty() {
                return None;
            }
            let prefix = if declarator.start_byte() > statement.start_byte() {
                normalize_line(&String::from_utf8_lossy(
                    &source.as_bytes()[statement.start_byte()..declarator.start_byte()],
                ))
            } else {
                String::new()
            };
            let type_ = if prefix.is_empty() {
                None
            } else {
                Some(prefix)
            };
            Some(Declaration { name, type_ })
        }
        "expression_statement" => {
            let assignment = statement.named_child(0)?;
            if assignment.kind() != "assignment_expression" {
                return None;
            }
            let name = assignment
                .child_by_field_name("left")
                .map(|left| normalize_line(&node_text(left, source)))
                .unwrap_or_default();
            if name.is_empty() {
                return None;
            }
            let type_ = assignment
                .child_by_field_name("right")
                .map(|right| infer_c_type(&normalize_line(&node_text(right, source)), found));
            Some(Declaration { name, type_ })
        }
        _ => None,
    }
}

/// Conservative display type inferred from a literal or a declared parameter.
fn infer_c_type(text: &str, found: &DiscoveredFn) -> String {
    if text.starts_with('"') {
        return "char *".to_owned();
    }
    match text {
        "NULL" => return "pointer".to_owned(),
        "true" | "false" => return "int".to_owned(),
        _ => {}
    }
    if text.parse::<f64>().is_ok() {
        return if text.contains('.') {
            "double".to_owned()
        } else {
            "int".to_owned()
        };
    }
    found
        .parameters
        .iter()
        .find(|parameter| parameter.name == text && !parameter.type_.trim().is_empty())
        .map(|parameter| parameter.type_.clone())
        .unwrap_or_else(|| "unknown".to_owned())
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

/// Which C construct produced an error fact, used for the `when` fallback.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ErrorKind {
    Fprintf,
    GuardReturn,
    Assert,
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
        "call_expression" => c_error_call(node, source),
        "return_statement" => {
            c_error_return(node, source).map(|code| (code, ErrorKind::GuardReturn))
        }
        _ => None,
    };
    if let Some((code, kind)) = found {
        let enclosing = enclosing_condition(node, source);
        // A bare `return -1` / `return NULL` only counts as an error inside a
        // guard; a normal error return is just a Return step.
        if kind != ErrorKind::GuardReturn || enclosing.is_some() {
            out.push(ErrorSite {
                byte: node.start_byte(),
                code,
                when: enclosing.unwrap_or_else(|| fallback_when(kind)),
            });
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_error_sites(child, source, out);
    }
}

/// Code carried by `fprintf(stderr, "CODE")` or `assert(expr)`.
fn c_error_call(node: Node<'_>, source: &str) -> Option<(String, ErrorKind)> {
    let function = node.child_by_field_name("function")?;
    let target = normalize_line(&node_text(function, source)).replace(' ', "");
    let arguments = node.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let args: Vec<Node<'_>> = arguments.named_children(&mut cursor).collect();
    match target.as_str() {
        "fprintf" => {
            let first = args.first()?;
            if !node_text(*first, source).contains("stderr") {
                return None;
            }
            let literal = args.iter().find(|arg| arg.kind() == "string_literal")?;
            let code = unquote(&node_text(*literal, source));
            (!code.is_empty()).then_some((code, ErrorKind::Fprintf))
        }
        "assert" => {
            if args.is_empty() {
                return None;
            }
            Some(("ASSERT".to_owned(), ErrorKind::Assert))
        }
        _ => None,
    }
}

/// Recognize `return -1` and `return NULL` as guard error returns.
fn c_error_return(node: Node<'_>, source: &str) -> Option<String> {
    let text = node_text(node, source);
    let text = text.trim().trim_end_matches(';').trim();
    match text {
        "return -1" => Some("ERR".to_owned()),
        "return NULL" => Some("NULL".to_owned()),
        _ => None,
    }
}

/// Nearest enclosing `if` condition, or `None` when the site is unguarded.
fn enclosing_condition(node: Node<'_>, source: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "if_statement" {
            if let Some(condition) = parent.child_by_field_name("condition") {
                let text = normalize_line(&node_text(condition, source));
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
        current = parent.parent();
    }
    None
}

/// Deterministic fallback when no enclosing `if` names the trigger.
fn fallback_when(kind: ErrorKind) -> String {
    match kind {
        ErrorKind::Fprintf => "explicit stderr report".to_owned(),
        ErrorKind::GuardReturn => "error return".to_owned(),
        ErrorKind::Assert => "assertion failed".to_owned(),
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
        "EMPTY_TEXT" | "EMPTY_SENTENCE" | "INSECURE_PROTOCOL" | "BAD_REQUEST" | "ERR" | "NULL"
        | "ASSERT" => Some(400),
        "NOT_FOUND" => Some(404),
        "UNAUTHORIZED" => Some(401),
        "FORBIDDEN" => Some(403),
        _ => Some(500),
    }
}

// ---------------------------------------------------------------------------
// Purity, permissions and budgets
// ---------------------------------------------------------------------------

/// Derive C purity, permissions, timeout and runtime from a function node.
fn classify_c_effects(node: Node<'_>, source: &str) -> (Purity, Vec<String>, u64, String) {
    let callees = collect_all_callees(node, source);
    let console = callees
        .iter()
        .any(|name| matches!(name.as_str(), "printf" | "puts" | "fprintf"));
    let read_fs = callees
        .iter()
        .any(|name| matches!(name.as_str(), "fopen" | "fread"));
    let write_fs = callees
        .iter()
        .any(|name| matches!(name.as_str(), "fwrite" | "remove"));
    let network = callees
        .iter()
        .any(|name| matches!(name.as_str(), "socket" | "connect") || name.starts_with("curl_"));
    let env_keys = extract_env_keys(node, source);

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
    // Reuse the contract's runtime labels so core and the UI need no C branch.
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

/// Every bare callee name below a node, in source order.
fn collect_all_callees(node: Node<'_>, source: &str) -> Vec<String> {
    let mut calls = Vec::new();
    visit_calls(node, source, &mut calls);
    calls.into_iter().map(|(_, name)| name).collect()
}

/// Environment variable names read through `getenv("KEY")`.
fn extract_env_keys(node: Node<'_>, source: &str) -> Vec<String> {
    let mut keys = Vec::new();
    visit_env_keys(node, source, &mut keys);
    keys
}

/// Recursive half of [`extract_env_keys`].
fn visit_env_keys(node: Node<'_>, source: &str, out: &mut Vec<String>) {
    if node.kind() == "call_expression" {
        if let Some(function) = node.child_by_field_name("function") {
            if normalize_line(&node_text(function, source)).replace(' ', "") == "getenv" {
                if let Some(arguments) = node.child_by_field_name("arguments") {
                    let mut cursor = arguments.walk();
                    let first = arguments.named_children(&mut cursor).next();
                    if let Some(first) = first {
                        if first.kind() == "string_literal" {
                            let key = unquote(&node_text(first, source));
                            if !key.is_empty() && !out.contains(&key) {
                                out.push(key);
                            }
                        }
                    }
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_env_keys(child, source, out);
    }
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

/// Remove surrounding quotes from a C string literal's source text.
fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    trimmed.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::LanguageAdapter;
    use crate::c_lang::discover::discover;
    use crate::c_lang::CAdapter;

    /// Call, branch, transform and return statements are ordered and classified.
    #[test]
    fn c_analyzes_call_branch_transform_return_steps() {
        let source = r#"
int compute(const char *text) { return 0; }
int analyze(const char *text) {
  // Guard against empty input.
  if (text == NULL) {
    return -1;
  }
  int len = compute(text);
  printf("%d\n", len);
  return len;
}
"#;
        let found = discover(source, "analyze.c");
        let analyze_fn = found
            .iter()
            .find(|item| item.name == "analyze")
            .expect("analyze must be discovered");
        let analysis = CAdapter::new().analyze(analyze_fn, source, &["compute".to_owned()]);
        let kinds: Vec<StepKind> = analysis.steps.iter().map(|step| step.kind).collect();
        // Spec §4 step 3: a declaration with a call initializer is a Call step.
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
            .starts_with("int len = compute(text);"));
        assert_eq!(analysis.steps[1].calls_fn_id.as_deref(), Some("compute"));
        assert_eq!(analysis.callee_names, ["compute", "printf"]);
    }

    /// `fprintf(stderr, ...)`, guarded error returns and `assert` produce codes.
    #[test]
    fn c_detects_fprintf_guard_error_and_assert() {
        let source = r#"
#include <assert.h>
int check(const char *text) {
  if (text == NULL) {
    fprintf(stderr, "EMPTY_TEXT");
    return NULL;
  }
  assert(text != NULL);
  return 0;
}
"#;
        let found = discover(source, "check.c");
        assert_eq!(found.len(), 1);
        let analysis = CAdapter::new().analyze(&found[0], source, &[]);
        let codes: Vec<&str> = analysis
            .errors
            .iter()
            .map(|error| error.code.as_str())
            .collect();
        assert_eq!(codes, ["EMPTY_TEXT", "NULL", "ASSERT"]);
        assert_eq!(analysis.errors[0].status, Some(400));
        assert!(analysis.errors[0].when.contains("text == NULL"));
        assert_eq!(analysis.errors[2].when, "assertion failed");
        assert!(analysis
            .steps
            .iter()
            .any(|step| step.kind == StepKind::Branch
                && step.output == "throws EMPTY_TEXT | continues"));
    }

    /// Console, filesystem, network and secret markers map to permissions.
    #[test]
    fn c_flags_console_fs_net_and_secret_effects() {
        let source = r#"
void work(void) {
  const char *key = getenv("API_KEY");
  printf("%s\n", key);
  FILE *fp = fopen("data.txt", "r");
  fread(key, 1, 1, fp);
  fwrite(key, 1, 1, fp);
  remove("data.txt");
  int fd = socket(1, 1, 0);
  connect(fd, 0, 0);
  curl_easy_perform(0);
}
"#;
        let found = discover(source, "work.c");
        assert_eq!(found.len(), 1);
        let analysis = CAdapter::new().analyze(&found[0], source, &[]);
        assert_eq!(analysis.purity, Purity::SideEffect);
        for permission in [
            "console:out",
            "fs:read",
            "fs:write",
            "net:outbound",
            "secrets:read:API_KEY",
        ] {
            assert!(
                analysis.permissions.contains(&permission.to_owned()),
                "missing permission {permission}"
            );
        }
        assert_eq!(analysis.timeout_ms, 5000);
        assert_eq!(analysis.runtime, "node20");
    }

    /// Repeating an analysis on the same source produces byte-identical facts.
    #[test]
    fn c_analysis_is_deterministic_on_repeat() {
        let source = r#"
int ping(int n) {
  if (n < 1) { return -1; }
  int out = n + 1;
  return out;
}
"#;
        let found = discover(source, "ping.c");
        let adapter = CAdapter::new();
        let first = adapter.analyze(&found[0], source, &[]);
        let second = adapter.analyze(&found[0], source, &[]);
        assert_eq!(first, second);
        assert_eq!(first.steps.len(), 3);
        assert_eq!(first.steps[0].id, "step-1");
        assert_eq!(first.steps[2].id, "step-3");
    }

    /// A pure function keeps the contract's default timeout and runtime labels.
    #[test]
    fn c_pure_function_keeps_default_budget() {
        let source = "int add(int a, int b) { return a + b; }";
        let found = discover(source, "add.c");
        let analysis = CAdapter::new().analyze(&found[0], source, &[]);
        assert_eq!(analysis.purity, Purity::Pure);
        assert!(analysis.permissions.is_empty());
        assert_eq!(analysis.timeout_ms, 150);
        assert_eq!(analysis.runtime, "universal");
    }
}
