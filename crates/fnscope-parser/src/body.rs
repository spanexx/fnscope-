//! Deterministic, AST-backed analysis of one function body.
//!
//! The pass reads only the source slice of a discovered function: it orders the
//! top-level statements, records calls/guards/transforms/returns, extracts thrown
//! error codes, and classifies effects. No randomness, clock or map iteration
//! enters the result, so the same input always produces the same `BodyAnalysis`.

use std::collections::BTreeSet;

use fnscope_model::{ErrorSpec, Parameter, Purity, Step, StepKind};
use tree_sitter::{Language, Node, Parser};

use crate::types::{BodyAnalysis, DiscoveredFn, Lang};

/// Grammar keywords that must never be reported as referenced identifiers.
const NON_INPUT_WORDS: &[&str] = &[
    "const", "let", "var", "return", "await", "new", "if", "else", "throw", "fn", "mut", "pub",
    "impl", "self", "Self", "match", "use", "as", "in", "ref", "move", "async", "static", "struct",
    "enum", "trait", "where", "break", "continue", "crate", "super", "Err", "Ok",
];

/// Arithmetic operators whose operands are treated as producing a number.
const NUMBER_OPERATORS: &[&str] = &["*", "/", "%", "-", "+", "**"];

/// Analyses one function body and returns deterministic execution facts.
///
/// The function node is located by its exact source slice, with a line/name
/// fallback. When no node matches, the neutral `BodyAnalysis::default()` is
/// returned so a partially discovered file cannot corrupt a manifest.
pub fn analyze(
    found: &DiscoveredFn,
    lang: Lang,
    source: &str,
    known_names: &[String],
) -> BodyAnalysis {
    let language: Language = match lang {
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Lang::JavaScript | Lang::Jsx => tree_sitter_javascript::LANGUAGE.into(),
        Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
    };

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

    // Effects are classified from the whole function text, not just the body,
    // so parameter defaults and inline type guards are also considered.
    let (purity, permissions, timeout_ms, runtime) = if lang == Lang::Rust {
        classify_rust_effects(&slice)
    } else {
        classify_effects(&slice)
    };

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
        Some(node) if matches!(node.kind(), "statement_block" | "block") => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .filter(|child| {
                    !matches!(
                        child.kind(),
                        "comment"
                            | "line_comment"
                            | "block_comment"
                            | "empty_statement"
                            | "attribute_item"
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
            lang,
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
        "function_declaration"
            | "function_expression"
            | "function"
            | "generator_function"
            | "generator_function_declaration"
            | "arrow_function"
            | "method_definition"
            | "function_signature"
            | "function_item"
            | "function_signature_item"
    )
}

/// Finds the smallest function node matching the recorded slice, or the node on
/// the recorded line whose name agrees with the discovery record.
fn find_function_node<'tree>(
    root: Node<'tree>,
    source: &str,
    found: &DiscoveredFn,
) -> Option<Node<'tree>> {
    let mut best: Option<Node<'tree>> = None;
    visit_function_nodes(root, source, found, &mut best);
    best
}

/// Depth-first walk that keeps the tightest matching function node.
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

/// Reads a function name from its own `name` field or its binding parent.
fn function_name(node: Node<'_>, source: &str) -> Option<String> {
    if let Some(name) = node.child_by_field_name("name") {
        let text = node_text(name, source);
        if !text.is_empty() {
            return Some(text);
        }
    }
    let parent = node.parent()?;
    let field = match parent.kind() {
        "variable_declarator" => "name",
        "assignment_expression" => "left",
        "pair" => "key",
        _ => return None,
    };
    parent
        .child_by_field_name(field)
        .map(|name| node_text(name, source))
}

/// Returns the statement block (or the expression body of a compact arrow).
fn function_body(node: Node<'_>) -> Option<Node<'_>> {
    if let Some(body) = node.child_by_field_name("body") {
        return Some(body);
    }
    // A few grammars expose the block as a named child without a field name.
    let mut cursor = node.walk();
    let found = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "statement_block");
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

/// Look through Rust `expression_statement` wrappers to the expression they contain.
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

/// Builds one ordered step plus the raw callee names encountered in it.
fn build_step(
    statement: Node<'_>,
    lang: Lang,
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
    // In a `add(...).abs()` chain the last callee labels the step, but the resolved
    // known function is the useful name; TS/JS titles stay exactly as they were.
    if lang == Lang::Rust {
        if let Some(resolved) = calls_fn_id.as_deref() {
            facts.callee = Some(resolved.to_owned());
        }
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

/// Collects the deterministic fields of a statement.
fn statement_facts(
    statement: Node<'_>,
    source: &str,
    found: &DiscoveredFn,
    error_sites: &[ThrowSite],
) -> StatementFacts {
    let code = normalize_line(&node_text(statement, source));
    let core = statement_expression(statement);
    let kind = match core.kind() {
        "return_statement" | "return_expression" => StepKind::Return,
        "if_statement" | "switch_statement" | "if_expression" | "match_expression" => {
            StepKind::Branch
        }
        _ => {
            if has_call(statement) {
                StepKind::Call
            } else {
                StepKind::Transform
            }
        }
    };

    let declarator = first_declarator(statement);
    let assigned_name = declarator
        .and_then(|node| node.child_by_field_name("name"))
        .map(|name| normalize_line(&node_text(name, source)))
        .filter(|name| !name.is_empty());
    let declared_type = declarator.and_then(|node| declared_type(node, source, found));

    let condition_node = core
        .child_by_field_name("condition")
        .or_else(|| core.child_by_field_name("value"));
    let condition = condition_node
        .map(|node| normalize_line(&node_text(node, source)))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "condition".to_owned());

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

/// Shortens a condition for human-readable labels without cutting mid-code badly.
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

/// Whether any call expression occurs below this node (excluding nested functions).
fn has_call(node: Node<'_>) -> bool {
    if node.kind() == "call_expression" {
        return true;
    }
    let mut cursor = node.walk();
    let found = node.named_children(&mut cursor).any(|child| {
        !matches!(
            child.kind(),
            "arrow_function"
                | "function_expression"
                | "method_definition"
                | "function_item"
                | "function_signature_item"
                | "closure_expression"
        ) && has_call(child)
    });
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
        // Nested function bodies belong to another function, not this step.
        if matches!(
            child.kind(),
            "arrow_function"
                | "function_expression"
                | "method_definition"
                | "function_item"
                | "function_signature_item"
                | "closure_expression"
        ) {
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

/// Collects identifier texts inside a subtree, in source order.
fn visit_identifiers(node: Node<'_>, source: &str, out: &mut Vec<String>) {
    if matches!(
        node.kind(),
        "identifier" | "property_identifier" | "field_identifier" | "self"
    ) {
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

/// Recursive half of [`collect_inputs`]; declarations and object keys are skipped.
fn visit_inputs(node: Node<'_>, source: &str, out: &mut Vec<(usize, String)>) {
    match node.kind() {
        "comment"
        | "line_comment"
        | "block_comment"
        | "type_annotation"
        | "type_identifier"
        | "primitive_type"
        | "generic_type"
        | "reference_type"
        | "scoped_type_identifier"
        | "lifetime"
        | "type_arguments"
        | "type_parameters" => return,
        "variable_declarator" | "let_declaration" => {
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
        "pair" => {
            if let Some(value) = node.child_by_field_name("value") {
                visit_inputs(value, source, out);
            }
            return;
        }
        "macro_invocation" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                // The macro name is a call target, not a referenced value.
                if matches!(child.kind(), "identifier" | "scoped_identifier") {
                    continue;
                }
                visit_inputs(child, source, out);
            }
            return;
        }
        "member_expression" | "subscript_expression" | "field_expression" => {
            if member_root(node).is_some() {
                let path = normalize_line(&node_text(node, source)).replace(' ', "");
                if !path.is_empty() {
                    out.push((node.start_byte(), path));
                }
                return;
            }
        }
        "scoped_identifier" => {
            let path = normalize_line(&node_text(node, source)).replace(' ', "");
            if !path.is_empty() {
                out.push((node.start_byte(), path));
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

/// Leftmost identifier of a member/subscript chain, if there is one.
fn member_root(node: Node<'_>) -> Option<Node<'_>> {
    let mut current = node
        .child_by_field_name("object")
        .or_else(|| node.child_by_field_name("value"))?;
    loop {
        match current.kind() {
            "identifier" | "self" | "super" | "crate" => return Some(current),
            "member_expression"
            | "subscript_expression"
            | "parenthesized_expression"
            | "field_expression" => {
                current = current
                    .child_by_field_name("object")
                    .or_else(|| current.child_by_field_name("value"))
                    .or_else(|| current.named_child(0))?;
            }
            _ => return None,
        }
    }
}

// ---------------------------------------------------------------------------
// Declarations and types
// ---------------------------------------------------------------------------

/// First declarator of a `const`/`let`/`var` statement, if any.
fn first_declarator(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "let_declaration" {
        return Some(node); // Rust stores the pattern and type directly on the statement
    }
    if !matches!(node.kind(), "variable_declaration" | "lexical_declaration") {
        return None;
    }
    let mut cursor = node.walk();
    let found = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "variable_declarator");
    found
}

/// Annotated type when present, otherwise a conservative inference from the value.
fn declared_type(declarator: Node<'_>, source: &str, found: &DiscoveredFn) -> Option<String> {
    let name = declarator
        .child_by_field_name("name")
        .or_else(|| declarator.child_by_field_name("pattern"))
        .map(|node| normalize_line(&node_text(node, source)))
        .unwrap_or_default();
    if name.is_empty() {
        return None;
    }

    let annotated = declarator
        .child_by_field_name("type")
        .map(|node| {
            normalize_line(&node_text(node, source))
                .trim_start_matches(':')
                .trim()
                .to_owned()
        })
        .filter(|text| !text.is_empty());
    if let Some(annotation) = annotated {
        return Some(format!("{}: {}", name, annotation));
    }

    let inferred = declarator
        .child_by_field_name("value")
        .map(|value| infer_type(value, source, found))
        .unwrap_or_else(|| "unknown".to_owned());
    Some(format!("{}: {}", name, inferred))
}

/// Infers a display type from a literal, arithmetic expression or parameter type.
fn infer_type(value: Node<'_>, source: &str, found: &DiscoveredFn) -> String {
    match value.kind() {
        "string" | "template_string" | "string_literal" => "string".to_owned(),
        "number" | "integer_literal" => "number".to_owned(),
        "float_literal" => "f64".to_owned(),
        "char_literal" => "char".to_owned(),
        "true" | "false" => "boolean".to_owned(),
        "null" => "null".to_owned(),
        "regex" => "RegExp".to_owned(),
        "array" => "unknown[]".to_owned(),
        "object" | "object_pattern" => "object".to_owned(),
        "arrow_function" | "function_expression" | "function_declaration" => "function".to_owned(),
        "binary_expression" => {
            let operator = value
                .child_by_field_name("operator")
                .map(|node| node_text(node, source))
                .unwrap_or_default();
            if NUMBER_OPERATORS.contains(&operator.as_str()) {
                "number".to_owned()
            } else if matches!(
                operator.as_str(),
                "==" | "===" | "!=" | "!==" | "<" | ">" | "<=" | ">="
            ) {
                "boolean".to_owned()
            } else {
                "unknown".to_owned()
            }
        }
        "unary_expression" => {
            let operator = node_text(value, source);
            if operator.trim_start().starts_with('!') {
                "boolean".to_owned()
            } else {
                "number".to_owned()
            }
        }
        "new_expression" => "object".to_owned(),
        "identifier" => parameter_type(&node_text(value, source), &found.parameters)
            .unwrap_or_else(|| "unknown".to_owned()),
        _ => "unknown".to_owned(),
    }
}

/// Looks up a declared parameter type for simple identifier-to-identifier copies.
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

/// Removes a leading enumerator such as `1.` or `2)` from a comment line.
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

/// One throw statement reduced to the facts needed by `ErrorSpec` and branches.
struct ThrowSite {
    byte: usize,
    code: String,
    when: String,
}

/// Walks every descendant of the body and records throw statements in source order.
fn collect_throw_sites(body: Node<'_>, source: &str) -> Vec<ThrowSite> {
    let mut sites = Vec::new();
    visit_throws(body, source, &mut sites);
    sites.sort_by_key(|site| site.byte);
    sites
}

/// Recursive half of [`collect_throw_sites`].
fn visit_throws(node: Node<'_>, source: &str, out: &mut Vec<ThrowSite>) {
    if node.kind() == "throw_statement" {
        let code = throw_code(node, source);
        let when = enclosing_condition(node, source);
        out.push(ThrowSite {
            byte: node.start_byte(),
            code,
            when,
        });
    }
    if node.kind() == "return_expression" {
        if let Some(code) = rust_err_code(node, source) {
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

/// Extracts the error code from `new FunctionError('CODE')`, `new Error('CODE')`
/// or a bare string literal, falling back to `THROW`.
fn throw_code(throw: Node<'_>, source: &str) -> String {
    let argument = throw.named_child(0);
    match argument {
        Some(node) if node.kind() == "string" || node.kind() == "template_string" => {
            unquote(&node_text(node, source))
        }
        Some(node) => first_string(node, source).unwrap_or_else(|| "THROW".to_owned()),
        None => "THROW".to_owned(),
    }
}

/// Error code carried by a Rust `return Err(...)` expression, if present.
fn rust_err_code(node: Node<'_>, source: &str) -> Option<String> {
    let mut call = None;
    visit_err_calls(node, source, &mut call);
    let call = call?;
    let arguments = call.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let first = arguments.named_children(&mut cursor).next()?;
    if first.kind() == "string_literal" {
        return Some(unquote(&node_text(first, source)));
    }
    if first.kind() == "struct_expression" {
        if let Some(code) = rust_struct_code_field(first, source) {
            return Some(code);
        }
    }
    if let Some(name) = rust_variant_name(first, source) {
        return Some(name);
    }
    first_string(first, source)
}

/// Code from the `code` field of `Err(TypeName { code: "CODE" })`.
fn rust_struct_code_field(node: Node<'_>, source: &str) -> Option<String> {
    let body = node.child_by_field_name("body")?;
    let mut cursor = body.walk();
    for field in body.named_children(&mut cursor) {
        if field.kind() != "field_initializer" {
            continue;
        }
        let name = field.child_by_field_name("field")?;
        if node_text(name, source).trim() != "code" {
            continue;
        }
        let value = field.child_by_field_name("value")?;
        if matches!(
            value.kind(),
            "string_literal" | "string" | "template_string"
        ) {
            return Some(unquote(&node_text(value, source)));
        }
        if matches!(value.kind(), "identifier" | "scoped_identifier") {
            let text = node_text(value, source);
            let bare = text.rsplit("::").next().unwrap_or(text.as_str()).trim();
            return rust_const_string(source, bare).or_else(|| Some(bare.to_owned()));
        }
    }
    None
}

/// Literal value of `const NAME: T = "VALUE";`, when the source declares it.
fn rust_const_string(source: &str, name: &str) -> Option<String> {
    let needle = format!("const {}", name);
    let mut offset = 0usize;
    while let Some(index) = source[offset..].find(needle.as_str()) {
        let start = offset + index + needle.len();
        let rest = &source[start..];
        if rest
            .chars()
            .next()
            .map(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            .unwrap_or(false)
        {
            offset = start;
            continue;
        }
        if let Some(equals) = rest.find('=') {
            let value = &rest[equals + 1..];
            if let Some(open) = value.find('"') {
                let after = &value[open + 1..];
                if let Some(close) = after.find('"') {
                    return Some(after[..close].to_owned());
                }
            }
        }
        offset = start;
    }
    None
}

/// Recursive half of [`rust_err_code`]; finds the first `Err(...)` call in source order.
fn visit_err_calls<'tree>(node: Node<'tree>, source: &str, out: &mut Option<Node<'tree>>) {
    if out.is_some() {
        return;
    }
    if node.kind() == "call_expression" && callee_bare_name(node, source).as_deref() == Some("Err")
    {
        *out = Some(node);
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_err_calls(child, source, out);
        if out.is_some() {
            return;
        }
    }
}

/// Last segment of a Rust variant path such as `AppError::NotFound`.
fn rust_variant_name(node: Node<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" | "type_identifier" => {
            let text = node_text(node, source);
            (!text.is_empty()).then_some(text)
        }
        "scoped_identifier" | "scoped_type_identifier" => {
            let text = node_text(node, source);
            let last = text.rsplit("::").next().unwrap_or("").trim().to_string();
            (!last.is_empty()).then_some(last)
        }
        "call_expression" => node
            .child_by_field_name("function")
            .and_then(|function| rust_variant_name(function, source)),
        _ => None,
    }
}

/// First string literal inside a subtree, using source order for determinism.
fn first_string(node: Node<'_>, source: &str) -> Option<String> {
    if node.kind() == "string" || node.kind() == "template_string" {
        return Some(unquote(&node_text(node, source)));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(code) = first_string(child, source) {
            return Some(code);
        }
    }
    None
}

/// Removes surrounding quotes from a literal's source text.
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

/// Nearest enclosing `if` condition, or `explicit throw` for an unguarded throw.
fn enclosing_condition(throw: Node<'_>, source: &str) -> String {
    let mut current = throw.parent();
    while let Some(node) = current {
        if matches!(node.kind(), "if_statement" | "if_expression") {
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

/// Deduplicates throw sites by code, keeping the first occurrence and its status.
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
        "EMPTY_SENTENCE" | "INSECURE_PROTOCOL" | "BAD_REQUEST" => Some(400),
        "NOT_FOUND" => Some(404),
        "UNAUTHORIZED" => Some(401),
        "FORBIDDEN" => Some(403),
        _ => Some(500),
    }
}

// ---------------------------------------------------------------------------
// Purity, permissions and budgets
// ---------------------------------------------------------------------------

/// Derives purity, permissions, timeout and runtime from a function's source text.
fn classify_effects(slice: &str) -> (Purity, Vec<String>, u64, String) {
    let lower = slice.to_ascii_lowercase();
    let network = lower.contains("fetch(")
        || lower.contains("axios.")
        || lower.contains("http.")
        || lower.contains("https.request")
        || lower.contains("node:http");
    let fs_read = lower.contains("node:fs") || lower.contains("fs.") || lower.contains("readfile");
    let fs_write =
        lower.contains("writefile") || lower.contains("appendfile") || lower.contains("fs.write");
    let database = lower.contains("database") || lower.contains("query(");
    let logging = lower.contains("console.log");
    let env_keys = extract_env_keys(slice);

    let mut permissions = BTreeSet::new();
    if network {
        permissions.insert("net:outbound".to_owned());
    }
    if fs_read {
        permissions.insert("fs:read".to_owned());
    }
    if fs_write {
        permissions.insert("fs:write".to_owned());
    }
    if database {
        permissions.insert("db:query".to_owned());
    }
    for key in &env_keys {
        permissions.insert(format!("secrets:read:{}", key));
    }

    let side_effect = network || fs_read || fs_write || database || logging || !env_keys.is_empty();
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

/// Environment variable names read through `process.env.<KEY>`.
fn extract_env_keys(slice: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let bytes = slice.as_bytes();
    let marker = b"process.env.";
    let mut index = 0;
    while index + marker.len() <= bytes.len() {
        if &bytes[index..index + marker.len()] == marker {
            let start = index + marker.len();
            let mut end = start;
            if end < bytes.len() && (bytes[end].is_ascii_alphabetic() || bytes[end] == b'_') {
                end += 1;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
                {
                    end += 1;
                }
                let key = slice[start..end].to_owned();
                if !keys.contains(&key) {
                    keys.push(key);
                }
                index = end;
                continue;
            }
        }
        index += 1;
    }
    keys
}

/// Rust-specific purity, permissions and budgets; JS/TS classification is untouched.
fn classify_rust_effects(slice: &str) -> (Purity, Vec<String>, u64, String) {
    let lower = slice.to_ascii_lowercase();
    let logging = lower.contains("println!") || lower.contains("eprintln!");
    let network = lower.contains("reqwest")
        || lower.contains("std::net")
        || lower.contains("tcpstream")
        || lower.contains("tcplistener")
        || lower.contains("udpsocket");
    let fs_read = lower.contains("std::fs::read")
        || lower.contains("fs::read")
        || lower.contains("file::open")
        || lower.contains("read_to_string");
    let fs_write = lower.contains("std::fs::write")
        || lower.contains("fs::write")
        || lower.contains("std::fs::create")
        || lower.contains("std::fs::remove")
        || lower.contains("file::create");
    let env_keys = extract_rust_env_keys(slice);

    let mut permissions = BTreeSet::new();
    if logging {
        permissions.insert("console:out".to_owned());
    }
    if network {
        permissions.insert("net:outbound".to_owned());
    }
    if fs_read {
        permissions.insert("fs:read".to_owned());
    }
    if fs_write {
        permissions.insert("fs:write".to_owned());
    }
    for key in &env_keys {
        permissions.insert(format!("secrets:read:{}", key));
    }

    let side_effect = logging || network || fs_read || fs_write || !env_keys.is_empty();
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

/// Environment variable names read through `std::env::var("KEY")`.
fn extract_rust_env_keys(slice: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for marker in ["env::var(\"", "env::var_os(\""] {
        let mut offset = 0usize;
        while let Some(index) = slice[offset..].find(marker) {
            let start = offset + index + marker.len();
            match slice[start..].find('"') {
                Some(end) => {
                    let key = slice[start..start + end].to_owned();
                    if !key.is_empty() && !keys.contains(&key) {
                        keys.push(key);
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
fn node_text(node: Node<'_>, source: &str) -> String {
    node.utf8_text(source.as_bytes()).unwrap_or("").to_owned()
}

/// Collapses every whitespace run into a single space and trims the result.
fn normalize_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FnKind;
    use fnscope_model::ReturnShape;

    /// Builds a discovery record whose source slice is the whole test snippet.
    fn discovered(name: &str, source: &str, returns: &str) -> DiscoveredFn {
        DiscoveredFn {
            name: name.to_owned(),
            qualified_name: name.to_owned(),
            kind: FnKind::Declaration,
            line: 1,
            end_line: source.lines().count(),
            source_code: source.to_owned(),
            parameters: Vec::new(),
            returns: ReturnShape {
                type_: returns.to_owned(),
                fields: Vec::new(),
            },
            doc_description: None,
            tags: Vec::new(),
            is_async: false,
            class_name: None,
        }
    }

    #[test]
    fn call_step_resolves_known_callee_and_inputs() {
        let source = "function evaluateGrammarRole(input: Input): Token[] {\n  const tokens: Token[] = tokenize(input.sentence);\n  return tokens;\n}";
        let found = discovered("evaluateGrammarRole", source, "Token[]");
        let known = vec!["tokenize".to_owned()];

        let analysis = analyze(&found, Lang::TypeScript, source, &known);
        let step = &analysis.steps[0];
        assert_eq!(step.id, "step-1");
        assert_eq!(step.kind, StepKind::Call);
        assert_eq!(step.calls_fn_id.as_deref(), Some("tokenize"));
        assert!(step.inputs.iter().any(|name| name == "input.sentence"));
        assert_eq!(step.output, "tokens: Token[]");
        assert_eq!(step.title, "Call tokenize()");
        assert!(!step.code.contains('\n'));
    }

    #[test]
    fn guard_step_reports_thrown_code_and_status() {
        let source = "function check(sentence: string): void {\n  const match = /^[a-z]+$/.test(sentence);\n  if (!match) {\n    throw new FunctionError('TOKEN_NOT_FOUND');\n  }\n  return;\n}";
        let found = discovered("check", source, "void");

        let analysis = analyze(&found, Lang::TypeScript, source, &[]);
        let guard = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Branch)
            .expect("guard step");
        assert!(guard.output.contains("throws TOKEN_NOT_FOUND"));
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "TOKEN_NOT_FOUND");
        assert_eq!(analysis.errors[0].status, Some(422));
        assert!(analysis.errors[0].when.contains("!match"));
    }

    #[test]
    fn return_step_uses_declared_return_type() {
        let source = "function role(): Role {\n  return { roleId, label };\n}";
        let found = discovered("role", source, "Role");

        let analysis = analyze(&found, Lang::TypeScript, source, &[]);
        let step = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Return)
            .expect("return step");
        assert_eq!(step.output, found.returns.type_);
    }

    #[test]
    fn annotated_comments_become_title_and_detail() {
        let source = "function evaluate(input: Input): Token[] {\n  // 1. Tokenize sentence\n  // Splits into tokens\n  const tokens: Token[] = tokenize(input.sentence);\n  return tokens;\n}";
        let found = discovered("evaluate", source, "Token[]");

        let analysis = analyze(&found, Lang::TypeScript, source, &[]);
        assert_eq!(analysis.steps[0].title, "Tokenize sentence");
        assert_eq!(analysis.steps[0].detail, "Splits into tokens");
    }

    #[test]
    fn effects_yield_permissions_and_budgets() {
        let source = "async function load(url: string) {\n  const key = process.env.API_KEY;\n  const res = await fetch(url);\n  return res;\n}";
        let found = discovered("load", source, "Response");

        let analysis = analyze(&found, Lang::TypeScript, source, &[]);
        assert_eq!(analysis.purity, Purity::SideEffect);
        assert!(analysis.permissions.iter().any(|p| p == "net:outbound"));
        assert!(analysis
            .permissions
            .iter()
            .any(|p| p == "secrets:read:API_KEY"));
        assert_eq!(analysis.timeout_ms, 5000);
        assert_eq!(analysis.runtime, "node20");
    }

    #[test]
    fn arithmetic_declaration_is_a_pure_transform() {
        let source =
            "function double(n: number): number {\n  const doubled = n * 2;\n  return doubled;\n}";
        let found = discovered("double", source, "number");

        let analysis = analyze(&found, Lang::TypeScript, source, &[]);
        assert_eq!(analysis.steps[0].kind, StepKind::Transform);
        assert!(analysis.steps[0].inputs.iter().any(|name| name == "n"));
        assert_eq!(analysis.steps[0].output, "doubled: number");
        assert_eq!(analysis.purity, Purity::Pure);
        assert_eq!(analysis.timeout_ms, 150);
        assert_eq!(analysis.runtime, "universal");
    }

    #[test]
    fn repeated_analysis_is_identical() {
        let source = "function evaluate(input: Input): Token[] {\n  const tokens: Token[] = tokenize(input.sentence);\n  if (!tokens) {\n    throw new FunctionError('EMPTY_SENTENCE');\n  }\n  return tokens;\n}";
        let found = discovered("evaluate", source, "Token[]");
        let known = vec!["tokenize".to_owned()];

        let first = analyze(&found, Lang::TypeScript, source, &known);
        let second = analyze(&found, Lang::TypeScript, source, &known);
        assert_eq!(first, second);
    }

    #[test]
    fn rust_let_and_call_steps() {
        let source =
            "fn evaluate(input: i32) -> i32 {\n  let doubled: i32 = scale(input);\n  doubled\n}";
        let found = discovered("evaluate", source, "i32");
        let known = vec!["scale".to_owned()];

        let analysis = analyze(&found, Lang::Rust, source, &known);
        assert_eq!(analysis.steps.len(), 2);
        let call = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Call)
            .expect("call step");
        assert_eq!(call.id, "step-1");
        assert_eq!(call.calls_fn_id.as_deref(), Some("scale"));
        assert!(call.inputs.iter().any(|name| name == "input"));
        assert_eq!(call.output, "doubled: i32");
        assert_eq!(call.code, "let doubled: i32 = scale(input);");
        assert!(!call.code.contains('\n'));
    }

    #[test]
    fn rust_guard_with_return_err_reports_error() {
        let source = "fn load(id: i32) -> Result<i32, AppError> {\n  if id < 1 {\n    return Err(AppError::NotFound);\n  }\n  Ok(id)\n}";
        let found = discovered("load", source, "Result<i32, AppError>");

        let analysis = analyze(&found, Lang::Rust, source, &[]);
        let guard = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Branch)
            .expect("guard step");
        assert!(guard.output.contains("throws NotFound"));
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "NotFound");
        assert_eq!(analysis.errors[0].status, Some(500));
        assert!(analysis.errors[0].when.contains("id < 1"));
    }

    #[test]
    fn rust_println_is_side_effect_with_console_permission() {
        let source = "fn greet(name: &str) {\n  println!(\"hi {}\", name);\n}";
        let found = discovered("greet", source, "()");

        let analysis = analyze(&found, Lang::Rust, source, &[]);
        assert_eq!(analysis.purity, Purity::SideEffect);
        assert!(analysis.permissions.iter().any(|p| p == "console:out"));
        assert_eq!(analysis.timeout_ms, 1000);
        assert_eq!(analysis.runtime, "node20");
    }

    #[test]
    fn rust_method_call_resolves_by_last_segment() {
        let source = "impl Service { fn run(&self, x: i32) -> i32 { self.add(x) } }";
        let found = discovered("run", source, "i32");
        let known = vec!["add".to_owned()];

        let analysis = analyze(&found, Lang::Rust, source, &known);
        let call = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Call)
            .expect("call step");
        assert_eq!(call.calls_fn_id.as_deref(), Some("add"));
        assert!(call.inputs.iter().any(|name| name == "x"));
        assert_eq!(call.title, "Call add()");
    }

    #[test]
    fn rust_doc_comments_become_step_title_and_detail() {
        let source = "fn evaluate(x: i32) -> i32 {\n  /// 1. Double value\n  /// Keeps it pure\n  let doubled = x * 2;\n  doubled\n}";
        let found = discovered("evaluate", source, "i32");

        let analysis = analyze(&found, Lang::Rust, source, &[]);
        let transform = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Transform)
            .expect("transform step");
        assert_eq!(transform.title, "Double value");
        assert_eq!(transform.detail, "Keeps it pure");
        assert_eq!(transform.output, "doubled: number");
    }

    #[test]
    fn rust_repeated_analysis_is_identical() {
        let source = "fn load(id: i32) -> Result<i32, AppError> {\n  if id < 1 {\n    return Err(\"BAD_REQUEST\");\n  }\n  Ok(id)\n}";
        let found = discovered("load", source, "Result<i32, AppError>");

        let first = analyze(&found, Lang::Rust, source, &[]);
        let second = analyze(&found, Lang::Rust, source, &[]);
        assert_eq!(first, second);
        assert_eq!(first.errors[0].code, "BAD_REQUEST");
        assert_eq!(first.errors[0].status, Some(400));
    }

    #[test]
    fn rust_struct_literal_error_code_feeds_guard_output() {
        let source = r#"pub fn normalize(text: &str) -> Result<String, AppError> {
  if text.is_empty() {
    return Err(AppError { code: "EMPTY_TEXT" });
  }
  Ok(text.to_string())
}"#;
        let found = discovered("normalize", source, "Result<String, AppError>");

        let analysis = analyze(&found, Lang::Rust, source, &[]);
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "EMPTY_TEXT");
        let guard = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Branch)
            .expect("guard step");
        assert!(
            guard.output.contains("throws EMPTY_TEXT"),
            "unexpected guard output: {}",
            guard.output
        );
    }

    #[test]
    fn rust_const_error_code_from_struct_literal() {
        let source = "const EMPTY_TEXT: &str = \"EMPTY_TEXT\";\nfn check(text: &str) -> Result<(), AppError> {\n  if text.is_empty() {\n    return Err(AppError { code: EMPTY_TEXT });\n  }\n  Ok(())\n}";
        let fn_start = source.find("fn check").expect("function start");
        let found = DiscoveredFn {
            name: "check".to_owned(),
            qualified_name: "check".to_owned(),
            kind: FnKind::Declaration,
            line: 2,
            end_line: source.lines().count(),
            source_code: source[fn_start..].to_owned(),
            parameters: Vec::new(),
            returns: ReturnShape {
                type_: "Result<(), AppError>".to_owned(),
                fields: Vec::new(),
            },
            doc_description: None,
            tags: Vec::new(),
            is_async: false,
            class_name: None,
        };

        let analysis = analyze(&found, Lang::Rust, source, &[]);
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "EMPTY_TEXT");
    }

    #[test]
    fn rust_chained_call_title_uses_resolved_callee() {
        let source = "fn calculate(x: i32) -> i32 { add(x).abs() }";
        let found = discovered("calculate", source, "i32");
        let known = vec!["add".to_owned()];

        let analysis = analyze(&found, Lang::Rust, source, &known);
        let call = analysis
            .steps
            .iter()
            .find(|step| step.kind == StepKind::Call)
            .expect("call step");
        assert_eq!(call.calls_fn_id.as_deref(), Some("add"));
        assert_eq!(call.title, "Call add()");
    }
}
