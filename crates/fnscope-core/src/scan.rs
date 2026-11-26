//! Deterministic scanner orchestration over a source tree.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use fnscope_model::{CallerRef, FnGraph, FnRecord, Purity, ReturnShape};
use fnscope_parser::{registry, BodyAnalysis, DiscoveredFn, LanguageAdapter, Registry};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};

use crate::{ast_hash, make_id};

/// Which files a scan reads and what output path it advertises.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScanOptions {
    /// Directory walked recursively; paths in the graph are relative to it.
    pub target_dir: PathBuf,
    /// Glob patterns that admit a file when any one matches.
    pub includes: Vec<String>,
    /// Glob patterns that reject a file when any one matches.
    pub excludes: Vec<String>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            target_dir: PathBuf::from("."),
            includes: vec![
                "**/*.ts",
                "**/*.tsx",
                "**/*.js",
                "**/*.jsx",
                "**/*.rs",
                "**/*.py",
                "**/*.go",
                "**/*.java",
                "**/*.c",
                "**/*.cpp",
                "**/*.cc",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            excludes: vec![
                "**/node_modules/**",
                "**/dist/**",
                // Cargo's build directory holds generated sources, not authored code.
                "**/target/**",
                "**/.fnscope/**",
                // Python's bytecode cache holds generated modules, not authored code.
                "**/__pycache__/**",
                "**/*.test.ts",
                "**/*.test.tsx",
                "**/*.spec.ts",
                "**/*.spec.tsx",
                "**/.*/**",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        }
    }
}

/// Counts and non-fatal problems produced by a scan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScanReport {
    /// Path where [`crate::write_manifest`] will place the graph.
    pub manifest_path: PathBuf,
    /// Number of functions written into the graph.
    pub function_count: usize,
    /// Number of source files that were read successfully.
    pub file_count: usize,
    /// Human-readable notes for skipped files and directories.
    pub warnings: Vec<String>,
}

/// One parsed file before ids and body analysis are assigned.
///
/// The adapter is the only language-specific value that crosses into core.
struct ScannedFile<'a> {
    relative: String,
    module: String,
    adapter: &'a dyn LanguageAdapter,
    source: String,
    found: Vec<DiscoveredFn>,
    ids: Vec<String>,
}

/// One function with its resolved body and the context needed to emit a record.
struct AnalyzedFn {
    id: String,
    relative: String,
    module: String,
    found: DiscoveredFn,
    body: BodyAnalysis,
}

/// Compiles user-supplied glob patterns into a deterministic matcher.
fn build_glob_set(patterns: &[String], label: &str) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob =
            Glob::new(pattern).with_context(|| format!("invalid {label} glob {pattern:?}"))?;
        builder.add(glob);
    }
    builder.build().context("building glob set")
}

/// Returns a POSIX-style path relative to the scan root for diagnostics.
fn display_relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Recursively collects readable, matching files in sorted order.
fn collect_files(
    dir: &Path,
    root: &Path,
    includes: &GlobSet,
    excludes: &GlobSet,
    out: &mut BTreeSet<String>,
    warnings: &mut Vec<String>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            warnings.push(format!(
                "skipping unreadable directory {}: {err}",
                display_relative(dir, root)
            ));
            return;
        }
    };

    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => paths.push(entry.path()),
            Err(err) => warnings.push(format!(
                "skipping unreadable entry in {}: {err}",
                display_relative(dir, root)
            )),
        }
    }
    paths.sort();

    for path in paths {
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            collect_files(&path, root, includes, excludes, out, warnings);
        } else if metadata.is_file() {
            let Ok(stripped) = path.strip_prefix(root) else {
                continue;
            };
            let relative = stripped.to_string_lossy().replace('\\', "/");
            if excludes.is_match(&relative) || !includes.is_match(&relative) {
                continue;
            }
            out.insert(relative);
        }
    }
}

/// Returns the slash-separated parent directory, or the stem for root files.
fn module_for_relative(relative: &str) -> String {
    match relative.rsplit_once('/') {
        Some((parent, _)) => parent.to_owned(),
        None => relative
            .rsplit_once('.')
            .map(|(stem, _)| stem.to_owned())
            .unwrap_or_else(|| relative.to_owned()),
    }
}

/// Slug of the owning directory, used to disambiguate colliding function ids.
fn parent_slug(relative: &str) -> String {
    let parent = relative
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("");
    let slug = make_id("", parent);
    if slug.is_empty() {
        "module".to_owned()
    } else {
        slug
    }
}

/// Finds the source line of a call snippet inside the caller's own source slice.
fn call_line(source: &str, code: &str, fallback: usize) -> usize {
    if code.is_empty() {
        return fallback.max(1);
    }
    match source.find(code) {
        Some(index) => {
            let offset = source[..index]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count();
            fallback + offset
        }
        None => fallback.max(1),
    }
}

/// Builds a validated, un-enriched function graph with the built-in adapters.
pub fn scan(
    options: &ScanOptions,
    out_dir: &Path,
    generated_at: String,
) -> Result<(FnGraph, ScanReport)> {
    let registry = registry();
    scan_with_registry(options, out_dir, generated_at, &registry)
}

/// Builds a graph with an injected registry, so a new language needs no core edit.
///
/// This is the single language-dispatch seam in the scanner: one extension
/// lookup per file, with no per-language branch anywhere in the graph builder.
pub fn scan_with_registry(
    options: &ScanOptions,
    out_dir: &Path,
    generated_at: String,
    registry: &Registry,
) -> Result<(FnGraph, ScanReport)> {
    let includes = build_glob_set(&options.includes, "include")?;
    let excludes = build_glob_set(&options.excludes, "exclude")?;

    let mut warnings = Vec::new();
    let mut paths = BTreeSet::new();
    collect_files(
        &options.target_dir,
        &options.target_dir,
        &includes,
        &excludes,
        &mut paths,
        &mut warnings,
    );

    let mut scanned = Vec::new();
    let mut types: BTreeMap<String, ReturnShape> = BTreeMap::new();
    for relative in paths {
        let absolute = options.target_dir.join(Path::new(&relative));
        let source = match std::fs::read_to_string(&absolute) {
            Ok(source) => source,
            Err(err) => {
                warnings.push(format!("skipping unreadable file {relative}: {err}"));
                continue;
            }
        };
        let extension = relative
            .rsplit_once('.')
            .map(|(_, ext)| ext)
            .unwrap_or_default();
        let Some(adapter) = registry.for_extension(extension) else {
            continue;
        };
        // Files are visited in sorted path order, so a later file wins when two
        // files declare the same type name; `extend` makes that deterministic.
        types.extend(adapter.collect_types(&source));
        let found = adapter.discover(&source, &relative);
        let module = module_for_relative(&relative);
        scanned.push(ScannedFile {
            relative,
            module,
            adapter,
            source,
            found,
            ids: Vec::new(),
        });
    }
    let file_count = scanned.len();

    // Pass one: assign stable ids and index every bare and qualified name.
    let mut used_ids: BTreeSet<String> = BTreeSet::new();
    let mut name_to_id: BTreeMap<String, String> = BTreeMap::new();
    for file in &mut scanned {
        for found in &file.found {
            let base = make_id(&file.relative, &found.qualified_name);
            let mut id = base.clone();
            if used_ids.contains(&id) {
                let parent = parent_slug(&file.relative);
                id = format!("{base}-{parent}");
                let mut suffix = 2;
                while used_ids.contains(&id) {
                    id = format!("{base}-{parent}-{suffix}");
                    suffix += 1;
                }
            }
            used_ids.insert(id.clone());
            file.ids.push(id.clone());

            if !name_to_id.contains_key(&found.name) {
                name_to_id.insert(found.name.clone(), id.clone());
            }
            name_to_id.insert(found.qualified_name.clone(), id);
        }
    }
    let known_names: Vec<String> = name_to_id.keys().cloned().collect();

    // Pass two: analyze bodies, resolve callee names and collect both edge directions.
    let mut analyzed: Vec<AnalyzedFn> = Vec::new();
    let mut reverse: BTreeMap<String, Vec<CallerRef>> = BTreeMap::new();
    let mut outgoing: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for file in scanned {
        let adapter = file.adapter;
        for (found, id) in file.found.into_iter().zip(file.ids) {
            // Resolve through the complete project-wide index: a type may be
            // declared in a file visited after the function that returns it.
            let mut found = found;
            adapter.resolve_return_fields(&mut found, &types);
            let mut body = adapter.analyze(&found, &file.source, &known_names);
            let mut callees = BTreeSet::new();

            for step in &mut body.steps {
                let resolved = step
                    .calls_fn_id
                    .as_ref()
                    .and_then(|name| name_to_id.get(name))
                    .cloned();
                step.calls_fn_id = resolved.clone();
                let Some(callee) = resolved else {
                    continue;
                };

                callees.insert(callee.clone());
                let line = call_line(&found.source_code, &step.code, found.line.max(1));
                let caller = CallerRef {
                    fn_id: Some(id.clone()),
                    name: found.name.clone(),
                    module: format!("{}:{}", file.relative, line),
                    call_site: step.code.clone(),
                };
                reverse.entry(callee).or_default().push(caller);
            }

            outgoing.insert(id.clone(), callees.into_iter().collect());
            analyzed.push(AnalyzedFn {
                id,
                relative: file.relative.clone(),
                module: file.module.clone(),
                found,
                body,
            });
        }
    }

    // Pass three: add external entrypoints only where no caller was discovered.
    for item in &analyzed {
        let has_internal_caller = reverse
            .get(&item.id)
            .is_some_and(|callers| callers.iter().any(|caller| caller.fn_id.is_some()));
        if has_internal_caller {
            continue;
        }
        let name = item.found.name.to_ascii_lowercase();
        let looks_like_entry = ["handler", "route", "main", "run", "entry"]
            .iter()
            .any(|fragment| name.contains(fragment))
            || item
                .found
                .tags
                .iter()
                .any(|tag| tag.eq_ignore_ascii_case("entrypoint"));
        if !looks_like_entry {
            continue;
        }

        let first_source_line = item
            .found
            .source_code
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_owned();
        reverse.entry(item.id.clone()).or_default().push(CallerRef {
            fn_id: None,
            name: "entrypoint".to_owned(),
            module: format!("{}:{}", item.relative, item.found.line.max(1)),
            call_site: first_source_line,
        });
    }

    // Pass four: propagate side effects and capabilities from callees to callers.
    let mut states: BTreeMap<String, (Purity, BTreeSet<String>)> = analyzed
        .iter()
        .map(|item| {
            (
                item.id.clone(),
                (
                    item.body.purity,
                    item.body.permissions.iter().cloned().collect(),
                ),
            )
        })
        .collect();
    for _ in 0..10 {
        let snapshot = states.clone();
        let mut changed = false;
        for (id, (purity, permissions)) in &snapshot {
            let mut next_purity = *purity;
            let mut next_permissions = permissions.clone();
            for callee in outgoing.get(id).into_iter().flatten() {
                if let Some((callee_purity, callee_permissions)) = snapshot.get(callee) {
                    if *callee_purity == Purity::SideEffect {
                        next_purity = Purity::SideEffect;
                    }
                    next_permissions.extend(callee_permissions.iter().cloned());
                }
            }
            if next_purity != *purity || next_permissions != *permissions {
                states.insert(id.clone(), (next_purity, next_permissions));
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Pass five: fill the public records and sort by id for byte-stable output.
    let mut functions: Vec<FnRecord> = analyzed
        .iter()
        .map(|item| {
            let (purity, permissions) = states.get(&item.id).cloned().unwrap_or_else(|| {
                (
                    item.body.purity,
                    item.body.permissions.iter().cloned().collect(),
                )
            });
            let purity_tag = if purity == Purity::Pure {
                "pure"
            } else {
                "side-effect"
            };
            let purity_tag_owned = purity_tag.to_owned();
            let mut tags: Vec<String> = Vec::new();
            for tag in item
                .found
                .tags
                .iter()
                .chain(std::iter::once(&purity_tag_owned))
            {
                if !tags.contains(tag) {
                    tags.push(tag.clone());
                }
            }
            let description = item
                .found
                .doc_description
                .as_ref()
                .filter(|text| !text.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| {
                    format!(
                        "Executes {} ordered steps and returns {}.",
                        item.body.steps.len(),
                        item.found.returns.type_
                    )
                });

            FnRecord {
                id: item.id.clone(),
                name: item.found.name.clone(),
                module: item.module.clone(),
                file_path: item.relative.clone(),
                line: item.found.line.max(1),
                ast_hash: ast_hash(&item.found.source_code),
                version: "1.0.0".to_owned(),
                runtime: if item.body.runtime.is_empty() {
                    "universal".to_owned()
                } else {
                    item.body.runtime.clone()
                },
                purity,
                timeout_ms: item.body.timeout_ms,
                permissions: permissions.into_iter().collect(),
                description,
                tags,
                called_by: reverse.get(&item.id).cloned().unwrap_or_default(),
                parameters: item.found.parameters.clone(),
                steps: item.body.steps.clone(),
                returns: item.found.returns.clone(),
                errors: item.body.errors.clone(),
                source_code: item.found.source_code.clone(),
            }
        })
        .collect();
    functions.sort_by(|left, right| left.id.cmp(&right.id));

    let graph = FnGraph {
        tool: "fnscope".to_owned(),
        schema_version: "1.0.0".to_owned(),
        generated_at,
        root_dir: options.target_dir.to_string_lossy().replace('\\', "/"),
        functions,
    };
    if let Err(message) = graph.validate() {
        bail!("{message}");
    }

    let report = ScanReport {
        manifest_path: out_dir.join("fn-graph.json"),
        function_count: graph.functions.len(),
        file_count,
        warnings,
    };
    Ok((graph, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_match_the_contract() {
        let options = ScanOptions::default();
        assert_eq!(options.target_dir, PathBuf::from("."));
        assert_eq!(
            options.includes,
            vec![
                "**/*.ts",
                "**/*.tsx",
                "**/*.js",
                "**/*.jsx",
                "**/*.rs",
                "**/*.py",
                "**/*.go",
                "**/*.java",
                "**/*.c",
                "**/*.cpp",
                "**/*.cc",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
        );
        for pattern in [
            "**/node_modules/**",
            "**/dist/**",
            "**/target/**",
            "**/.fnscope/**",
            "**/__pycache__/**",
            "**/*.test.ts",
            "**/*.test.tsx",
            "**/*.spec.ts",
            "**/*.spec.tsx",
            "**/.*/**",
        ] {
            assert!(options.excludes.iter().any(|value| value == pattern));
        }
    }

    #[test]
    fn helpers_choose_adapter_module_and_absolute_call_line() {
        let registry = registry();
        for (extension, id) in [
            ("ts", "typescript"),
            ("tsx", "tsx"),
            ("js", "javascript"),
            ("jsx", "jsx"),
            ("rs", "rust"),
            ("RS", "rust"),
        ] {
            assert_eq!(
                registry
                    .for_extension(extension)
                    .map(|adapter| adapter.id()),
                Some(id)
            );
        }
        assert!(registry.for_extension("json").is_none());
        assert_eq!(module_for_relative("nlp/roles.ts"), "nlp");
        assert_eq!(module_for_relative("main.ts"), "main");
        // The third argument is the function's absolute start line, so the
        // result is an absolute file line (99 + two newlines before the call).
        assert_eq!(call_line("first\nsecond\nfoo()", "foo()", 99), 101);
        assert_eq!(call_line("no match", "foo()", 7), 7);
    }

    /// Temp directory removed on drop so the test leaves no scratch files behind.
    struct TempScanDir {
        path: PathBuf,
    }

    impl TempScanDir {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0);
            let path =
                std::env::temp_dir().join(format!("fnscope-{tag}-{}-{nanos}", std::process::id()));
            std::fs::create_dir_all(&path).expect("create temp scan dir");
            Self { path }
        }
    }

    impl Drop for TempScanDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// A minimal adapter claiming a novel extension.
    struct FakeAdapter;

    impl LanguageAdapter for FakeAdapter {
        fn id(&self) -> &'static str {
            "fake"
        }

        fn extensions(&self) -> &'static [&'static str] {
            &["zig"]
        }

        fn discover(&self, _source: &str, _relative_path: &str) -> Vec<DiscoveredFn> {
            vec![DiscoveredFn {
                name: "zeta".to_owned(),
                qualified_name: "zeta".to_owned(),
                kind: fnscope_parser::FnKind::Declaration,
                line: 1,
                end_line: 1,
                source_code: "pub fn zeta() {}".to_owned(),
                parameters: Vec::new(),
                returns: ReturnShape {
                    type_: "void".to_owned(),
                    fields: Vec::new(),
                },
                doc_description: None,
                tags: Vec::new(),
                is_async: false,
                class_name: None,
            }]
        }

        fn analyze(
            &self,
            _found: &DiscoveredFn,
            _source: &str,
            _known_names: &[String],
        ) -> BodyAnalysis {
            BodyAnalysis::default()
        }

        fn collect_types(&self, _source: &str) -> BTreeMap<String, ReturnShape> {
            BTreeMap::new()
        }

        fn resolve_return_fields(
            &self,
            _found: &mut DiscoveredFn,
            _types: &BTreeMap<String, ReturnShape>,
        ) {
        }
    }

    /// A registered adapter is selected by core for an extension it uniquely claims.
    #[test]
    fn injected_adapter_is_selected_for_a_novel_extension() {
        let temp = TempScanDir::new("adapter-registry");
        std::fs::write(temp.path.join("main.zig"), "pub fn zeta() {}").expect("write fixture");
        let options = ScanOptions {
            target_dir: temp.path.clone(),
            includes: vec!["**/*.zig".to_owned()],
            ..ScanOptions::default()
        };

        // The built-in registry alone cannot scan this extension.
        let (builtin, _) = scan(
            &options,
            Path::new(".fnscope"),
            "2026-11-26T00:00:00Z".to_owned(),
        )
        .expect("built-in scan");
        assert!(builtin.functions.is_empty());

        let mut registry = Registry::new();
        registry.register(Box::new(FakeAdapter));
        let (graph, report) = scan_with_registry(
            &options,
            Path::new(".fnscope"),
            "2026-11-26T00:00:00Z".to_owned(),
            &registry,
        )
        .expect("injected scan");

        assert_eq!(report.function_count, 1);
        assert_eq!(report.file_count, 1);
        assert_eq!(graph.functions.len(), 1);
        assert_eq!(graph.functions[0].name, "zeta");
        assert_eq!(graph.functions[0].file_path, "main.zig");
    }
}
