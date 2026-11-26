//! Deterministic, AST-backed analysis of one PHP function body.
//!
//! Only the direct statements of the declaration's `compound_statement` become
//! steps, in source order. Calls, guards, transforms and returns follow the
//! locked §4 mapping; throws and side-effect markers are collected from the
//! whole function slice. No clock, randomness or map iteration enters a result.

use std::collections::BTreeSet;

use fnscope_model::{ErrorSpec, Parameter, Purity, Step, StepKind};
use tree_sitter::{Node, Parser};

use crate::types::{BodyAnalysis, DiscoveredFn};

/// PHP keywords and built-ins that are never meaningful step inputs.
const NON_INPUT_WORDS: &[&str] = &[
    "if",
    "else",
    "elseif",
    "endif",
    "switch",
    "case",
    "default",
    "endswitch",
    "try",
    "catch",
    "finally",
    "throw",
    "return",
    "echo",
    "print",
    "new",
    "function",
    "class",
    "public",
    "private",
    "protected",
    "static",
    "const",
    "true",
    "false",
    "null",
    "and",
    "or",
    "xor",
    "instanceof",
    "as",
    "foreach",
    "for",
    "while",
    "do",
    "break",
    "continue",
    "isset",
    "empty",
    "array",
    "self",
    "parent",
    "namespace",
    "use",
    "fn",
];

/// Call node kinds in this grammar; argument nodes are never callees.
const CALL_KINDS: &[&str] = &[
    "function_call_expression",
    "member_call_expression",
    "nullsafe_member_call_expression",
    "scoped_call_expression",
];

/// Analyze one discovered function into ordered, deterministic facts.
///
/// The declaration node is located by exact source-slice match with a line+name
/// fallback; a miss returns the neutral `BodyAnalysis::default()` so a partially
/// discovered file cannot corrupt a manifest.
pub fn analyze(found: &DiscoveredFn, source: &str, known_names: &[String]) -> BodyAnalysis {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_php::LANGUAGE_PHP.into();
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
    let (purity, permissions, timeout_ms, runtime) = classify_php_effects(&slice);

    let body = function.child_by_field_name("body");
    let error_sites = collect_throw_sites(body.unwrap_or(function), source);
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
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .map(unwrap_statement)
                .filter(|child| {
                    !matches!(
                        child.kind(),
                        "comment" | "text" | "php_tag" | "empty_statement"
                    )
                })
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

/// Find the tightest declaration matching the recorded slice or line+name.
fn find_function_node<'tree>(
    root: Node<'tree>,
    source: &str,
    found: &DiscoveredFn,
) -> Option<Node<'tree>> {
    let mut best: Option<Node<'tree>> = None;
    visit_function_nodes(root, source, found, &mut best);
    best
}

/// Depth-first walk that keeps the smallest matching declaration node.
fn visit_function_nodes<'tree>(
    node: Node<'tree>,
    source: &str,
    found: &DiscoveredFn,
    best: &mut Option<Node<'tree>>,
) {
    if matches!(node.kind(), "function_definition" | "method_declaration") {
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

/// Descend through a hidden `statement` / `expression` supertype wrapper.
fn unwrap_statement<'tree>(mut node: Node<'tree>) -> Node<'tree> {
    while matches!(node.kind(), "statement" | "expression") {
        let mut cursor = node.walk();
        // Bind the consumer so the iterator borrowing `cursor` is dropped first.
        let first = node.named_children(&mut cursor).next();
        match first {
            Some(child) => node = child,
            None => break,
        }
    }
    node
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

/// Look through an `expression_statement` wrapper to the expression it holds.
fn statement_expression<'tree>(mut node: Node<'tree>) -> Node<'tree> {
    while matches!(node.kind(), "expression_statement" | "statement") {
        let mut cursor = node.walk();
        // Bind the consumer so the iterator borrowing `cursor` is dropped first.
        let first = node.named_children(&mut cursor).next();
        match first {
            Some(child) => node = child,
            None => break,
        }
    }
    node
}

/// Whether a node is one of the grammar's explicit call expressions.
fn is_call(node: Node<'_>) -> bool {
    CALL_KINDS.contains(&node.kind())
}

/// Classify a statement according to the locked §4 step-3 mapping.
fn classify_statement(statement: Node<'_>) -> StepKind {
    let core = statement_expression(statement);
    match core.kind() {
        // `echo` writes to the console; it is a Call step, not a Return.
        "echo_statement" | "print_intrinsic" => StepKind::Call,
        "return_statement" => StepKind::Return,
        "if_statement" | "else_if_clause" | "switch_statement" | "try_statement"
        | "for_statement" | "foreach_statement" | "while_statement" | "do_statement" => {
            StepKind::Branch
        }
        // An assignment whose right-hand side calls a function is a Call step.
        "assignment_expression" | "augmented_assignment_expression" => {
            if right_has_call(core) {
                StepKind::Call
            } else {
                StepKind::Transform
            }
        }
        "throw_expression" => StepKind::Transform,
        _ => {
            if is_call(core) || has_call(core) {
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
    error_sites: &[ThrowSite],
) -> (Step, Vec<String>) {
    let mut facts = statement_facts(statement, source, found, error_sites);
    let calls = collect_calls(statement, source);
    let callee_names: Vec<String> = calls.iter().map(|(_, name)| name.clone()).collect();
    let calls_fn_id = if facts.kind == StepKind::Call || facts.callee.as_deref() == Some("echo") {
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
        StepKind::Branch => match facts.guard_error.as_deref() {
            Some(code) => format!("throws {code} | continues"),
            None => format!("{} | continues", facts.condition),
        },
        _ => facts
            .declared_type
            .clone()
            .unwrap_or_else(|| "void".to_owned()),
    };

    let (comment_title, comment_detail) =
        preceding_slash_comments(source, statement.start_position().row);
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
    error_sites: &[ThrowSite],
) -> StatementFacts {
    let code = normalize_line(&node_text(&statement, source));
    let kind = classify_statement(statement);
    let core = statement_expression(statement);
    let assigned_name = assignment_left(core, source);
    let declared_type = declared_type(core, source, found, assigned_name.as_deref());
    let condition = condition_text(core, source);
    let calls = collect_calls(statement, source);
    let callee = if matches!(core.kind(), "echo_statement" | "print_intrinsic") {
        Some("echo".to_owned())
    } else {
        calls.first().map(|(_, name)| name.clone())
    };
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
fn assignment_left(node: Node<'_>, source: &str) -> Option<String> {
    if !matches!(
        node.kind(),
        "assignment_expression" | "augmented_assignment_expression"
    ) {
        return None;
    }
    node.child_by_field_name("left")
        .map(|node| normalize_line(&node_text(&node, source)))
        .filter(|text| !text.is_empty())
}

/// Annotation when present, otherwise a conservative inference from the value.
fn declared_type(
    node: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
    assigned_name: Option<&str>,
) -> Option<String> {
    let name = assigned_name?;
    if let Some(annotation) = node
        .child_by_field_name("type")
        .map(|node| normalize_line(&node_text(&node, source)))
        .filter(|text| !text.is_empty())
    {
        return Some(format!("{name}: {}", annotation.trim_start_matches(':')));
    }
    let value = node.child_by_field_name("right")?;
    Some(format!("{name}: {}", infer_type(value, source, found)))
}

/// Infer a display type from a PHP literal, operator expression or parameter copy.
fn infer_type(value: Node<'_>, source: &str, found: &DiscoveredFn) -> String {
    match value.kind() {
        "string" | "encapsed_string" | "heredoc" | "nowdoc" => "string".to_owned(),
        "integer" => "int".to_owned(),
        "float" => "float".to_owned(),
        "boolean" => "bool".to_owned(),
        "null" => "null".to_owned(),
        "array_creation_expression" => "array".to_owned(),
        "object_creation_expression" => first_expression_text(value, source),
        "binary_expression" => infer_binary_type(value, source, found),
        "unary_op_expression" | "update_expression" => infer_unary_type(value, source, found),
        "parenthesized_expression" | "error_suppression_expression" => {
            infer_wrapped_type(value, source, found)
        }
        "cast_expression" => infer_cast_type(value, source),
        "variable_name" => {
            let text = node_text(&value, source);
            let bare = text.trim().trim_start_matches('$');
            parameter_type(bare, &found.parameters).unwrap_or_else(|| "unknown".to_owned())
        }
        // A call's result cannot be inferred without cross-function data.
        _ => "unknown".to_owned(),
    }
}

/// Infer a binary expression's type from its operator and operand types.
fn infer_binary_type(node: Node<'_>, source: &str, found: &DiscoveredFn) -> String {
    let operator = node
        .child_by_field_name("operator")
        .map(|op| normalize_line(&node_text(&op, source)))
        .unwrap_or_default();
    if operator == "." {
        return "string".to_owned();
    }
    if super::is_comparison_operator(&operator) {
        return "bool".to_owned();
    }
    let left = node.child_by_field_name("left");
    let right = node.child_by_field_name("right");
    let left_type = left
        .map(|inner| infer_type(inner, source, found))
        .unwrap_or_default();
    if operator == "??" {
        return if left_type.is_empty() || left_type == "unknown" {
            right
                .map(|inner| infer_type(inner, source, found))
                .unwrap_or_else(|| "unknown".to_owned())
        } else {
            left_type
        };
    }
    let right_type = right
        .map(|inner| infer_type(inner, source, found))
        .unwrap_or_default();
    match (left_type.as_str(), right_type.as_str()) {
        ("float", _) | (_, "float") => "float".to_owned(),
        ("int", _) | (_, "int") => "int".to_owned(),
        (left, right) if !left.is_empty() && left == right => left.to_owned(),
        _ => "unknown".to_owned(),
    }
}

/// Infer a unary operator's type: `!` is boolean, other operators keep the operand type.
fn infer_unary_type(node: Node<'_>, source: &str, found: &DiscoveredFn) -> String {
    let operator = node
        .child_by_field_name("operator")
        .map(|op| normalize_line(&node_text(&op, source)))
        .unwrap_or_default();
    if operator == "!" {
        return "bool".to_owned();
    }
    let argument = node
        .child_by_field_name("argument")
        .or_else(|| node.named_child(0));
    argument
        .map(|inner| infer_type(inner, source, found))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Infer through a `parenthesized_expression` or `@` suppression wrapper.
fn infer_wrapped_type(node: Node<'_>, source: &str, found: &DiscoveredFn) -> String {
    let mut cursor = node.walk();
    // Bind the consumer so the iterator borrowing `cursor` is dropped first.
    let inner = node.named_children(&mut cursor).next();
    match inner {
        Some(child) => infer_type(child, source, found),
        None => "unknown".to_owned(),
    }
}

/// Infer an explicit PHP cast's target type from its `cast_type` field.
fn infer_cast_type(node: Node<'_>, source: &str) -> String {
    node.child_by_field_name("type")
        .map(|cast| normalize_line(&node_text(&cast, source)))
        .map(|text| {
            let target = strip_outer_parens(&text);
            match target.as_str() {
                "int" | "integer" => "int".to_owned(),
                "float" | "double" | "real" => "float".to_owned(),
                "bool" | "boolean" => "bool".to_owned(),
                "string" | "binary" => "string".to_owned(),
                "array" => "array".to_owned(),
                "object" => "object".to_owned(),
                other => other.to_owned(),
            }
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// First named child of a node that is not a wrapper, as normalized text.
fn first_expression_text(node: Node<'_>, source: &str) -> String {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() != "arguments" {
            return normalize_line(&node_text(&child, source));
        }
    }
    "unknown".to_owned()
}

/// Look up a declared parameter type for simple variable-to-variable copies.
fn parameter_type(name: &str, parameters: &[Parameter]) -> Option<String> {
    parameters
        .iter()
        .find(|parameter| parameter.name == name && !parameter.type_.trim().is_empty())
        .map(|parameter| parameter.type_.clone())
}

/// Condition of an `if`/`switch`/loop, with one layer of parentheses removed.
fn condition_text(node: Node<'_>, source: &str) -> String {
    node.child_by_field_name("condition")
        .map(|node| strip_outer_parens(&normalize_line(&node_text(&node, source))))
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

/// Whether the assignment's right-hand side contains a call expression.
fn right_has_call(node: Node<'_>) -> bool {
    node.child_by_field_name("right")
        .map(|right| has_call(right))
        .unwrap_or(false)
}

/// Whether any call expression occurs below a node, excluding nested declarations.
fn has_call(node: Node<'_>) -> bool {
    if is_call(node) {
        return true;
    }
    let mut cursor = node.walk();
    // Bind the consumer so the iterator borrowing `cursor` is dropped first.
    let found = node.named_children(&mut cursor).any(|child| {
        !matches!(
            child.kind(),
            "function_definition"
                | "method_declaration"
                | "anonymous_function"
                | "arrow_function"
                | "class_declaration"
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

/// Recursive half of [`collect_calls`]; nested declarations belong to another body.
fn visit_calls(node: Node<'_>, source: &str, out: &mut Vec<(usize, String)>) {
    if is_call(node) {
        if let Some(callee) = callee_bare_name(node, source) {
            out.push((node.start_byte(), callee));
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if matches!(
            child.kind(),
            "function_definition"
                | "method_declaration"
                | "anonymous_function"
                | "arrow_function"
                | "class_declaration"
        ) {
            continue;
        }
        visit_calls(child, source, out);
    }
}

/// Bare callee of a PHP call: the last identifier before its `arguments` child.
fn callee_bare_name(call: Node<'_>, source: &str) -> Option<String> {
    let target = call
        .child_by_field_name("function")
        .or_else(|| call.child_by_field_name("name"))
        .unwrap_or(call);
    let mut names = Vec::new();
    collect_callee_identifiers(target, source, &mut names);
    names.last().cloned()
}

/// Identifiers of a callee expression, stopping at the `arguments` wrapper.
fn collect_callee_identifiers(node: Node<'_>, source: &str, out: &mut Vec<String>) {
    if node.kind() == "arguments" {
        return;
    }
    match node.kind() {
        "name" => {
            out.push(node_text(&node, source));
            return;
        }
        "variable_name" => {
            if let Some(inner) = node.named_child(0) {
                out.push(node_text(&inner, source));
            }
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_callee_identifiers(child, source, out);
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
        "comment" | "text" | "php_tag" | "string" | "encapsed_string" | "integer" | "float"
        | "boolean" | "null" | "type" => return,
        "assignment_expression" | "augmented_assignment_expression" => {
            if let Some(right) = node.child_by_field_name("right") {
                visit_inputs(right, source, out);
            }
            return;
        }
        "variable_name" => {
            let text = node_text(&node, source);
            let bare = text.trim().trim_start_matches('$').trim();
            if !bare.is_empty() && bare != "this" && !NON_INPUT_WORDS.contains(&bare) {
                out.push((node.start_byte(), bare.to_owned()));
            }
            return;
        }
        "name" => {
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

/// Top two contiguous `//` lines directly above a statement.
fn preceding_slash_comments(
    source: &str,
    statement_row: usize,
) -> (Option<String>, Option<String>) {
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

/// One `throw` expression reduced to the facts needed by `ErrorSpec` and branches.
struct ThrowSite {
    byte: usize,
    code: String,
    when: String,
}

/// Walk every descendant of the body and record `throw` sites in source order.
fn collect_throw_sites(body: Node<'_>, source: &str) -> Vec<ThrowSite> {
    let mut sites = Vec::new();
    visit_throws(body, source, &mut sites);
    sites.sort_by_key(|site| site.byte);
    sites
}

/// Recursive half of [`collect_throw_sites`].
fn visit_throws(node: Node<'_>, source: &str, out: &mut Vec<ThrowSite>) {
    if node.kind() == "throw_expression" {
        if let Some(code) = throw_code(node, source) {
            out.push(ThrowSite {
                byte: node.start_byte(),
                code,
                when: enclosing_condition(node, source),
            });
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_throws(child, source, out);
    }
}

/// Code carried by `throw new X("CODE")` or by a bare `throw new X`.
fn throw_code(node: Node<'_>, source: &str) -> Option<String> {
    if let Some(code) = first_string(node, source) {
        return Some(code);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "object_creation_expression" {
            let text = first_expression_text(child, source);
            let last = text.rsplit('\\').next().unwrap_or(&text).trim().to_owned();
            if !last.is_empty() {
                return Some(last);
            }
        }
    }
    None
}

/// First string literal inside a subtree, using source order for determinism.
fn first_string(node: Node<'_>, source: &str) -> Option<String> {
    if matches!(
        node.kind(),
        "string" | "encapsed_string" | "heredoc" | "nowdoc"
    ) {
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

/// Nearest enclosing `if` / `elseif` condition, or `explicit throw`.
fn enclosing_condition(throw: Node<'_>, source: &str) -> String {
    let mut current = throw.parent();
    while let Some(node) = current {
        if matches!(node.kind(), "if_statement" | "else_if_clause") {
            if let Some(condition) = node.child_by_field_name("condition") {
                let text = strip_outer_parens(&normalize_line(&node_text(&condition, source)));
                if !text.is_empty() {
                    return text;
                }
            }
        }
        current = node.parent();
    }
    "explicit throw".to_owned()
}

/// Deduplicate throw sites by code, keeping the first occurrence and status.
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

/// Derive PHP purity, permissions, timeout and runtime from a function's slice.
fn classify_php_effects(slice: &str) -> (Purity, Vec<String>, u64, String) {
    let lower = slice.to_ascii_lowercase();
    let console = lower.contains("echo ")
        || lower.contains("echo(")
        || lower.contains("print(")
        || lower.contains("printf(")
        || lower.contains("print_r(")
        || lower.contains("var_dump(");
    let fs_read = lower.contains("file_get_contents(")
        || lower.contains("fopen(")
        || lower.contains("readfile(")
        || lower.contains("scandir(");
    let fs_write = lower.contains("file_put_contents(")
        || lower.contains("fwrite(")
        || lower.contains("unlink(")
        || lower.contains("mkdir(");
    let net = lower.contains("curl_")
        || lower.contains("stream_socket_client(")
        || (lower.contains("file_get_contents(")
            && (lower.contains("http://") || lower.contains("https://")));
    let env_keys = extract_env_keys(slice);

    let mut permissions = BTreeSet::new();
    if console {
        permissions.insert("console:out".to_owned());
    }
    if fs_read {
        permissions.insert("fs:read".to_owned());
    }
    if fs_write {
        permissions.insert("fs:write".to_owned());
    }
    if net {
        permissions.insert("net:outbound".to_owned());
    }
    for key in &env_keys {
        permissions.insert(format!("secrets:read:{key}"));
    }

    let side_effect = console || fs_read || fs_write || net || !env_keys.is_empty();
    let purity = if side_effect {
        Purity::SideEffect
    } else {
        Purity::Pure
    };
    // Budgets and runtime labels match the existing adapters' contract values.
    let timeout_ms = if !side_effect {
        150
    } else if net {
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

/// Environment keys read via `getenv("KEY")` or `$_ENV["KEY"]`.
fn extract_env_keys(slice: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let markers = ["getenv(\"", "getenv('", "$_ENV[\"", "$_ENV['"];
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

/// Remove exactly one outer pair of parentheses from a condition.
fn strip_outer_parens(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('(') && trimmed.ends_with(')') {
        return trimmed[1..trimmed.len() - 1].trim().to_owned();
    }
    trimmed.to_owned()
}

/// Remove one layer of PHP string quotes.
fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'\'' | b'"') && bytes[bytes.len() - 1] == bytes[0] {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    trimmed.to_owned()
}
