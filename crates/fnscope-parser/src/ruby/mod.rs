//! Ruby language adapter.
//!
//! All Ruby grammar knowledge lives in this module and its two children. The
//! shared type-name resolver in `crate::resolve` is grammar-free string
//! handling, so it is reused here exactly as the Python adapter reuses it. This
//! job does not register the adapter globally; registration is a separate,
//! reviewed job.

mod body;
mod discover;

use std::collections::BTreeMap;

use fnscope_model::ReturnShape;
use tree_sitter::Node;

use crate::adapter::LanguageAdapter;
use crate::resolve::resolve_fields;
use crate::types::{BodyAnalysis, DiscoveredFn};

/// Stateless adapter for `.rb` sources, held by the registry without global state.
pub struct RubyAdapter;

impl RubyAdapter {
    /// Construct the stateless adapter; all grammar state is local to each call.
    pub fn new() -> Self {
        Self
    }
}

impl Default for RubyAdapter {
    fn default() -> Self {
        Self
    }
}

impl LanguageAdapter for RubyAdapter {
    fn id(&self) -> &'static str {
        "ruby"
    }

    fn extensions(&self) -> &'static [&'static str] {
        // `Registry::for_extension` trims a leading dot, so this is `rb`, not `.rb`.
        &["rb"]
    }

    fn discover(&self, source: &str, relative_path: &str) -> Vec<DiscoveredFn> {
        discover::discover(source, relative_path)
    }

    fn analyze(&self, found: &DiscoveredFn, source: &str, known_names: &[String]) -> BodyAnalysis {
        body::analyze(found, source, known_names)
    }

    fn collect_types(&self, source: &str) -> BTreeMap<String, ReturnShape> {
        discover::collect_types(source)
    }

    fn resolve_return_fields(
        &self,
        found: &mut DiscoveredFn,
        types: &BTreeMap<String, ReturnShape>,
    ) {
        resolve_fields(found, types);
    }
}

/// Exact source text of a node, degrading to an empty string for invalid UTF-8.
pub(crate) fn node_text(node: &Node<'_>, source: &str) -> String {
    node.utf8_text(source.as_bytes()).unwrap_or("").to_owned()
}

/// Collapse every whitespace run into one space and trim the result.
pub(crate) fn normalize_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Remove one layer of Ruby string quotes from a literal's source text.
pub(crate) fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'\'' | b'"') && bytes[bytes.len() - 1] == bytes[0] {
        return trimmed[1..trimmed.len() - 1].to_owned();
    }
    trimmed.to_owned()
}

/// Bare callee name of a Ruby `call` node, read from its `method` field.
pub(crate) fn callee_name(call: Node<'_>, source: &str) -> Option<String> {
    let method = call.child_by_field_name("method")?;
    let name = node_text(&method, source).trim().to_owned();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FnKind;
    use fnscope_model::{Purity, StepKind};

    /// A module-level method exposes its bare name, exact slice and 1-based lines.
    #[test]
    fn discovers_module_method() {
        let source = "def add(a, b)\n  a + b\nend\n";
        let found = RubyAdapter::new().discover(source, "pkg/add.rb");
        assert_eq!(found.len(), 1);
        let function = &found[0];
        assert_eq!(function.name, "add");
        assert_eq!(function.qualified_name, "add");
        assert_eq!(function.kind, FnKind::Declaration);
        assert_eq!(function.line, 1);
        assert_eq!(function.end_line, 3);
        assert!(function.source_code.starts_with("def add"));
        assert_eq!(function.class_name, None);
    }

    /// Class instance methods are qualified as `ClassName.method` with a class name.
    #[test]
    fn discovers_class_method_with_qualified_name() {
        let source = "class Greeter\n  def greet(name)\n    name\n  end\nend\n";
        let found = RubyAdapter::new().discover(source, "pkg/greeter.rb");
        assert_eq!(found.len(), 1);
        let method = &found[0];
        assert_eq!(method.name, "greet");
        assert_eq!(method.qualified_name, "Greeter.greet");
        assert_eq!(method.kind, FnKind::Method);
        assert_eq!(method.class_name.as_deref(), Some("Greeter"));
    }

    /// `attr_*` is not a function and a method nested inside a body is ignored.
    #[test]
    fn ignores_attributes_and_nested_methods() {
        let source = concat!(
            "module Util\n",
            "  attr_reader :name\n",
            "  def self.build(x)\n",
            "    def inner(y)\n",
            "      y\n",
            "    end\n",
            "    x\n",
            "  end\n",
            "end\n",
        );
        let found = RubyAdapter::new().discover(source, "pkg/util.rb");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "build");
        assert_eq!(found[0].qualified_name, "Util.build");
    }

    /// Optional, splat, keyword and block parameters carry YARD types and defaults.
    #[test]
    fn discovers_parameters_with_yard_types_and_defaults() {
        let source = concat!(
            "# Join parts.\n",
            "# @param [String] prefix leading text\n",
            "# @param [Integer] times count\n",
            "# @param [Array] rest extra values\n",
            "# @param [Hash] opts options\n",
            "# @param [Proc] blk callback\n",
            "# @return [String] joined text\n",
            "def join(prefix, times = 2, *rest, opts: {}, &blk)\n",
            "  prefix\n",
            "end\n",
        );
        let found = RubyAdapter::new().discover(source, "pkg/join.rb");
        assert_eq!(found.len(), 1);
        let function = &found[0];
        assert_eq!(function.doc_description.as_deref(), Some("Join parts."));
        assert_eq!(function.returns.type_, "String");
        assert_eq!(function.parameters.len(), 5);

        assert_eq!(function.parameters[0].name, "prefix");
        assert_eq!(function.parameters[0].type_, "String");
        assert!(function.parameters[0].required);
        assert_eq!(function.parameters[0].description, "leading text");

        assert_eq!(function.parameters[1].name, "times");
        assert_eq!(function.parameters[1].type_, "Integer");
        assert!(!function.parameters[1].required);
        assert_eq!(
            function.parameters[1].default_value,
            Some(serde_json::Value::String("2".to_owned()))
        );

        assert_eq!(function.parameters[2].name, "rest");
        assert_eq!(function.parameters[2].type_, "Array");
        assert!(!function.parameters[2].required);

        assert_eq!(function.parameters[3].name, "opts");
        assert_eq!(function.parameters[3].type_, "Hash");
        assert!(!function.parameters[3].required);

        assert_eq!(function.parameters[4].name, "blk");
        assert_eq!(function.parameters[4].type_, "Proc");
        assert!(!function.parameters[4].required);
    }

    /// Class attributes, struct constants and aliases build a return type index.
    #[test]
    fn collects_class_module_and_struct_types() {
        let source = concat!(
            "class Point\n",
            "  # @!attribute [Integer] x\n",
            "  attr_accessor :x, :y\n",
            "end\n",
            "Record = Struct.new(:id, :name)\n",
            "UserId = Integer\n",
        );
        let types = RubyAdapter::new().collect_types(source);
        assert_eq!(types["Point"].type_, "Point");
        assert_eq!(types["Point"].fields.len(), 2);
        assert_eq!(types["Point"].fields[0].name, "x");
        assert_eq!(types["Point"].fields[0].type_, "Integer");
        assert_eq!(types["Point"].fields[1].name, "y");
        assert_eq!(types["Record"].type_, "Struct");
        assert_eq!(types["Record"].fields.len(), 2);
        assert_eq!(types["Record"].fields[1].name, "name");
        assert_eq!(types["UserId"].type_, "Integer");
    }

    /// Assignment calls, guards and the implicit final expression become steps.
    #[test]
    fn analyzes_call_branch_transform_and_return_steps() {
        let source = concat!(
            "def normalize(text)\n",
            "  cleaned = strip(text)\n",
            "  if cleaned.nil?\n",
            "    raise ArgumentError, \"EMPTY_TEXT\"\n",
            "  end\n",
            "  padded = \"[#{cleaned}]\"\n",
            "  cleaned\n",
            "end\n",
        );
        let adapter = RubyAdapter::new();
        let found = adapter.discover(source, "pkg/normalize.rb");
        let analysis = adapter.analyze(&found[0], source, &["strip".to_owned()]);
        let kinds: Vec<StepKind> = analysis.steps.iter().map(|step| step.kind).collect();
        assert_eq!(
            kinds,
            vec![
                StepKind::Call,
                StepKind::Branch,
                StepKind::Transform,
                StepKind::Return
            ]
        );
        assert!(analysis.steps.iter().all(|step| !step.code.contains('\n')));
        assert!(analysis.steps.iter().any(|step| {
            step.kind == StepKind::Branch && step.output == "throws EMPTY_TEXT | continues"
        }));
        assert!(analysis.callee_names.iter().any(|name| name == "strip"));
        assert!(analysis
            .steps
            .iter()
            .any(|step| step.calls_fn_id.as_deref() == Some("strip")));
    }

    /// Two guarded `raise` sites with one code collapse to a single `ErrorSpec`.
    #[test]
    fn extracts_guarded_raise_error_with_dedup() {
        let source = concat!(
            "def check(text)\n",
            "  if text.empty?\n",
            "    raise ArgumentError, \"EMPTY_TEXT\"\n",
            "    raise ArgumentError, \"EMPTY_TEXT\"\n",
            "  end\n",
            "  text\n",
            "end\n",
        );
        let adapter = RubyAdapter::new();
        let found = adapter.discover(source, "pkg/check.rb");
        let analysis = adapter.analyze(&found[0], source, &[]);
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "EMPTY_TEXT");
        assert_eq!(analysis.errors[0].status, Some(400));
        assert_eq!(analysis.errors[0].when, "text.empty?");
    }

    /// Console, file, network and environment markers drive permissions and budgets.
    #[test]
    fn side_effects_and_pure_budget() {
        let effect = concat!(
            "def report(key)\n",
            "  puts ENV[\"API_KEY\"]\n",
            "  File.read(\"x\")\n",
            "  Net::HTTP.get(\"y\")\n",
            "end\n",
        );
        let adapter = RubyAdapter::new();
        let found = adapter.discover(effect, "pkg/report.rb");
        let analysis = adapter.analyze(&found[0], effect, &[]);
        assert_eq!(analysis.purity, Purity::SideEffect);
        for permission in [
            "console:out",
            "fs:read",
            "net:outbound",
            "secrets:read:API_KEY",
        ] {
            assert!(
                analysis.permissions.iter().any(|p| p == permission),
                "missing permission {permission}"
            );
        }
        assert_eq!(analysis.timeout_ms, 5000);
        assert_eq!(analysis.runtime, "node20");

        let pure = "def add(a, b)\n  a + b\nend\n";
        let found = adapter.discover(pure, "pkg/add.rb");
        let analysis = adapter.analyze(&found[0], pure, &[]);
        assert_eq!(analysis.purity, Purity::Pure);
        assert!(analysis.permissions.is_empty());
        assert_eq!(analysis.timeout_ms, 150);
        assert_eq!(analysis.runtime, "universal");
    }

    /// Repeating discovery and analysis on one source yields identical facts.
    #[test]
    fn analysis_is_deterministic() {
        let source = concat!(
            "# Describe the work.\n",
            "def work(text)\n",
            "  if text.empty?\n",
            "    raise ArgumentError, \"EMPTY_TEXT\"\n",
            "  end\n",
            "  puts text\n",
            "  text\n",
            "end\n",
        );
        let adapter = RubyAdapter::new();
        let first = adapter.discover(source, "pkg/work.rb");
        let second = adapter.discover(source, "pkg/work.rb");
        assert_eq!(first, second);
        assert_eq!(
            adapter.analyze(&first[0], source, &[]),
            adapter.analyze(&second[0], source, &[])
        );
    }
}
