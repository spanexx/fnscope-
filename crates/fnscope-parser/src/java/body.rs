//! Deterministic, AST-backed analysis of one Java method or constructor body.
//!
//! The pass reads only the function located by the discovery record: it orders
//! top-level statements, records calls/guards/transforms/returns, extracts
//! thrown error codes and classifies effects. No clock, randomness or map
//! iteration enters the result, so the same input always produces the same
//! `BodyAnalysis`.

use std::collections::BTreeSet;

use fnscope_model::{ErrorSpec, Parameter, Purity, Step, StepKind};
use tree_sitter::{Node, Parser};

use crate::types::{BodyAnalysis, DiscoveredFn};

/// Grammar words that must never be reported as referenced identifiers.
const NON_INPUT_WORDS: &[&str] = &[
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "record",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "true",
    "try",
    "var",
    "void",
    "volatile",
    "while",
    "yield",
];

/// Kinds whose internals belong to another callable and must not be attributed
/// to the step that happens to contain them.
const OPAQUE_KINDS: &[&str] = &[
    "lambda_expression",
    "class_declaration",
    "interface_declaration",
    "record_declaration",
    "enum_declaration",
    "annotation_type_declaration",
];

/// Analyse one discovered Java method and return deterministic execution facts.
///
/// A function node is located by exact source slice, with a line/name fallback.
/// When no node matches, the neutral [`BodyAnalysis::default`] is returned so a
/// partially parsed file cannot corrupt a manifest.
pub fn analyze(found: &DiscoveredFn, source: &str, known_names: &[String]) -> BodyAnalysis {
    let language: tree_sitter::Language = tree_sitter_java::LANGUAGE.into();
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
    // guard or annotation in the header is also considered.
    let (purity, permissions, timeout_ms, runtime) = classify_java_effects(&slice);

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
        Some(node) => block_statements(node),
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

/// Whether a node kind can be the discovered Java function itself.
fn is_function_kind(kind: &str) -> bool {
    matches!(kind, "method_declaration" | "constructor_declaration")
}

/// Find the smallest Java function node matching the discovery record.
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

/// Return the `block` body of a method or constructor declaration.
fn function_body(node: Node<'_>) -> Option<Node<'_>> {
    if let Some(body) = node.child_by_field_name("body") {
        return Some(body);
    }
    let mut cursor = node.walk();
    // Bind the consumer: returning the iterator's item directly would keep the
    // temporary borrowing `cursor` alive past the cursor's drop (E0597).
    let block = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "block");
    block
}

/// Top-level statements of a `block`, in source order.
///
/// `statement` is an abstract supertype in the Java grammar, so concrete
/// statements are direct children of `block`. The wrapper cases are still
/// handled defensively: a `static_initializer` wraps a `block` (not a
/// statement) and a `switch_block` groups statements under
/// `switch_block_statement_group`, so an unexpected wrapper cannot hide a step.
fn block_statements(node: Node<'_>) -> Vec<Node<'_>> {
    let mut items = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "comment" | "line_comment" | "block_comment" => {}
            "switch_block_statement_group" | "switch_rule" | "block_statement_group" => {
                let mut inner_cursor = child.walk();
                for inner in child.named_children(&mut inner_cursor) {
                    if !matches!(
                        inner.kind(),
                        "comment" | "line_comment" | "block_comment" | "switch_label"
                    ) {
                        items.push(inner);
                    }
                }
            }
            _ => items.push(child),
        }
    }
    items
}

// ---------------------------------------------------------------------------
// Step construction
// ---------------------------------------------------------------------------

/// Everything needed to describe one Java statement, gathered in one AST walk.
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

/// Collect the deterministic fields of one Java statement.
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
        "if_statement" | "switch_expression" | "try_statement" | "try_with_resources_statement" => {
            StepKind::Branch
        }
        // A `throw` is a control transfer, but the contract has only four step
        // kinds and reserves `branch` for `if`/`switch`/`try`; the error itself
        // is reported through `ErrorSpec`.
        "throw_statement" | "assert_statement" => StepKind::Transform,
        "local_variable_declaration" | "expression_statement" | "assignment_expression" => {
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

/// Whether any call expression occurs below this node, excluding nested callables.
fn has_call(node: Node<'_>) -> bool {
    if is_call_kind(node.kind()) {
        return true;
    }
    if OPAQUE_KINDS.contains(&node.kind()) {
        return false;
    }
    let mut cursor = node.walk();
    // Bind the consumer for the same reason as `function_body`: the iterator
    // temporary must be dropped before the cursor goes out of scope.
    let found = node
        .named_children(&mut cursor)
        .any(|child| has_call(child));
    found
}

/// Java AST kinds that invoke a callable: a method call or a constructor call.
fn is_call_kind(kind: &str) -> bool {
    matches!(kind, "method_invocation" | "object_creation_expression")
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
    if OPAQUE_KINDS.contains(&node.kind()) {
        return;
    }
    if is_call_kind(node.kind()) {
        if let Some(callee) = callee_bare_name(node, source) {
            out.push((node.start_byte(), callee));
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_calls(child, source, out);
    }
}

/// Bare name of a call target: `a.b.c(...)` reports `c`, `new p.T()` reports `T`.
fn callee_bare_name(call: Node<'_>, source: &str) -> Option<String> {
    let target = if call.kind() == "object_creation_expression" {
        call.child_by_field_name("type")?
    } else {
        call.child_by_field_name("name")
            .or_else(|| call.named_child(0))?
    };
    let text = bare_name(&node_text(target, source));
    (!text.is_empty()).then_some(text)
}

/// Reduce a type or method path to its last identifier, dropping generics.
fn bare_name(text: &str) -> String {
    let mut name = text.trim();
    if let Some(index) = name.find('<') {
        name = &name[..index];
    }
    if let Some(index) = name.rfind('.') {
        name = &name[index + 1..];
    }
    name.trim().to_owned()
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

/// Recursive half of [`collect_inputs`]; declarations and callee names are skipped.
fn visit_inputs(node: Node<'_>, source: &str, out: &mut Vec<(usize, String)>) {
    match node.kind() {
        "comment"
        | "line_comment"
        | "block_comment"
        | "type_identifier"
        | "generic_type"
        | "array_type"
        | "integral_type"
        | "floating_point_type"
        | "boolean_type"
        | "void_type"
        | "type_arguments" => return,
        "local_variable_declaration" | "field_declaration" => {
            // Only the initializer reads values; the declared type is not an input.
            let mut cursor = node.walk();
            for declarator in node.named_children(&mut cursor) {
                if declarator.kind() == "variable_declarator" {
                    if let Some(value) = declarator.child_by_field_name("value") {
                        visit_inputs(value, source, out);
                    }
                }
            }
            return;
        }
        "method_invocation" => {
            if let Some(object) = node.child_by_field_name("object") {
                visit_inputs(object, source, out);
            }
            if let Some(arguments) = node.child_by_field_name("arguments") {
                visit_inputs(arguments, source, out);
            }
            return;
        }
        "field_access" => {
            if let Some(object) = node.child_by_field_name("object") {
                visit_inputs(object, source, out);
            }
            if let Some(field) = node.child_by_field_name("field") {
                let text = node_text(field, source);
                if !NON_INPUT_WORDS.contains(&text.as_str()) {
                    out.push((field.start_byte(), text));
                }
            }
            return;
        }
        "identifier" => {
            let text = node_text(node, source);
            if !NON_INPUT_WORDS.contains(&text.as_str()) {
                out.push((node.start_byte(), text));
            }
            return;
        }
        _ => {}
    }
    if OPAQUE_KINDS.contains(&node.kind()) {
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_inputs(child, source, out);
    }
}

// ---------------------------------------------------------------------------
// Declarations
// ---------------------------------------------------------------------------

/// The condition of a branch, or a deterministic fallback when none is named.
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

/// First assignment target and its declared or inferred display type.
fn first_declaration(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
) -> Option<Declaration> {
    match statement.kind() {
        "local_variable_declaration" => {
            let type_ = statement
                .child_by_field_name("type")
                .map(|type_node| normalize_line(&node_text(type_node, source)))
                .filter(|text| !text.is_empty());
            let mut cursor = statement.walk();
            let declarator = statement
                .children_by_field_name("declarator", &mut cursor)
                .next()?;
            let name = declarator
                .child_by_field_name("name")
                .map(|name| normalize_line(&node_text(name, source)))
                .filter(|name| !name.is_empty())?;
            let inferred = declarator
                .child_by_field_name("value")
                .map(|value| normalize_line(&node_text(value, source)))
                .map(|text| infer_java_type(&text, found));
            Some(Declaration {
                name,
                type_: type_.or(inferred),
            })
        }
        "expression_statement" | "assignment_expression" => {
            let assignment = if statement.kind() == "assignment_expression" {
                statement
            } else {
                statement.named_child(0)?
            };
            if assignment.kind() != "assignment_expression" {
                return None;
            }
            let name = assignment
                .child_by_field_name("left")
                .map(|left| normalize_line(&node_text(left, source)))
                .filter(|name| !name.is_empty())?;
            let type_ = assignment
                .child_by_field_name("right")
                .map(|right| normalize_line(&node_text(right, source)))
                .map(|text| infer_java_type(&text, found));
            Some(Declaration { name, type_ })
        }
        _ => None,
    }
}

/// Conservative display type inferred from a literal or a declared parameter.
fn infer_java_type(text: &str, found: &DiscoveredFn) -> String {
    if text.starts_with('"') {
        return "String".to_owned();
    }
    match text {
        "true" | "false" => return "boolean".to_owned(),
        "null" => return "null".to_owned(),
        _ => {}
    }
    if !text.is_empty() && text.chars().all(|ch| ch.is_ascii_digit()) {
        return "int".to_owned();
    }
    if text.parse::<f64>().is_ok() {
        return "double".to_owned();
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

/// One error site reduced to the facts needed by `ErrorSpec` and branches.
struct ErrorSite {
    byte: usize,
    code: String,
    when: String,
}

/// Walk every descendant of the body and record thrown error sites in source order.
fn collect_error_sites(body: Node<'_>, source: &str) -> Vec<ErrorSite> {
    let mut sites = Vec::new();
    visit_error_sites(body, source, &mut sites);
    sites.sort_by_key(|site| site.byte);
    sites
}

/// Recursive half of [`collect_error_sites`].
fn visit_error_sites(node: Node<'_>, source: &str, out: &mut Vec<ErrorSite>) {
    if node.kind() == "throw_statement" {
        if let Some(code) = java_throw_code(node, source) {
            out.push(ErrorSite {
                byte: node.start_byte(),
                code,
                when: enclosing_condition(node, source),
            });
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_error_sites(child, source, out);
    }
}

/// Code carried by `throw new FunctionError("CODE")` or `throw new X("CODE")`.
fn java_throw_code(node: Node<'_>, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    let expression = node.named_children(&mut cursor).next()?;
    if expression.kind() != "object_creation_expression" {
        return None;
    }
    let arguments = expression.child_by_field_name("arguments")?;
    let mut args_cursor = arguments.walk();
    let first = arguments.named_children(&mut args_cursor).next()?;
    if first.kind() != "string_literal" {
        return None;
    }
    let code = unquote(&node_text(first, source));
    if code.is_empty() {
        None
    } else {
        Some(code)
    }
}

/// Nearest enclosing `if`/`switch` condition, or a deterministic fallback.
fn enclosing_condition(node: Node<'_>, source: &str) -> String {
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(parent.kind(), "if_statement" | "switch_expression") {
            if let Some(condition) = parent.child_by_field_name("condition") {
                let text = normalize_line(&node_text(condition, source));
                if !text.is_empty() {
                    return text;
                }
            }
        }
        current = parent.parent();
    }
    "explicit throw".to_owned()
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

/// Derive Java purity, permissions, timeout and runtime from a function's text.
fn classify_java_effects(slice: &str) -> (Purity, Vec<String>, u64, String) {
    let console = slice.contains("System.out.print") || slice.contains("System.err");
    let fs = slice.contains("java.nio.file") || slice.contains("Files.");
    let fs_write = fs
        && (slice.contains("Files.write")
            || slice.contains("Files.create")
            || slice.contains("Files.delete")
            || slice.contains("Files.copy")
            || slice.contains("Files.move")
            || slice.contains("Files.newOutputStream"));
    let network = slice.contains("java.net")
        || slice.contains("HttpClient")
        || slice.contains("HttpURLConnection")
        || slice.contains("java.net.Socket");
    let env_keys = extract_java_env_keys(slice);

    let mut permissions = BTreeSet::new();
    if console {
        permissions.insert("console:out".to_owned());
    }
    if fs {
        permissions.insert("fs:read".to_owned());
    }
    if fs_write {
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
    // Reuse the contract's runtime labels so core and the UI need no Java branch.
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

/// Environment variable names read through `System.getenv("KEY")`.
fn extract_java_env_keys(slice: &str) -> Vec<String> {
    let needle = "System.getenv(";
    let mut keys = Vec::new();
    let mut offset = 0usize;
    while let Some(index) = slice[offset..].find(needle) {
        let start = offset + index + needle.len();
        let rest = &slice[start..];
        if !rest.starts_with('"') {
            offset = start;
            continue;
        }
        let after = &rest[1..];
        let end = match after.find('"') {
            Some(end) => end,
            None => break,
        };
        let key = &after[..end];
        if !key.is_empty() && !keys.contains(&key.to_owned()) {
            keys.push(key.to_owned());
        }
        offset = start + 1 + end + 1;
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

/// Remove surrounding quotes from a Java string literal's source text.
fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    trimmed.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::discover::discover;
    use crate::types::FnKind;

    /// Call, branch, transform and return statements are ordered and classified.
    #[test]
    fn java_analyzes_call_branch_transform_return_steps() {
        let source = r#"
class Service {
    String normalize(String text) { return text.trim(); }
    String analyze(String text) {
        // Guard against empty input.
        if (text.isEmpty()) { return ""; }
        String cleaned = normalize(text);
        System.out.println(cleaned);
        int length = 1 + 2;
        return cleaned;
    }
}
"#;
        let found = discover(source, "Service.java");
        let analyze_fn = found
            .iter()
            .find(|item| item.name == "analyze")
            .expect("analyze must be discovered");
        assert_eq!(analyze_fn.kind, FnKind::Method);

        let analysis = analyze(analyze_fn, source, &["normalize".to_owned()]);
        let kinds: Vec<StepKind> = analysis.steps.iter().map(|step| step.kind).collect();
        // Spec section 4 step 3 and the locked schema example
        // `const tokens = tokenize(...)` => `call` govern the initializer call.
        // Only top-level statements count; the `return ""` inside the guard is
        // part of the branch step, not a separate step.
        assert_eq!(
            kinds,
            [
                StepKind::Branch,
                StepKind::Call,
                StepKind::Call,
                StepKind::Transform,
                StepKind::Return
            ]
        );
        assert_eq!(analysis.steps[0].title, "Guard against empty input.");
        assert!(analysis.steps[1]
            .code
            .starts_with("String cleaned = normalize(text)"));
        assert_eq!(analysis.steps[1].calls_fn_id.as_deref(), Some("normalize"));
        assert_eq!(analysis.steps[3].kind, StepKind::Transform);
        // The guard's `text.isEmpty()` is also a real method invocation, so it
        // precedes the initializer call in source order.
        assert_eq!(analysis.callee_names, ["isEmpty", "normalize", "println"]);
    }

    /// `throw new X("CODE")` produces deduplicated codes with status and `when`.
    #[test]
    fn java_extracts_throw_error_codes() {
        let source = r#"
class Check {
    String check(String text) {
        if (text.isEmpty()) { throw new IllegalArgumentException("EMPTY_TEXT"); }
        if (text.equals("x")) { throw new FunctionError("TOKEN_NOT_FOUND"); }
        if (text.equals("y")) { throw new FunctionError("TOKEN_NOT_FOUND"); }
        return text;
    }
}
"#;
        let found = discover(source, "Check.java");
        let analysis = analyze(&found[0], source, &[]);
        assert_eq!(analysis.errors.len(), 2);
        assert_eq!(analysis.errors[0].code, "EMPTY_TEXT");
        assert_eq!(analysis.errors[0].status, Some(400));
        // Spec section 4 step 4 keeps the enclosing `if (...)` text verbatim;
        // rust/python/typescript adapters report `when` the same way.
        assert_eq!(analysis.errors[0].when, "(text.isEmpty())");
        assert_eq!(analysis.errors[1].code, "TOKEN_NOT_FOUND");
        assert_eq!(analysis.errors[1].status, Some(422));
        assert!(analysis
            .steps
            .iter()
            .any(|step| step.kind == StepKind::Branch
                && step.output == "throws EMPTY_TEXT | continues"));
    }

    /// A top-level throw without an enclosing `if` uses a deterministic `when`.
    #[test]
    fn java_extracts_throw_without_guard() {
        let source = r#"class C { void fail() { throw new RuntimeException("BROKEN"); } }"#;
        let found = discover(source, "C.java");
        let analysis = analyze(&found[0], source, &[]);
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "BROKEN");
        assert_eq!(analysis.errors[0].status, Some(500));
        assert_eq!(analysis.errors[0].when, "explicit throw");
    }

    /// Console output is a side effect with the contract runtime and timeout.
    #[test]
    fn java_flags_system_out_as_console_side_effect() {
        let source = "class C { void report(String text) { System.out.println(text); } }";
        let found = discover(source, "C.java");
        let analysis = analyze(&found[0], source, &[]);
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
    fn java_maps_effects_to_permissions() {
        let source = r#"class C {
    String load() {
        String key = System.getenv("API_KEY");
        String body = Files.readString(Paths.get("data.txt"));
        HttpClient client = HttpClient.newHttpClient();
        return key + body + client;
    }
}
"#;
        let found = discover(source, "C.java");
        let analysis = analyze(&found[0], source, &[]);
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
    fn java_analysis_is_deterministic_on_repeat() {
        let source = r#"
class C {
    int ping(int n) {
        if (n < 1) { return 0; }
        int out = n + 1;
        return out;
    }
}
"#;
        let found = discover(source, "C.java");
        let first = analyze(&found[0], source, &[]);
        let second = analyze(&found[0], source, &[]);
        assert_eq!(first, second);
        assert_eq!(first.steps.len(), 3);
        assert_eq!(first.steps[0].id, "step-1");
        assert_eq!(first.steps[2].id, "step-3");
    }

    /// A pure function keeps the contract's default timeout and runtime labels.
    #[test]
    fn java_pure_function_keeps_default_budget() {
        let source = "class C { int add(int a, int b) { return a + b; } }";
        let found = discover(source, "C.java");
        let analysis = analyze(&found[0], source, &[]);
        assert_eq!(analysis.purity, Purity::Pure);
        assert!(analysis.permissions.is_empty());
        assert_eq!(analysis.timeout_ms, 150);
        assert_eq!(analysis.runtime, "universal");
    }

    /// Interface method signatures have no body and therefore no steps.
    #[test]
    fn java_interface_signature_has_no_steps() {
        let source = "interface R { String read(int id); }";
        let found = discover(source, "R.java");
        let analysis = analyze(&found[0], source, &["read".to_owned()]);
        assert!(analysis.steps.is_empty());
        assert!(analysis.callee_names.is_empty());
        assert_eq!(analysis.purity, Purity::Pure);
    }
}
