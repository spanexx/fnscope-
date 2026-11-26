//! TypeScript, TSX, JavaScript and JSX adapter.
//!
//! All TS-family grammar knowledge lives in this module. `Lang` stays an
//! implementation detail: core never sees it, only this adapter does.

mod body;
mod discover;

use std::collections::BTreeMap;

use fnscope_model::ReturnShape;

use crate::adapter::LanguageAdapter;
use crate::resolve::resolve_fields;
use crate::types::{BodyAnalysis, DiscoveredFn, Lang};

/// One TypeScript-family grammar, selected at registration time.
pub struct TypeScriptAdapter {
    lang: Lang,
}

impl TypeScriptAdapter {
    /// Selects the grammar used by this adapter instance.
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    /// Grammar used for body analysis; `Lang::Rust` is unreachable through the registry.
    fn language(&self) -> tree_sitter::Language {
        match self.lang {
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::JavaScript | Lang::Jsx => tree_sitter_javascript::LANGUAGE.into(),
            _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        }
    }
}

impl LanguageAdapter for TypeScriptAdapter {
    fn id(&self) -> &'static str {
        match self.lang {
            Lang::Tsx => "tsx",
            Lang::JavaScript => "javascript",
            Lang::Jsx => "jsx",
            _ => "typescript",
        }
    }

    fn extensions(&self) -> &'static [&'static str] {
        match self.lang {
            Lang::Tsx => &["tsx"],
            Lang::JavaScript => &["js"],
            Lang::Jsx => &["jsx"],
            _ => &["ts"],
        }
    }

    fn discover(&self, source: &str, relative_path: &str) -> Vec<DiscoveredFn> {
        discover::discover(self.lang, source, relative_path)
    }

    fn analyze(&self, found: &DiscoveredFn, source: &str, known_names: &[String]) -> BodyAnalysis {
        body::analyze(found, self.language(), source, known_names)
    }

    fn collect_types(&self, source: &str) -> BTreeMap<String, ReturnShape> {
        discover::collect_types(self.lang, source)
    }

    fn resolve_return_fields(
        &self,
        found: &mut DiscoveredFn,
        types: &BTreeMap<String, ReturnShape>,
    ) {
        resolve_fields(found, types);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FnKind;
    use fnscope_model::Purity;

    /// Representative snippet keeps its pre-refactor discovery facts.
    #[test]
    fn typescript_discovery_matches_pre_refactor_facts() {
        let source = r#"
type Role = { id: string; label: string };
export class Service {
  run(x: string): Promise<Role> { return Promise.resolve({ id: "1", label: "admin" }); }
}
"#;
        let found = TypeScriptAdapter::new(Lang::TypeScript).discover(source, "src/service.ts");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "run");
        assert_eq!(found[0].qualified_name, "Service.run");
        assert_eq!(found[0].kind, FnKind::Method);
        assert_eq!(found[0].returns.type_, "Promise<Role>");
        assert_eq!(found[0].returns.fields.len(), 2);
        assert_eq!(found[0].returns.fields[0].name, "id");
    }

    /// Representative snippet keeps its pre-refactor steps, errors and effects.
    #[test]
    fn typescript_analysis_matches_pre_refactor_facts() {
        let source = "function evaluate(input: Input): Token[] {\n  const tokens: Token[] = tokenize(input.sentence);\n  if (!tokens) {\n    throw new FunctionError('TOKEN_NOT_FOUND');\n  }\n  return tokens;\n}";
        let found = crate::discover::discover(Lang::TypeScript, source, "src/evaluate.ts");
        assert_eq!(found.len(), 1);

        let analysis = TypeScriptAdapter::new(Lang::TypeScript).analyze(
            &found[0],
            source,
            &["tokenize".to_owned()],
        );
        assert_eq!(analysis.steps[0].calls_fn_id.as_deref(), Some("tokenize"));
        assert_eq!(analysis.steps[0].title, "Call tokenize()");
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].code, "TOKEN_NOT_FOUND");
        assert_eq!(analysis.errors[0].status, Some(422));
        assert_eq!(analysis.purity, Purity::Pure);
        assert_eq!(analysis.timeout_ms, 150);
    }
}
