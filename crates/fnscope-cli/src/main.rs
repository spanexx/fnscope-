//! `fnscope` binary: scan a source tree, optionally enrich it, and serve the UI.

mod cli;
mod watch;

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::Result;
use clap::Parser;
use fnscope_core::ScanOptions;
use fnscope_enrich::{CachedEnricher, Enricher, NoopEnricher};

use crate::cli::{Cli, Commands, InvokeKind};

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
pub async fn run_open(
    target_dir: PathBuf,
    port: u16,
    host: String,
    no_watch: bool,
    invoke: InvokeKind,
    enrich: bool,
    model: Option<String>,
) -> Result<()> {
    let out_dir = PathBuf::from(".fnscope");
    let options = build_options(target_dir, Vec::new(), Vec::new());
    let manifest_path = scan_and_write(&options, &out_dir, enrich, model.as_deref())?;

    let bind: SocketAddr = format!("{host}:{port}").parse()?;
    let config = fnscope_server::ServerConfig {
        manifest_path,
        bind,
        invoke_mode: invoke.into(),
    };

    if !no_watch {
        watch::spawn_watcher(options, out_dir)?;
    }

    fnscope_server::serve(config).await
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
    let (mut graph, report) =
        fnscope_core::scan(options, out_dir, chrono::Utc::now().to_rfc3339())?;
    println!(
        "scanned {} functions in {} files",
        report.function_count, report.file_count
    );

    if enrich {
        let enricher = build_enricher(out_dir, model)?;
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
            ..
        } => run_open(target, port, host, no_watch, invoke, enrich, model).await,
    }
}

#[cfg(test)]
mod tests {
    use super::missing_api_key_error;

    #[test]
    fn missing_api_key_error_names_model_and_every_provider_key() {
        let message = format!("{}", missing_api_key_error("gpt-4o-mini"));
        assert!(message.contains("gpt-4o-mini"));
        assert!(message.contains("OPENAI_API_KEY"));
        assert!(message.contains("GEMINI_API_KEY"));
        assert!(message.contains("ANTHROPIC_API_KEY"));
    }
}
