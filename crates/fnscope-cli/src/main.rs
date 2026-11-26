//! `fnscope` binary: scan a source tree, optionally enrich it, and serve the UI.

mod cli;
mod watch;
mod workspace;

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::Result;
use clap::Parser;
use fnscope_core::ScanOptions;
use fnscope_enrich::{CachedEnricher, Enricher, NoopEnricher};

use crate::cli::{Cli, Commands, InvokeKind};
use crate::workspace::WorkspaceSelection;

/// Scans a tree, optionally enriches the graph, and writes the manifest.
pub fn run_scan(
    target_dir: PathBuf,
    out: PathBuf,
    enrich: bool,
    model: Option<String>,
    include: Vec<String>,
    excludes: Vec<String>,
) -> Result<()> {
    let options = build_options(target_dir, include, excludes);
    let path = scan_and_write(&options, &out, enrich, model.as_deref())?;
    println!("wrote {}", path.display());
    Ok(())
}

/// Scans once, starts the optional watcher, then serves the explorer UI.
///
/// Workspace mode scans each scope into `.fnscope/scopes/<id>`; no workspace
/// configuration keeps one scan and one top-level `.fnscope/fn-graph.json`.
pub async fn run_open(
    target_dir: PathBuf,
    port: u16,
    host: String,
    no_watch: bool,
    invoke: InvokeKind,
    enrich: bool,
    model: Option<String>,
    workspace_request: WorkspaceSelection,
) -> Result<()> {
    let out_dir = PathBuf::from(".fnscope");
    let bind: SocketAddr = format!("{host}:{port}").parse()?;
    let invoke_mode = invoke.into();

    if workspace_request.is_configured() {
        let plan = workspace::load_plan(&workspace_request, &out_dir, bind, invoke_mode)?;
        scan_plan(&plan, &out_dir, enrich, model.as_deref())?;

        if !no_watch {
            let scopes = plan
                .scopes
                .iter()
                .map(|scope| watch::WatchScope {
                    id: scope.id.clone(),
                    options: scope.scan_options.clone(),
                    out_dir: scope.out_dir.clone(),
                })
                .collect();
            watch::spawn_workspace_watcher(scopes)?;
        }

        return fnscope_server::serve_workspace(plan.server_config()).await;
    }

    let options = build_options(target_dir, Vec::new(), Vec::new());
    let manifest_path = scan_and_write(&options, &out_dir, enrich, model.as_deref())?;
    let config = fnscope_server::WorkspaceConfig::single(manifest_path, bind, invoke_mode);

    if !no_watch {
        watch::spawn_watcher(options, out_dir)?;
    }

    fnscope_server::serve_workspace(config).await
}

/// Scans every planned scope independently, sharing the enrichment cache directory.
fn scan_plan(
    plan: &workspace::WorkspacePlan,
    out_root: &Path,
    enrich: bool,
    model: Option<&str>,
) -> Result<()> {
    for scope in &plan.scopes {
        println!(
            "scope {}: scanning {}",
            scope.id,
            scope.scan_options.target_dir.display()
        );
        scan_and_write_using(&scope.scan_options, &scope.out_dir, out_root, enrich, model)?;
    }
    Ok(())
}

/// Applies non-empty CLI glob overrides on top of the scanner defaults.
fn build_options(target_dir: PathBuf, include: Vec<String>, exclude: Vec<String>) -> ScanOptions {
    let mut options = ScanOptions {
        target_dir,
        ..Default::default()
    };
    if !include.is_empty() {
        options.includes = include;
    }
    if !exclude.is_empty() {
        options.excludes = exclude;
    }
    options
}

/// Runs the shared scan/enrich/write pipeline used by both commands.
fn scan_and_write(
    options: &ScanOptions,
    out_dir: &Path,
    enrich: bool,
    model: Option<&str>,
) -> Result<PathBuf> {
    scan_and_write_using(options, out_dir, out_dir, enrich, model)
}

/// Writes one manifest while keeping the enrichment cache in `cache_dir`.
fn scan_and_write_using(
    options: &ScanOptions,
    out_dir: &Path,
    cache_dir: &Path,
    enrich: bool,
    model: Option<&str>,
) -> Result<PathBuf> {
    let (mut graph, report) =
        fnscope_core::scan(options, out_dir, chrono::Utc::now().to_rfc3339())?;
    println!(
        "scanned {} functions in {} files",
        report.function_count, report.file_count
    );

    if enrich {
        let enricher = build_enricher(cache_dir, model)?;
        let report = enricher.enrich(&mut graph)?;
        println!(
            "enrich: {} cache hits, {} misses, {} API calls",
            report.cache_hits, report.cache_misses, report.api_calls
        );
    }

    Ok(fnscope_core::write_manifest(&graph, out_dir)?)
}

/// Selects the enricher, forwarding an explicit model or falling back offline.
fn build_enricher(out_dir: &Path, model: Option<&str>) -> Result<Box<dyn Enricher>> {
    let cache_path = out_dir.join("enrich-cache.json");

    match model {
        Some(name) => {
            if !has_api_key() {
                return Err(missing_api_key_error(name));
            }
            Ok(Box::new(CachedEnricher::from_env_with_model(
                cache_path,
                name.to_string(),
            )?))
        }
        None => match CachedEnricher::from_env(cache_path) {
            Ok(enricher) => Ok(Box::new(enricher)),
            Err(_) => {
                eprintln!(
                    "warning: --enrich needs one of \
                     OPENAI_API_KEY/GEMINI_API_KEY/ANTHROPIC_API_KEY; using deterministic fallbacks"
                );
                Ok(Box::new(NoopEnricher))
            }
        },
    }
}

/// Builds the readable error shown when `--model` has no provider key.
fn missing_api_key_error(name: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "--model {name} needs an API key; set one of \
         OPENAI_API_KEY/GEMINI_API_KEY/ANTHROPIC_API_KEY, or drop \
         --model to use deterministic fallbacks"
    )
}

/// Reports whether any provider key is present and non-empty.
fn has_api_key() -> bool {
    ["OPENAI_API_KEY", "GEMINI_API_KEY", "ANTHROPIC_API_KEY"]
        .iter()
        .any(|key| {
            std::env::var(key)
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
        })
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let command = cli.resolved_command();
    let target = command.target_path();

    match command {
        Commands::Scan {
            out,
            enrich,
            model,
            include,
            exclude,
            ..
        } => run_scan(target, out, enrich, model, include, exclude),
        Commands::Open {
            port,
            host,
            no_watch,
            invoke,
            enrich,
            model,
            workspace,
            scope,
            default_scope,
            ..
        } => {
            let selection = WorkspaceSelection::from_sources(
                workspace,
                scope,
                default_scope,
                std::env::var("FNSCOPE_WORKSPACE").ok(),
                std::env::var("FNSCOPE_SCOPE").ok(),
            )?;
            run_open(
                target, port, host, no_watch, invoke, enrich, model, selection,
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeSet,
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use fnscope_server::InvokeMode;

    /// A uniquely named temp directory that cleans itself up.
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "fnscope-cli-{label}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Returns the committed workspace fixture config.
    fn fixture_config() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/workspace/fnscope.workspace.json")
    }

    /// Loads the fixture plan into a temp output root.
    fn fixture_plan(out: &Path) -> workspace::WorkspacePlan {
        workspace::load_plan(
            &WorkspaceSelection {
                config: Some(fixture_config()),
                ..Default::default()
            },
            out,
            "127.0.0.1:4310".parse().expect("valid address"),
            InvokeMode::Simulate,
        )
        .expect("fixture plan")
    }

    #[test]
    fn missing_api_key_error_names_model_and_every_provider_key() {
        let message = format!("{}", missing_api_key_error("gpt-4o-mini"));
        assert!(message.contains("gpt-4o-mini"));
        assert!(message.contains("OPENAI_API_KEY"));
        assert!(message.contains("GEMINI_API_KEY"));
        assert!(message.contains("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn two_scope_fixture_scan_writes_isolated_manifests() {
        let out = TestDir::new("two-scope-scan");
        let plan = fixture_plan(out.path());
        assert_eq!(plan.default_scope, "alpha");
        scan_plan(&plan, out.path(), false, None).expect("both scopes scan");

        let alpha_path = out.path().join("scopes/alpha/fn-graph.json");
        let beta_path = out.path().join("scopes/beta/fn-graph.json");
        assert!(alpha_path.exists(), "alpha manifest must exist");
        assert!(beta_path.exists(), "beta manifest must exist");

        let alpha = fnscope_core::read_manifest(&alpha_path).expect("alpha manifest");
        let beta = fnscope_core::read_manifest(&beta_path).expect("beta manifest");
        let alpha_names: BTreeSet<&str> = alpha
            .functions
            .iter()
            .map(|function| function.name.as_str())
            .collect();
        let beta_names: BTreeSet<&str> = beta
            .functions
            .iter()
            .map(|function| function.name.as_str())
            .collect();

        assert_eq!(
            alpha_names,
            BTreeSet::from(["alphaEntry", "shared"]),
            "alpha must not contain beta functions"
        );
        assert_eq!(
            beta_names,
            BTreeSet::from(["betaEntry", "shared"]),
            "beta must not contain alpha functions"
        );
    }

    #[test]
    fn shared_name_keeps_one_identity_per_scope_and_different_bodies() {
        let out = TestDir::new("shared-identity");
        let plan = fixture_plan(out.path());
        scan_plan(&plan, out.path(), false, None).expect("both scopes scan");

        let alpha = fnscope_core::read_manifest(&out.path().join("scopes/alpha/fn-graph.json"))
            .expect("alpha manifest");
        let beta = fnscope_core::read_manifest(&out.path().join("scopes/beta/fn-graph.json"))
            .expect("beta manifest");

        let alpha_shared = alpha
            .functions
            .iter()
            .find(|function| function.name == "shared")
            .expect("alpha shared");
        let beta_shared = beta
            .functions
            .iter()
            .find(|function| function.name == "shared")
            .expect("beta shared");

        assert_eq!(
            alpha_shared.id, beta_shared.id,
            "same relative path plus name must produce the same id"
        );
        assert_ne!(
            alpha_shared.ast_hash, beta_shared.ast_hash,
            "identical ids must not hide different bodies"
        );
        assert!(alpha_shared.source_code.contains("* 2"));
        assert!(beta_shared.source_code.contains("* 10"));
    }

    #[test]
    fn fixture_server_config_exposes_relative_roots() {
        let out = TestDir::new("server-config");
        let plan = fixture_plan(out.path());
        let config = plan.server_config();

        assert_eq!(config.default_scope, "alpha");
        assert_eq!(config.scopes.len(), 2);
        assert_eq!(config.scopes[0].display_root, "alpha");
        assert_eq!(config.scopes[1].display_root, "beta");
        for scope in &config.scopes {
            assert!(
                !scope.display_root.contains(':') && !scope.display_root.starts_with('/'),
                "display root must stay relative: {}",
                scope.display_root
            );
        }
    }
}
