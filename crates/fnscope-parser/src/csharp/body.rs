//! Deterministic, AST-backed analysis of one C# callable body.
//!
//! The pass reads only the source slice of a discovered function. It orders the
//! top-level statements of a `block`, records calls/guards/transforms/returns,
//! extracts `throw new` error codes and classifies effects. No clock, randomness
//! or map iteration enters the result, so identical input yields identical facts.

use std::collections::BTreeSet;

use fnscope_model::{ErrorSpec, Parameter, Purity, Step, StepKind};
use tree_sitter::{Node, Parser};

use crate::types::{BodyAnalysis, DiscoveredFn};

use super::discover::value_after_equals;

/// C# keywords and built-in type names that are never referenced inputs.
const NON_INPUT_WORDS: &[&str] = &[
    "return",
    "if",
    "else",
    "switch",
    "case",
    "default",
    "try",
    "catch",
    "finally",
    "throw",
    "new",
    "var",
    "int",
    "string",
    "bool",
    "double",
    "float",
    "long",
    "short",
    "byte",
    "char",
    "object",
    "void",
    "decimal",
    "uint",
    "ulong",
    "true",
    "false",
    "null",
    "this",
    "base",
    "class",
    "struct",
    "record",
    "interface",
    "enum",
    "namespace",
    "using",
    "public",
    "private",
    "protected",
    "internal",
    "static",
    "readonly",
    "const",
    "async",
    "await",
    "in",
    "out",
    "ref",
    "is",
    "as",
    "foreach",
    "for",
    "while",
    "do",
    "break",
    "continue",
    "lock",
    "checked",
    "unchecked",
    "typeof",
    "sizeof",
    "nameof",
    "get",
    "set",
    "yield",
    "params",
    "sealed",
];

/// Nodes whose bodies belong to another callable and are not descended into.
const OPAQUE_KINDS: &[&str] = &[
    "local_function_statement",
    "class_declaration",
    "struct_declaration",
    "record_declaration",
    "interface_declaration",
    "enum_declaration",
];

/// Analyze one discovered callable into ordered, deterministic facts.
///
/// The node is located by exact source slice with a line+name fallback. A miss
/// returns the neutral `BodyAnalysis::default()` so a partial file cannot
/// corrupt a manifest.
pub fn analyze(found: &DiscoveredFn, source: &str, known_names: &[String]) -> BodyAnalysis {
    let language: tree_sitter::Language = tree_sitter_c_sharp::LANGUAGE.into();
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return BodyAnalysis::default();
    }
    let tree = match parser.parse(source, None) {
        Some(tree) => tree,
        None => return BodyAnalysis::default(),
    };
    let function = match find_function_node(tree.root_node(), source, found) {
        Some(node) => node,
        None => return BodyAnalysis::default(),
    };
    let slice = node_text(&function, source);
    let (purity, permissions, timeout_ms, runtime) = classify_effects(&slice);

    let body = function_body(function);
    let error_sites = match body {
        Some(node) => collect_throw_sites(node, source),
        None => Vec::new(),
    };
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

    let implicit_return = body.is_some_and(|node| node.kind() == "arrow_expression_clause");
    let statements: Vec<Node<'_>> = match body {
        Some(node) if node.kind() == "block" => block_statements(node),
        Some(node) if node.kind() == "arrow_expression_clause" => {
            node.named_child(0).into_iter().collect()
        }
        _ => Vec::new(),
    };

    for (index, statement) in statements.iter().enumerate() {
        let (step, calls) = build_step(
            *statement,
            source,
            found,
            known_names,
            index + 1,
            &error_sites,
            implicit_return,
        );
        analysis.steps.push(step);
        analysis.callee_names.extend(calls);
    }
    analysis
}

// ---------------------------------------------------------------------------
// Function location
// ---------------------------------------------------------------------------

/// Find the tightest callable node matching the recorded slice or line+name.
fn find_function_node<'tree>(
    root: Node<'tree>,
    source: &str,
    found: &DiscoveredFn,
) -> Option<Node<'tree>> {
    let mut best: Option<Node<'tree>> = None;
    visit_nodes(root, source, found, &mut best);
    best
}

/// Depth-first walk that keeps the smallest matching callable node.
fn visit_nodes<'tree>(
    node: Node<'tree>,
    source: &str,
    found: &DiscoveredFn,
    best: &mut Option<Node<'tree>>,
) {
    if node_matches(node, source, found) {
        let better = best.map_or(true, |current| {
            node_range_len(node) < node_range_len(current)
        });
        if better {
            *best = Some(node);
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_nodes(child, source, found, best);
    }
}

/// Whether a node is the callable described by `found`.
fn node_matches(node: Node<'_>, source: &str, found: &DiscoveredFn) -> bool {
    if !matches!(
        node.kind(),
        "method_declaration"
            | "constructor_declaration"
            | "local_function_statement"
            | "class_declaration"
            | "struct_declaration"
            | "record_declaration"
    ) {
        return false;
    }
    let text = node_text(&node, source);
    if text == found.source_code || text.trim() == found.source_code.trim() {
        return true;
    }
    if node.start_position().row + 1 != found.line {
        return false;
    }
    let name = node
        .child_by_field_name("name")
        .map(|name| node_text(&name, source))
        .unwrap_or_default();
    if name != found.name {
        return false;
    }
    if matches!(
        node.kind(),
        "class_declaration" | "struct_declaration" | "record_declaration"
    ) {
        // A primary constructor's slice is a header prefix, so only a type
        // header that actually declares a parameter list can match it.
        return primary_parameter_list(node).is_some();
    }
    true
}

/// Byte span of a node, used only as a deterministic tie-breaker.
fn node_range_len(node: Node<'_>) -> usize {
    node.end_byte().saturating_sub(node.start_byte())
}

/// The `parameter_list` child used by a C# 12 primary constructor.
fn primary_parameter_list(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    let found = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "parameter_list");
    found
}

/// The executable body of a callable; primary constructors have none.
fn function_body(node: Node<'_>) -> Option<Node<'_>> {
    if matches!(
        node.kind(),
        "class_declaration" | "struct_declaration" | "record_declaration"
    ) {
        return None;
    }
    node.child_by_field_name("body")
        .filter(|node| matches!(node.kind(), "block" | "arrow_expression_clause"))
}

/// Direct statements of a `block`, excluding comments and nested local functions.
fn block_statements(body: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = body.walk();
    body.named_children(&mut cursor)
        .filter(|child| child.kind() != "comment" && child.kind() != "local_function_statement")
        .collect()
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

/// Classify a statement by its C# node kind and initializer shape.
fn classify_statement(statement: Node<'_>) -> StepKind {
    match statement.kind() {
        "if_statement" | "switch_statement" | "try_statement" => StepKind::Branch,
        "return_statement" => StepKind::Return,
        "local_declaration_statement" => {
            if initializer_has_call(statement) {
                StepKind::Call
            } else {
                StepKind::Transform
            }
        }
        "expression_statement" => {
            if has_call(statement) {
                StepKind::Call
            } else {
                StepKind::Transform
            }
        }
        // A top-level throw is a transform; its failure lives in `errors`.
        "throw_statement" => StepKind::Transform,
        _ => {
            if has_call(statement) {
                StepKind::Call
            } else {
                StepKind::Transform
            }
        }
    }
}

/// Build one ordered step plus the raw callee names encountered in it.
#[allow(clippy::too_many_arguments)]
fn build_step(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
    known_names: &[String],
    index: usize,
    error_sites: &[ThrowSite],
    implicit_return: bool,
) -> (Step, Vec<String>) {
    let mut facts = statement_facts(statement, source, found, error_sites);
    if implicit_return {
        facts.kind = StepKind::Return;
    }
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

/// Collect the deterministic fields of one statement.
fn statement_facts(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
    error_sites: &[ThrowSite],
) -> StatementFacts {
    let kind = classify_statement(statement);
    let assigned_name = assignment_left(statement, source);
    let calls = collect_calls(statement, source);
    let guard_error = error_sites
        .iter()
        .find(|site| site.byte >= statement.start_byte() && site.byte < statement.end_byte())
        .map(|site| site.code.clone());
    StatementFacts {
        kind,
        code: normalize_line(&node_text(&statement, source)),
        inputs: collect_inputs(statement, source),
        declared_type: declared_type(statement, source, found, assigned_name.as_deref()),
        condition: condition_text(statement, source),
        callee: calls.first().map(|(_, name)| name.clone()),
        assigned_name,
        guard_error,
    }
}

/// Left-hand target of an assignment or a local variable declaration.
fn assignment_left(statement: Node<'_>, source: &str) -> Option<String> {
    match statement.kind() {
        "local_declaration_statement" => {
            let mut cursor = statement.walk();
            let declarator = statement
                .named_children(&mut cursor)
                .find(|child| child.kind() == "variable_declaration")?
                .named_child(0)?;
            declarator
                .child_by_field_name("name")
                .map(|node| node_text(&node, source).trim().to_owned())
        }
        "expression_statement" => {
            let mut cursor = statement.walk();
            let assignment = statement
                .named_children(&mut cursor)
                .find(|child| child.kind() == "assignment_expression")?;
            assignment
                .child_by_field_name("left")
                .map(|node| normalize_line(&node_text(&node, source)))
        }
        _ => None,
    }
}

/// Display type of an assigned name: declared annotation, or inferred from a parameter.
fn declared_type(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
    assigned_name: Option<&str>,
) -> Option<String> {
    let name = assigned_name?;
    match statement.kind() {
        "local_declaration_statement" => {
            let mut cursor = statement.walk();
            let declaration = statement
                .named_children(&mut cursor)
                .find(|child| child.kind() == "variable_declaration")?;
            let type_ = declaration
                .child_by_field_name("type")
                .map(|node| node_text(&node, source).trim().to_owned())
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "var".to_owned());
            if type_ != "var" {
                return Some(format!("{name}: {type_}"));
            }
            let declarator = declaration.named_child(0)?;
            let value = value_after_equals(declarator)?;
            let inferred = infer_type(value, source, found);
            Some(format!("{name}: {inferred}"))
        }
        "expression_statement" => Some(name.to_owned()),
        _ => None,
    }
}

/// Infer a display type from a literal or a parameter's declared type.
fn infer_type(value: Node<'_>, source: &str, found: &DiscoveredFn) -> String {
    match value.kind() {
        "string_literal" | "verbatim_string_literal" | "raw_string_literal" => "string".to_owned(),
        "integer_literal" => "int".to_owned(),
        "real_literal" => "double".to_owned(),
        "boolean_literal" => "bool".to_owned(),
        "null_literal" => "null".to_owned(),
        "identifier" => parameter_type(&node_text(&value, source), &found.parameters)
            .unwrap_or_else(|| "unknown".to_owned()),
        _ => "unknown".to_owned(),
    }
}

/// Look up a declared parameter type for identifier-to-identifier copies.
fn parameter_type(name: &str, parameters: &[Parameter]) -> Option<String> {
    parameters
        .iter()
        .find(|parameter| parameter.name == name && !parameter.type_.trim().is_empty())
        .map(|parameter| parameter.type_.clone())
}

/// Condition of an `if`, subject of a `switch`, or the literal `try`.
fn condition_text(statement: Node<'_>, source: &str) -> String {
    match statement.kind() {
        "if_statement" | "switch_statement" => {
            let field = if statement.kind() == "if_statement" {
                "condition"
            } else {
                "value"
            };
            statement
                .child_by_field_name(field)
                .map(|node| normalize_line(&node_text(&node, source)))
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "condition".to_owned())
        }
        "try_statement" => "try".to_owned(),
        _ => "condition".to_owned(),
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

/// Whether a call or object construction occurs below a node, excluding nested callables.
fn has_call(node: Node<'_>) -> bool {
    if matches!(
        node.kind(),
        "invocation_expression" | "object_creation_expression"
    ) {
        return true;
    }
    let mut cursor = node.walk();
    let found = node
        .named_children(&mut cursor)
        .any(|child| !OPAQUE_KINDS.contains(&child.kind()) && has_call(child));
    found
}

/// Whether a variable declarator's initializer contains a call.
fn initializer_has_call(statement: Node<'_>) -> bool {
    let mut cursor = statement.walk();
    let declaration = match statement
        .named_children(&mut cursor)
        .find(|child| child.kind() == "variable_declaration")
    {
        Some(node) => node,
        None => return false,
    };
    let mut declarators = declaration.walk();
    for declarator in declaration.named_children(&mut declarators) {
        if declarator.kind() != "variable_declarator" {
            continue;
        }
        let initializer = value_after_equals(declarator);
        if let Some(value) = initializer {
            if has_call(value) {
                return true;
            }
        }
    }
    false
}

/// All calls below a node as `(start byte, bare callee name)` pairs in source order.
fn collect_calls(node: Node<'_>, source: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    visit_calls(node, source, &mut found);
    found.sort_by_key(|(byte, _)| *byte);
    found
}

/// Recursive half of [`collect_calls`]; nested callables belong to another body.
fn visit_calls(node: Node<'_>, source: &str, out: &mut Vec<(usize, String)>) {
    if matches!(
        node.kind(),
        "invocation_expression" | "object_creation_expression"
    ) {
        if let Some(callee) = callee_bare_name(node, source) {
            out.push((node.start_byte(), callee));
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if OPAQUE_KINDS.contains(&child.kind()) {
            continue;
        }
        visit_calls(child, source, out);
    }
}

/// Bare callee of a call: the last identifier of `A.B.C(...)`, or a constructed type.
fn callee_bare_name(node: Node<'_>, source: &str) -> Option<String> {
    if node.kind() == "object_creation_expression" {
        return node
            .child_by_field_name("type")
            .map(|node| bare_name(&node_text(&node, source)))
            .filter(|name| !name.is_empty());
    }
    let target = node
        .child_by_field_name("function")
        .or_else(|| node.named_child(0))?;
    let mut identifiers = Vec::new();
    visit_identifiers(target, source, &mut identifiers);
    identifiers
        .pop()
        .map(|name| bare_name(&name))
        .filter(|name| !name.is_empty())
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

/// Reduce a possibly qualified/generic type or member text to its bare last name.
fn bare_name(text: &str) -> String {
    let mut name = text.trim();
    if let Some(index) = name.find('<') {
        name = &name[..index];
    }
    name.rsplit('.').next().unwrap_or(name).trim().to_owned()
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

/// Recursive half of [`collect_inputs`]; literals, callees and locals are skipped.
fn visit_inputs(node: Node<'_>, source: &str, out: &mut Vec<(usize, String)>) {
    match node.kind() {
        "comment"
        | "string_literal"
        | "verbatim_string_literal"
        | "raw_string_literal"
        | "integer_literal"
        | "real_literal"
        | "boolean_literal"
        | "null_literal"
        | "character_literal"
        | "interpolated_string_expression" => return,
        "identifier" => {
            let text = node_text(&node, source);
            if !NON_INPUT_WORDS.contains(&text.as_str()) {
                out.push((node.start_byte(), text));
            }
            return;
        }
        "member_access_expression" | "qualified_name" => {
            let path = normalize_line(&node_text(&node, source)).replace(' ', "");
            if !path.is_empty() {
                out.push((node.start_byte(), path));
            }
            return;
        }
        "invocation_expression" => {
            if let Some(arguments) = node.child_by_field_name("arguments") {
                visit_inputs(arguments, source, out);
            }
            return;
        }
        "object_creation_expression" => {
            if let Some(arguments) = node.child_by_field_name("arguments") {
                visit_inputs(arguments, source, out);
            }
            if let Some(initializer) = node.child_by_field_name("initializer") {
                visit_inputs(initializer, source, out);
            }
            return;
        }
        _ if OPAQUE_KINDS.contains(&node.kind()) => return,
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

/// Top two `//` or `///` lines directly above a statement, XML tags removed.
fn preceding_comments(source: &str, statement_row: usize) -> (Option<String>, Option<String>) {
    let lines: Vec<&str> = source.lines().collect();
    let mut block = Vec::new();
    let mut row = statement_row;
    while row > 0 {
        row -= 1;
        let line = lines.get(row).copied().unwrap_or("").trim();
        let text = line
            .strip_prefix("///")
            .or_else(|| line.strip_prefix("//"))
            .map(strip_xml_tags)
            .unwrap_or_default();
        if text.is_empty() {
            break;
        }
        block.push(text);
    }
    block.reverse();
    (block.first().cloned(), block.get(1).cloned())
}

/// Remove `<tag>` sequences so XML doc comments read as plain sentences.
fn strip_xml_tags(text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for ch in text.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// One `throw` statement reduced to the facts needed by `ErrorSpec` and branches.
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
    if node.kind() == "throw_statement" {
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
        if OPAQUE_KINDS.contains(&child.kind()) {
            continue;
        }
        visit_throws(child, source, out);
    }
}

/// Code carried by `throw new X("CODE")` or `throw new FunctionError("CODE")`.
fn throw_code(node: Node<'_>, source: &str) -> Option<String> {
    if let Some(code) = first_string(node, source) {
        return Some(code);
    }
    let mut cursor = node.walk();
    let constructed = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "object_creation_expression");
    if let Some(constructed) = constructed {
        let name = constructed
            .child_by_field_name("type")
            .map(|node| bare_name(&node_text(&node, source)))
            .unwrap_or_default();
        if !name.is_empty() {
            return Some(name);
        }
    }
    let text = normalize_line(&node_text(&node, source));
    (!text.is_empty()).then_some(text)
}

/// First string literal inside a subtree, using source order for determinism.
fn first_string(node: Node<'_>, source: &str) -> Option<String> {
    if matches!(
        node.kind(),
        "string_literal" | "verbatim_string_literal" | "raw_string_literal"
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

/// Nearest enclosing `if`/`switch` condition, or `explicit throw`.
fn enclosing_condition(throw: Node<'_>, source: &str) -> String {
    let mut current = throw.parent();
    while let Some(node) = current {
        let field = match node.kind() {
            "if_statement" => Some("condition"),
            "switch_statement" => Some("value"),
            _ => None,
        };
        if let Some(field) = field {
            if let Some(condition) = node.child_by_field_name(field) {
                let text = normalize_line(&node_text(&condition, source));
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

/// Derive C# purity, permissions, timeout and runtime from a callable's source.
fn classify_effects(slice: &str) -> (Purity, Vec<String>, u64, String) {
    let lower = slice.to_ascii_lowercase();
    let console_out = lower.contains("console.write");
    let console_in = lower.contains("console.read");
    let network = lower.contains("httpclient")
        || lower.contains("webrequest")
        || lower.contains("webclient")
        || lower.contains("httpwebrequest");
    let filesystem = lower.contains("system.io")
        || lower.contains("file.")
        || lower.contains("directory.")
        || lower.contains("path.");
    let fs_write = lower.contains("file.write")
        || lower.contains("file.create")
        || lower.contains("file.delete")
        || lower.contains("file.copy")
        || lower.contains("file.move")
        || lower.contains("file.append");
    let env_keys = extract_env_keys(slice);

    let mut permissions = BTreeSet::new();
    if console_out {
        permissions.insert("console:out".to_owned());
    }
    if console_in {
        permissions.insert("console:in".to_owned());
    }
    if filesystem {
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

    let side_effect = console_out || console_in || network || filesystem || !env_keys.is_empty();
    let purity = if side_effect {
        Purity::SideEffect
    } else {
        Purity::Pure
    };
    // Budget and runtime labels match the other adapters' contract values.
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

/// Environment keys read through `Environment.GetEnvironmentVariable("KEY")`.
fn extract_env_keys(slice: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let markers = [
        "GetEnvironmentVariable(\"",
        "GetEnvironmentVariable('",
        "getenvironmentvariable(\"",
        "getenvironmentvariable('",
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

/// Remove one layer of C# string quotes, including verbatim and raw strings.
fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix('@') {
        return unquote(rest);
    }
    if trimmed.len() >= 6 && trimmed.starts_with("\"\"\"") && trimmed.ends_with("\"\"\"") {
        return trimmed[3..trimmed.len() - 3].to_owned();
    }
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    trimmed.to_owned()
}
