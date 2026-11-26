//! Command-line surface for `fnscope`.
//!
//! Parsing lives here so the runtime wiring in `main` stays small, and so the
//! command shape can be unit-tested without starting a server or a watcher.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// Top-level argument parser: a subcommand, or a bare target that implies `open`.
///
/// The `open` flags are repeated at the top level so `fnscope ./src --port 4311`
/// parses the same way as `fnscope open ./src --port 4311`.
#[derive(Debug, Parser)]
#[command(
    name = "fnscope",
    version,
    about = "Inspect functions in a source tree and serve an explorer UI",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    /// Explicit subcommand (`scan` or `open`), if any.
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Target directory when no subcommand is given; treated as `open`.
    pub target: Option<PathBuf>,

    /// Port for the `open` server.
    #[arg(long, default_value_t = 4310)]
    pub port: u16,

    /// Host address for the `open` server.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Disable the live file watcher when opening.
    #[arg(long)]
    pub no_watch: bool,

    /// How `/api/invoke` executes functions.
    #[arg(long, value_enum, default_value = "simulate")]
    pub invoke: InvokeKind,

    /// Enrich descriptions and step details via an LLM.
    #[arg(long)]
    pub enrich: bool,

    /// Model name to use together with `--enrich`.
    #[arg(long)]
    pub model: Option<String>,

    /// Workspace file describing several project scopes; omit for single-scope mode.
    #[arg(long, value_name = "FILE")]
    pub workspace: Option<PathBuf>,

    /// Add or override a scope as ID=PATH; repeatable.
    #[arg(long = "scope", value_name = "ID=PATH")]
    pub scope: Vec<String>,

    /// Scope served by the compatibility routes; defaults to the workspace default.
    #[arg(long, value_name = "ID")]
    pub default_scope: Option<String>,
}

/// Alias matching the name used by the locked API contract.
#[allow(dead_code)]
pub type FnScopeCli = Cli;

/// The two operations `fnscope` can perform.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Scan a source tree and write `.fnscope/fn-graph.json`.
    Scan {
        /// Directory to scan.
        target_dir: PathBuf,

        /// Output directory for generated artifacts.
        #[arg(long, default_value = ".fnscope")]
        out: PathBuf,

        /// Enrich the graph using an LLM plus the local cache.
        #[arg(long)]
        enrich: bool,

        /// Model name for enrichment (defaults to the configured low-cost model).
        #[arg(long)]
        model: Option<String>,

        /// Glob pattern of files to include; repeatable.
        #[arg(long)]
        include: Vec<String>,

        /// Glob pattern of files to exclude; repeatable.
        #[arg(long)]
        exclude: Vec<String>,
    },

    /// Scan once, then serve the explorer UI (optionally watching for edits).
    Open {
        /// Directory to scan and serve; defaults to the current directory.
        target_dir: Option<PathBuf>,

        /// Port to listen on.
        #[arg(long, default_value_t = 4310)]
        port: u16,

        /// Host address to bind.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Disable the live file watcher.
        #[arg(long)]
        no_watch: bool,

        /// How `/api/invoke` executes functions.
        #[arg(long, value_enum, default_value = "simulate")]
        invoke: InvokeKind,

        /// Enrich the graph using an LLM plus the local cache.
        #[arg(long)]
        enrich: bool,

        /// Model name for enrichment.
        #[arg(long)]
        model: Option<String>,

        /// Workspace file describing several project scopes; omit for single-scope mode.
        #[arg(long, value_name = "FILE")]
        workspace: Option<PathBuf>,

        /// Add or override a scope as ID=PATH; repeatable.
        #[arg(long = "scope", value_name = "ID=PATH")]
        scope: Vec<String>,

        /// Scope served by the compatibility routes; defaults to the workspace default.
        #[arg(long, value_name = "ID")]
        default_scope: Option<String>,
    },
}

/// Execution backend selected by `--invoke`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InvokeKind {
    /// Return deterministic simulated traces without running project code.
    Simulate,
    /// Import and call the real function with `node`.
    Node,
}

impl From<InvokeKind> for fnscope_server::InvokeMode {
    /// Maps the CLI value onto the server crate's invoke mode.
    fn from(kind: InvokeKind) -> Self {
        match kind {
            InvokeKind::Simulate => Self::Simulate,
            InvokeKind::Node => Self::Node,
        }
    }
}

impl Cli {
    /// Resolves a bare target (or no arguments at all) into an `Open` command.
    pub fn resolved_command(self) -> Commands {
        match self.command {
            Some(command) => command,
            None => Commands::Open {
                target_dir: self.target,
                port: self.port,
                host: self.host,
                no_watch: self.no_watch,
                invoke: self.invoke,
                enrich: self.enrich,
                model: self.model,
                workspace: self.workspace,
                scope: self.scope,
                default_scope: self.default_scope,
            },
        }
    }
}

impl Commands {
    /// Returns the directory this command operates on, defaulting to `.`.
    pub fn target_path(&self) -> PathBuf {
        match self {
            Self::Scan { target_dir, .. } => target_dir.clone(),
            Self::Open { target_dir, .. } => {
                target_dir.clone().unwrap_or_else(|| PathBuf::from("."))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_accepts_all_options() {
        let cli = Cli::try_parse_from([
            "fnscope",
            "scan",
            "./src",
            "--out",
            ".fnscope",
            "--enrich",
            "--model",
            "gpt-4o-mini",
            "--include",
            "**/*.ts",
            "--exclude",
            "**/*.test.ts",
        ])
        .expect("scan arguments should parse");

        match cli.command.expect("scan is a subcommand") {
            Commands::Scan {
                target_dir,
                out,
                enrich,
                model,
                include,
                exclude,
            } => {
                assert_eq!(target_dir, PathBuf::from("./src"));
                assert_eq!(out, PathBuf::from(".fnscope"));
                assert!(enrich);
                assert_eq!(model.as_deref(), Some("gpt-4o-mini"));
                assert_eq!(include, vec!["**/*.ts"]);
                assert_eq!(exclude, vec!["**/*.test.ts"]);
            }
            other => panic!("expected Scan, got {other:?}"),
        }
    }

    #[test]
    fn bare_target_with_open_flags_resolves_to_open() {
        let cli = Cli::try_parse_from([
            "fnscope",
            "./src",
            "--port",
            "4311",
            "--invoke",
            "node",
            "--no-watch",
        ])
        .expect("bare target arguments should parse");

        match cli.resolved_command() {
            Commands::Open {
                target_dir,
                port,
                no_watch,
                invoke,
                ..
            } => {
                assert_eq!(target_dir, Some(PathBuf::from("./src")));
                assert_eq!(port, 4311);
                assert!(no_watch);
                assert_eq!(invoke, InvokeKind::Node);
            }
            other => panic!("expected Open, got {other:?}"),
        }
    }

    #[test]
    fn open_without_target_uses_defaults() {
        let cli = Cli::try_parse_from(["fnscope", "open"]).expect("open should parse");
        let command = cli.resolved_command();

        match &command {
            Commands::Open {
                target_dir,
                port,
                host,
                no_watch,
                invoke,
                enrich,
                model,
                workspace,
                scope,
                default_scope,
            } => {
                assert_eq!(target_dir, &None);
                assert_eq!(*port, 4310);
                assert_eq!(host, "127.0.0.1");
                assert!(!no_watch);
                assert_eq!(*invoke, InvokeKind::Simulate);
                assert!(!enrich);
                assert!(model.is_none());
                assert!(workspace.is_none());
                assert!(scope.is_empty());
                assert!(default_scope.is_none());
            }
            other => panic!("expected Open, got {other:?}"),
        }
        assert_eq!(command.target_path(), PathBuf::from("."));
    }

    #[test]
    fn invalid_invoke_mode_is_rejected() {
        assert!(Cli::try_parse_from(["fnscope", "open", "--invoke", "python"]).is_err());
        assert!(Cli::try_parse_from(["fnscope", "./src", "--invoke", "python"]).is_err());
    }

    #[test]
    fn open_parses_workspace_flags() {
        let cli = Cli::try_parse_from([
            "fnscope",
            "open",
            "--workspace",
            "fnscope.workspace.json",
            "--scope",
            "alpha=alpha",
            "--scope",
            "beta=beta",
            "--default-scope",
            "beta",
        ])
        .expect("workspace flags should parse");

        match cli.command.expect("open is a subcommand") {
            Commands::Open {
                workspace,
                scope,
                default_scope,
                ..
            } => {
                assert_eq!(workspace, Some(PathBuf::from("fnscope.workspace.json")));
                assert_eq!(scope, vec!["alpha=alpha", "beta=beta"]);
                assert_eq!(default_scope.as_deref(), Some("beta"));
            }
            other => panic!("expected Open, got {other:?}"),
        }
    }

    #[test]
    fn bare_target_parses_workspace_flags() {
        let cli = Cli::try_parse_from([
            "fnscope",
            "./src",
            "--workspace",
            "ws.json",
            "--scope",
            "alpha=alpha",
        ])
        .expect("bare workspace flags should parse");

        match cli.resolved_command() {
            Commands::Open {
                target_dir,
                workspace,
                scope,
                ..
            } => {
                assert_eq!(target_dir, Some(PathBuf::from("./src")));
                assert_eq!(workspace, Some(PathBuf::from("ws.json")));
                assert_eq!(scope, vec!["alpha=alpha"]);
            }
            other => panic!("expected Open, got {other:?}"),
        }
    }
}
