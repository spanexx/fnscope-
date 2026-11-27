//! Workspace configuration for `fnscope open`.
//!
//! A workspace is an explicit, reviewable list of project roots. Paths in the
//! config are always relative to the config file; absolute paths and `..`
//! traversal are rejected so one local file cannot silently reach another tree.
//! With no workspace configuration the caller keeps the single-scope behavior
//! by building [`fnscope_server::WorkspaceConfig::single`].

use std::{
    collections::HashSet,
    net::SocketAddr,
    path::{Component, Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use fnscope_core::ScanOptions;
use fnscope_server::{is_valid_scope_id, InvokeMode, ScopeConfig, WorkspaceConfig};
use serde::Deserialize;

/// Directory below the output root that holds one manifest per scope.
pub const SCOPES_DIR: &str = "scopes";

/// Only version 1 of `fnscope.workspace.json` is understood.
const WORKSPACE_VERSION: u32 = 1;

/// File name users are expected to pass to `--workspace`.
///
/// Auto-discovery is intentionally not enabled (the path stays an explicit CLI
/// choice); the constant names the file in tests and for future help text.
#[allow(dead_code)]
pub const WORKSPACE_FILE: &str = "fnscope.workspace.json";

/// On-disk shape of `fnscope.workspace.json`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceFile {
    /// Schema version; unsupported versions fail instead of being guessed.
    version: u32,
    /// Preferred scope for the compatibility routes; the first scope when absent.
    #[serde(default)]
    default_scope: Option<String>,
    /// Configured projects in display order.
    #[serde(default)]
    scopes: Vec<ScopeEntry>,
}

/// One scope as written in the config file.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScopeEntry {
    /// Registry key, also the output directory name.
    id: String,
    /// Display label; defaults to the id.
    #[serde(default)]
    label: String,
    /// Optional sentence shown by the UI.
    #[serde(default)]
    note: String,
    /// Project root relative to the config file.
    path: PathBuf,
    /// Optional include globs; scanner defaults apply when absent.
    #[serde(default)]
    include: Vec<String>,
    /// Optional exclude globs; scanner defaults apply when absent.
    #[serde(default)]
    exclude: Vec<String>,
    /// Optional per-scope invoke override.
    #[serde(default)]
    invoke: Option<InvokeMode>,
    /// Optional starting function for the explorer.
    #[serde(default)]
    default_function_id: Option<String>,
}

/// Workspace selectors after flag-over-environment precedence is applied.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceSelection {
    /// Config file to read, when one was selected.
    pub config: Option<PathBuf>,
    /// Raw `ID=PATH` scope overrides, in command-line order.
    pub scopes: Vec<String>,
    /// Explicit default scope, which outranks the config file's choice.
    pub default_scope: Option<String>,
}

impl WorkspaceSelection {
    /// Applies `flag > environment` precedence without touching the filesystem.
    pub fn from_sources(
        workspace: Option<PathBuf>,
        scopes: Vec<String>,
        default_scope: Option<String>,
        env_workspace: Option<String>,
        env_scope: Option<String>,
    ) -> Result<Self> {
        let config = match workspace {
            Some(path) => Some(path),
            None => env_workspace
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
        };
        let scopes = if scopes.is_empty() {
            parse_env_scopes(env_scope.as_deref())?
        } else {
            scopes
        };
        Ok(Self {
            config,
            scopes,
            default_scope,
        })
    }

    /// Reports whether the user asked for workspace mode at all.
    pub fn is_configured(&self) -> bool {
        self.config.is_some() || !self.scopes.is_empty()
    }
}

/// One resolved scope ready to scan and serve.
#[derive(Clone, Debug, PartialEq)]
pub struct ScopePlan {
    /// Registry key and output directory name.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Optional explanation shown by the UI.
    pub note: String,
    /// Scanner options with the resolved absolute root and per-scope globs.
    pub scan_options: ScanOptions,
    /// Directory receiving this scope's manifest.
    pub out_dir: PathBuf,
    /// Manifest path handed to the server.
    pub manifest_path: PathBuf,
    /// Relative root shown to the UI; never absolute.
    pub display_root: String,
    /// Optional starting function.
    pub default_function_id: Option<String>,
    /// Effective invoke mode for this scope.
    pub invoke_mode: InvokeMode,
}

/// A fully resolved workspace: scopes, default and listener.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspacePlan {
    /// Scopes in configured order.
    pub scopes: Vec<ScopePlan>,
    /// Scope used by `/api/fn-graph.json` and `/api/invoke`.
    pub default_scope: String,
    /// Listener handed to the server.
    pub bind: SocketAddr,
    /// Workspace-wide invoke mode before per-scope overrides.
    pub invoke_mode: InvokeMode,
}

impl WorkspacePlan {
    /// Converts the plan into the exact server configuration types.
    pub fn server_config(&self) -> WorkspaceConfig {
        WorkspaceConfig {
            scopes: self
                .scopes
                .iter()
                .map(|scope| ScopeConfig {
                    id: scope.id.clone(),
                    label: scope.label.clone(),
                    note: scope.note.clone(),
                    scan_root: scope.scan_options.target_dir.clone(),
                    display_root: scope.display_root.clone(),
                    manifest_path: scope.manifest_path.clone(),
                    default_function_id: scope.default_function_id.clone(),
                    invoke_mode: Some(scope.invoke_mode),
                })
                .collect(),
            default_scope: self.default_scope.clone(),
            bind: self.bind,
            invoke_mode: self.invoke_mode,
        }
    }
}

/// Intermediate scope before output paths and scanner defaults are attached.
#[derive(Clone, Debug)]
struct ScopeSeed {
    id: String,
    label: String,
    note: String,
    root: PathBuf,
    display_root: String,
    include: Vec<String>,
    exclude: Vec<String>,
    invoke: Option<InvokeMode>,
    default_function_id: Option<String>,
}

/// Resolves a selection into scan plans; reads the config file when present.
pub fn load_plan(
    selection: &WorkspaceSelection,
    out_root: &Path,
    bind: SocketAddr,
    invoke_mode: InvokeMode,
) -> Result<WorkspacePlan> {
    let cwd = absolute(Path::new("."))?;
    load_plan_from(selection, out_root, bind, invoke_mode, &cwd)
}

/// Implementation of [`load_plan`] with an injectable working directory for tests.
fn load_plan_from(
    selection: &WorkspaceSelection,
    out_root: &Path,
    bind: SocketAddr,
    invoke_mode: InvokeMode,
    cwd: &Path,
) -> Result<WorkspacePlan> {
    if !selection.is_configured() {
        bail!("no workspace configuration");
    }

    let cli_scopes = parse_scope_args(&selection.scopes)?;
    let (mut seeds, file_default) = match &selection.config {
        Some(path) => {
            let config_path = absolute(path)?;
            let raw = std::fs::read_to_string(&config_path)
                .with_context(|| format!("reading workspace {}", config_path.display()))?;
            let file: WorkspaceFile = serde_json::from_str(&raw)
                .with_context(|| format!("parsing workspace {}", config_path.display()))?;
            if file.version != WORKSPACE_VERSION {
                bail!(
                    "unsupported workspace version {} in {}; expected {}",
                    file.version,
                    config_path.display(),
                    WORKSPACE_VERSION
                );
            }
            let base = config_path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .map(Path::to_path_buf)
                .unwrap_or_else(|| cwd.to_path_buf());
            let seeds = file
                .scopes
                .into_iter()
                .map(|entry| seed_from_entry(entry, &base))
                .collect::<Result<Vec<_>>>()?;
            (seeds, file.default_scope)
        }
        None => (Vec::new(), None),
    };

    for (id, path) in cli_scopes {
        let root = resolve_scope_root(cwd, &path, &id)?;
        let display_root = display_root(&path);
        match seeds.iter_mut().find(|seed| seed.id == id) {
            Some(seed) => {
                seed.root = root;
                seed.display_root = display_root;
            }
            None => seeds.push(ScopeSeed {
                label: id.clone(),
                id,
                note: String::new(),
                root,
                display_root,
                include: Vec::new(),
                exclude: Vec::new(),
                invoke: None,
                default_function_id: None,
            }),
        }
    }

    validate_seeds(&seeds)?;

    let default_scope = selection
        .default_scope
        .clone()
        .or(file_default)
        .unwrap_or_else(|| seeds[0].id.clone());
    if !seeds.iter().any(|seed| seed.id == default_scope) {
        bail!("default scope {:?} is not configured", default_scope);
    }

    let scopes = seeds
        .into_iter()
        .map(|seed| {
            let out_dir = out_root.join(SCOPES_DIR).join(&seed.id);
            let mut scan_options = ScanOptions {
                target_dir: seed.root,
                ..Default::default()
            };
            if !seed.include.is_empty() {
                scan_options.includes = seed.include;
            }
            if !seed.exclude.is_empty() {
                scan_options.excludes = seed.exclude;
            }
            let invoke_mode = seed.invoke.unwrap_or(invoke_mode);
            ScopePlan {
                id: seed.id,
                label: seed.label,
                note: seed.note,
                scan_options,
                manifest_path: out_dir.join("fn-graph.json"),
                out_dir,
                display_root: seed.display_root,
                default_function_id: seed.default_function_id,
                invoke_mode,
            }
        })
        .collect();

    Ok(WorkspacePlan {
        scopes,
        default_scope,
        bind,
        invoke_mode,
    })
}

/// Builds a seed from one config entry, resolving its path against the config file.
fn seed_from_entry(entry: ScopeEntry, base: &Path) -> Result<ScopeSeed> {
    let root = resolve_scope_root(base, &entry.path, &entry.id)?;
    let display_root = display_root(&entry.path);
    let label = if entry.label.is_empty() {
        entry.id.clone()
    } else {
        entry.label
    };
    Ok(ScopeSeed {
        id: entry.id,
        label,
        note: entry.note,
        root,
        display_root,
        include: entry.include,
        exclude: entry.exclude,
        invoke: entry.invoke,
        default_function_id: entry.default_function_id,
    })
}

/// Rejects unsafe relative paths and joins the accepted path onto `base`.
fn resolve_scope_root(base: &Path, raw: &Path, id: &str) -> Result<PathBuf> {
    if raw.as_os_str().is_empty() {
        bail!("scope {id:?} must name a path");
    }
    if is_absolute_like(raw) {
        bail!(
            "scope {id:?} path must be relative to the workspace file, got {}",
            raw.display()
        );
    }
    for component in raw.components() {
        match component {
            Component::ParentDir => bail!(
                "scope {id:?} path must not contain '..', got {}",
                raw.display()
            ),
            Component::RootDir | Component::Prefix(_) => bail!(
                "scope {id:?} path must be relative to the workspace file, got {}",
                raw.display()
            ),
            _ => {}
        }
    }
    Ok(base.join(raw))
}

/// Detects Windows drive/UNC and root-relative paths even when running on Unix.
fn is_absolute_like(path: &Path) -> bool {
    if path.is_absolute() {
        return true;
    }
    let text = path.to_string_lossy();
    if text.starts_with("\\\\") || text.starts_with("//") {
        return true;
    }
    let bytes = text.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// Renders the configured path as the relative root shown to the UI.
fn display_root(path: &Path) -> String {
    let parts: Vec<String> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

/// Checks the merged scope list before any scan starts.
fn validate_seeds(seeds: &[ScopeSeed]) -> Result<()> {
    if seeds.is_empty() {
        bail!("workspace must configure at least one scope");
    }
    let mut seen: HashSet<&str> = HashSet::new();
    for seed in seeds {
        if !is_valid_scope_id(&seed.id) {
            bail!("scope id {:?} must match [a-z0-9][a-z0-9-]*", seed.id);
        }
        if !seen.insert(seed.id.as_str()) {
            bail!("duplicate scope id: {}", seed.id);
        }
    }
    Ok(())
}

/// Parses repeatable `--scope ID=PATH` values, rejecting duplicates.
fn parse_scope_args(raw: &[String]) -> Result<Vec<(String, PathBuf)>> {
    let mut parsed = Vec::new();
    let mut seen = HashSet::new();
    for value in raw {
        let (id, path) = parse_scope_arg(value)?;
        if !seen.insert(id.clone()) {
            bail!("duplicate scope id: {id}");
        }
        parsed.push((id, path));
    }
    Ok(parsed)
}

/// Splits one `ID=PATH` selector and validates its id syntax.
pub fn parse_scope_arg(value: &str) -> Result<(String, PathBuf)> {
    let (id, path) = value
        .split_once('=')
        .with_context(|| format!("scope {value:?} must use ID=PATH"))?;
    let id = id.trim();
    let path = path.trim();
    if id.is_empty() {
        bail!("scope {value:?} has an empty id");
    }
    if path.is_empty() {
        bail!("scope {value:?} has an empty path");
    }
    if !is_valid_scope_id(id) {
        bail!("scope id {id:?} must match [a-z0-9][a-z0-9-]*");
    }
    Ok((id.to_owned(), PathBuf::from(path)))
}

/// Reads `FNSCOPE_SCOPE`, accepting comma-separated `ID=PATH` entries.
fn parse_env_scopes(value: Option<&str>) -> Result<Vec<String>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    Ok(value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Makes a path absolute without requiring it to exist, normalizing `.`/`..`.
fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    match std::path::absolute(path) {
        Ok(path) => Ok(path),
        Err(error) => std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .with_context(|| format!("resolving {} ({error})", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    /// A uniquely named temp directory that cleans itself up.
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "fnscope-workspace-{label}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.0.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(&path, contents).expect("write file");
            path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Returns the committed two-scope fixture config.
    fn fixture_config() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/workspace/fnscope.workspace.json")
    }

    /// Minimal bind address for plans that never open a socket.
    fn bind() -> SocketAddr {
        "127.0.0.1:4310".parse().expect("valid address")
    }

    /// Loads a selection against an explicit working directory.
    fn plan(selection: &WorkspaceSelection, base: &Path) -> Result<WorkspacePlan> {
        load_plan_from(
            selection,
            Path::new(".fnscope"),
            bind(),
            InvokeMode::Simulate,
            base,
        )
    }

    /// Writes a workspace config into a temp dir and returns its path.
    fn config(dir: &TestDir, contents: &str) -> PathBuf {
        dir.write(WORKSPACE_FILE, contents)
    }

    #[test]
    fn parses_the_two_scope_fixture() {
        let selection = WorkspaceSelection {
            config: Some(fixture_config()),
            ..Default::default()
        };
        let plan = plan(&selection, Path::new(".")).expect("fixture parses");

        assert_eq!(plan.default_scope, "alpha");
        assert_eq!(plan.scopes.len(), 2);
        assert_eq!(plan.scopes[0].id, "alpha");
        assert_eq!(plan.scopes[0].label, "Alpha");
        assert_eq!(plan.scopes[0].display_root, "alpha");
        assert_eq!(plan.scopes[0].scan_options.includes, vec!["**/*.ts"]);
        assert_eq!(plan.scopes[1].id, "beta");
        assert_eq!(plan.scopes[1].display_root, "beta");
        assert!(plan.scopes[0]
            .scan_options
            .target_dir
            .ends_with(Path::new("alpha")));
        assert_eq!(
            plan.scopes[0].manifest_path,
            Path::new(".fnscope/scopes/alpha/fn-graph.json")
        );
        assert_eq!(plan.scopes[0].invoke_mode, InvokeMode::Simulate);
    }

    #[test]
    fn relative_paths_resolve_against_the_config_file() {
        let dir = TestDir::new("relative");
        fs::create_dir_all(dir.path().join("alpha")).expect("scope dir");
        let config_path = config(
            &dir,
            r#"{"version":1,"defaultScope":"alpha","scopes":[{"id":"alpha","path":"alpha"}]}"#,
        );
        let selection = WorkspaceSelection {
            config: Some(config_path),
            ..Default::default()
        };

        let plan = plan(&selection, Path::new("/elsewhere")).expect("config parses");
        assert_eq!(
            plan.scopes[0].scan_options.target_dir,
            dir.path().join("alpha")
        );
        assert_eq!(plan.scopes[0].display_root, "alpha");
    }

    #[test]
    fn cli_scope_override_keeps_config_metadata() {
        let dir = TestDir::new("override");
        fs::create_dir_all(dir.path().join("alpha")).expect("alpha");
        fs::create_dir_all(dir.path().join("other")).expect("other");
        let config_path = config(
            &dir,
            r#"{"version":1,"defaultScope":"alpha","scopes":[
                {"id":"alpha","label":"Configured","note":"keep me","path":"alpha",
                 "include":["**/*.ts"],"invoke":"node"}]}"#,
        );
        let selection = WorkspaceSelection {
            config: Some(config_path),
            scopes: vec!["alpha=other".to_owned()],
            default_scope: None,
        };

        let plan = plan(&selection, dir.path()).expect("override parses");
        assert_eq!(plan.scopes.len(), 1);
        assert_eq!(plan.scopes[0].label, "Configured");
        assert_eq!(plan.scopes[0].note, "keep me");
        assert_eq!(
            plan.scopes[0].scan_options.target_dir,
            dir.path().join("other")
        );
        assert_eq!(plan.scopes[0].display_root, "other");
        assert_eq!(plan.scopes[0].scan_options.includes, vec!["**/*.ts"]);
        assert_eq!(plan.scopes[0].invoke_mode, InvokeMode::Node);
    }

    #[test]
    fn cli_scopes_work_without_a_config_file() {
        let dir = TestDir::new("cli-only");
        fs::create_dir_all(dir.path().join("beta")).expect("beta");
        let selection = WorkspaceSelection {
            config: None,
            scopes: vec!["beta=beta".to_owned()],
            default_scope: Some("beta".to_owned()),
        };

        let plan = plan(&selection, dir.path()).expect("CLI scopes parse");
        assert_eq!(plan.default_scope, "beta");
        assert_eq!(plan.scopes[0].label, "beta");
        assert_eq!(plan.scopes[0].display_root, "beta");
    }

    #[test]
    fn default_scope_precedence_is_flag_then_file_then_first() {
        let dir = TestDir::new("defaults");
        fs::create_dir_all(dir.path().join("alpha")).expect("alpha");
        fs::create_dir_all(dir.path().join("beta")).expect("beta");
        let with_default = config(
            &dir,
            r#"{"version":1,"defaultScope":"beta","scopes":[
                {"id":"alpha","path":"alpha"},{"id":"beta","path":"beta"}]}"#,
        );

        let from_file = plan(
            &WorkspaceSelection {
                config: Some(with_default),
                ..Default::default()
            },
            dir.path(),
        )
        .expect("file default");
        assert_eq!(from_file.default_scope, "beta");

        let from_flag = plan(
            &WorkspaceSelection {
                config: Some(dir.path().join(WORKSPACE_FILE)),
                default_scope: Some("alpha".to_owned()),
                ..Default::default()
            },
            dir.path(),
        )
        .expect("flag default");
        assert_eq!(from_flag.default_scope, "alpha");

        let no_default = config(
            &dir,
            r#"{"version":1,"scopes":[
                {"id":"alpha","path":"alpha"},{"id":"beta","path":"beta"}]}"#,
        );
        let first = plan(
            &WorkspaceSelection {
                config: Some(no_default),
                ..Default::default()
            },
            dir.path(),
        )
        .expect("first scope default");
        assert_eq!(first.default_scope, "alpha");
    }

    #[test]
    fn rejects_duplicate_invalid_and_empty_ids() {
        let dir = TestDir::new("bad-ids");
        let duplicate = config(
            &dir,
            r#"{"version":1,"scopes":[
                {"id":"alpha","path":"a"},{"id":"alpha","path":"b"}]}"#,
        );
        let invalid = config(
            &dir,
            r#"{"version":1,"scopes":[{"id":"Alpha","path":"a"}]}"#,
        );
        let empty = config(&dir, r#"{"version":1,"scopes":[{"id":"","path":"a"}]}"#);

        for (label, path) in [
            ("duplicate", duplicate),
            ("invalid", invalid),
            ("empty", empty),
        ] {
            let selection = WorkspaceSelection {
                config: Some(path),
                ..Default::default()
            };
            assert!(
                plan(&selection, dir.path()).is_err(),
                "{label} id must be rejected"
            );
        }
    }

    #[test]
    fn rejects_absolute_and_traversing_paths() {
        let dir = TestDir::new("bad-paths");
        for (label, path) in [
            ("absolute", "/tmp/outside"),
            ("windows", "C:\\Users\\me\\repo"), // culture-allow: absolute path: test input
            ("traversal", "../outside"),
            ("nested-traversal", "alpha/../beta"),
        ] {
            let config_path = config(
                &dir,
                &format!(r#"{{"version":1,"scopes":[{{"id":"alpha","path":{path:?}}}]}}"#),
            );
            let selection = WorkspaceSelection {
                config: Some(config_path),
                ..Default::default()
            };
            let error = plan(&selection, dir.path()).expect_err(label);
            let message = format!("{error}");
            assert!(
                message.contains("relative") || message.contains(".."),
                "{label}: {message}"
            );
        }
    }

    #[test]
    fn rejects_missing_default_scope_and_unsupported_version() {
        let dir = TestDir::new("bad-default");
        let missing_default = config(
            &dir,
            r#"{"version":1,"defaultScope":"gamma","scopes":[{"id":"alpha","path":"alpha"}]}"#,
        );
        let bad_version = config(
            &dir,
            r#"{"version":2,"scopes":[{"id":"alpha","path":"alpha"}]}"#,
        );

        for (label, path) in [
            ("missing default", missing_default),
            ("bad version", bad_version),
        ] {
            let selection = WorkspaceSelection {
                config: Some(path),
                ..Default::default()
            };
            assert!(
                plan(&selection, dir.path()).is_err(),
                "{label} must be rejected"
            );
        }
    }

    #[test]
    fn rejects_duplicate_cli_scopes_and_bad_selectors() {
        for value in ["alpha", "=alpha", "alpha=", "Alpha=alpha"] {
            assert!(parse_scope_arg(value).is_err(), "{value} must fail");
        }
        assert_eq!(
            parse_scope_arg("alpha=src").expect("valid selector"),
            ("alpha".to_owned(), PathBuf::from("src"))
        );

        let dir = TestDir::new("dup-cli");
        let selection = WorkspaceSelection {
            config: None,
            scopes: vec!["alpha=a".to_owned(), "alpha=b".to_owned()],
            default_scope: None,
        };
        assert!(plan(&selection, dir.path()).is_err());
    }

    #[test]
    fn env_scope_is_used_only_without_flags() {
        let selection = WorkspaceSelection::from_sources(
            None,
            Vec::new(),
            None,
            Some(" env.workspace.json ".to_owned()),
            Some("alpha=a, beta=b".to_owned()),
        )
        .expect("env sources");
        assert_eq!(selection.config, Some(PathBuf::from("env.workspace.json")));
        assert_eq!(selection.scopes, vec!["alpha=a", "beta=b"]);

        let flag_wins = WorkspaceSelection::from_sources(
            Some(PathBuf::from("flag.json")),
            vec!["flag=flag".to_owned()],
            None,
            Some("env.json".to_owned()),
            Some("env=env".to_owned()),
        )
        .expect("flag sources");
        assert_eq!(flag_wins.config, Some(PathBuf::from("flag.json")));
        assert_eq!(flag_wins.scopes, vec!["flag=flag"]);
    }

    #[test]
    fn empty_selection_keeps_single_scope_mode() {
        let selection = WorkspaceSelection::from_sources(None, Vec::new(), None, None, None)
            .expect("empty selection");
        assert!(!selection.is_configured());

        let single = fnscope_server::WorkspaceConfig::single(
            PathBuf::from(".fnscope/fn-graph.json"),
            bind(),
            InvokeMode::Simulate,
        );
        assert_eq!(single.scopes.len(), 1);
        assert_eq!(single.scopes[0].id, "default");
        assert_eq!(single.default_scope, "default");
        assert_eq!(single.scopes[0].display_root, ".");
        assert_eq!(
            single.scopes[0].manifest_path,
            PathBuf::from(".fnscope/fn-graph.json")
        );
        fnscope_server::validate_workspace_config(&single).expect("single config is valid");
    }

    #[test]
    fn server_config_carries_relative_roots_and_scope_invoke_modes() {
        let dir = TestDir::new("server-config");
        fs::create_dir_all(dir.path().join("alpha")).expect("alpha");
        let config_path = config(
            &dir,
            r#"{"version":1,"defaultScope":"alpha","scopes":[
                {"id":"alpha","path":"alpha","invoke":"node"}]}"#,
        );
        let plan = plan(
            &WorkspaceSelection {
                config: Some(config_path),
                ..Default::default()
            },
            dir.path(),
        )
        .expect("plan");

        let server = plan.server_config();
        assert_eq!(server.default_scope, "alpha");
        assert_eq!(server.scopes[0].display_root, "alpha");
        assert_eq!(server.scopes[0].invoke_mode, Some(InvokeMode::Node));
        assert!(!server.scopes[0].display_root.contains(':'));
    }
}
