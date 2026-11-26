//! Live-reload support: poll a directory fingerprint and rescan when it moves.
//!
//! Keeping this out of `cli` and `main` means argument parsing and serving stay
//! readable, and the pure fingerprint function is easy to unit-test.

use std::{
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use anyhow::Result;
use fnscope_core::ScanOptions;
use sha2::{Digest, Sha256};

/// Hashes the relative paths, modified times and sizes of every file below `dir`.
///
/// Reading failures are returned to the caller so the watcher can warn and keep
/// the previous fingerprint instead of treating an unreadable tree as a change.
pub fn fingerprint(dir: &Path) -> Result<String> {
    let mut entries = Vec::new();
    collect_entries(dir, dir, &mut entries)?;
    entries.sort();
    let joined = entries.join("\n");
    Ok(hex::encode(Sha256::digest(joined.as_bytes())))
}

/// One workspace root watched by the shared polling loop.
#[derive(Clone, Debug)]
pub struct WatchScope {
    /// Scope id used in log lines so one failure is attributable.
    pub id: String,
    /// Scanner options for this scope's root.
    pub options: ScanOptions,
    /// Directory receiving this scope's `fn-graph.json`.
    pub out_dir: PathBuf,
}

/// Spawns a detached background thread that rescans on fingerprint changes.
///
/// Rust terminates outstanding threads when `main` returns, so no join handle is
/// kept; the thread lives exactly as long as the server does.
pub fn spawn_watcher(options: ScanOptions, out_dir: PathBuf) -> Result<()> {
    std::thread::Builder::new()
        .name("fnscope-watch".to_string())
        .spawn(move || watch_loop(options, out_dir))?;
    Ok(())
}

/// Spawns the single polling loop shared by every workspace scope.
pub fn spawn_workspace_watcher(scopes: Vec<WatchScope>) -> Result<()> {
    std::thread::Builder::new()
        .name("fnscope-workspace-watch".to_string())
        .spawn(move || watch_workspace_loop(scopes))?;
    Ok(())
}

/// Recursively appends `relative|modified-nanos|length` triples for regular files.
fn collect_entries(root: &Path, dir: &Path, entries: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();

        if kind.is_dir() {
            collect_entries(root, &path, entries)?;
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let metadata = entry.metadata()?;
            let modified_nanos = metadata
                .modified()?
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            entries.push(format!("{relative}|{modified_nanos}|{}", metadata.len()));
        }
    }
    Ok(())
}

/// Polls every 500 ms, rescanning and rewriting the manifest on a change.
fn watch_loop(options: ScanOptions, out_dir: PathBuf) {
    let mut last: Option<String> = None;

    loop {
        std::thread::sleep(Duration::from_millis(500));

        match fingerprint(&options.target_dir) {
            Ok(current) => {
                let changed = last.as_ref().is_some_and(|previous| *previous != current);
                last = Some(current);

                if changed {
                    eprintln!("changed -> rescanning");
                    if let Err(error) = scan_and_replace(&options, &out_dir) {
                        eprintln!("warning: rescan failed: {error}");
                    }
                }
            }
            Err(error) => eprintln!(
                "warning: cannot read {} ({error}); treating as no change",
                options.target_dir.display()
            ),
        }
    }
}

/// Polls every scope through one loop; one unreadable or failing scope never stops the rest.
fn watch_workspace_loop(scopes: Vec<WatchScope>) {
    let mut last: Vec<Option<String>> = vec![None; scopes.len()];

    loop {
        std::thread::sleep(Duration::from_millis(500));

        for (index, scope) in scopes.iter().enumerate() {
            match fingerprint(&scope.options.target_dir) {
                Ok(current) => {
                    let changed = last[index]
                        .as_ref()
                        .is_some_and(|previous| *previous != current);
                    last[index] = Some(current);

                    if changed {
                        eprintln!("scope {} changed -> rescanning", scope.id);
                        if let Err(error) = scan_and_replace(&scope.options, &scope.out_dir) {
                            eprintln!("warning: scope {} rescan failed: {error}", scope.id);
                        }
                    }
                }
                Err(error) => eprintln!(
                    "warning: cannot read {} for scope {} ({error}); treating as no change",
                    scope.options.target_dir.display(),
                    scope.id
                ),
            }
        }
    }
}

/// Rescans one root and atomically replaces its manifest.
fn scan_and_replace(options: &ScanOptions, out_dir: &Path) -> Result<()> {
    let (graph, _) = fnscope_core::scan(options, out_dir, chrono::Utc::now().to_rfc3339())?;
    atomic_replace(out_dir, |temp_dir| {
        fnscope_core::write_manifest(&graph, temp_dir)
    })
}

/// Lists scope indices whose fingerprint changed since the previous poll.
#[cfg(test)]
fn changed_scope_indices(previous: &[Option<String>], current: &[String]) -> Vec<usize> {
    current
        .iter()
        .enumerate()
        .filter(|(index, fingerprint)| {
            previous
                .get(*index)
                .and_then(|value| value.as_ref())
                .is_some_and(|value| value.as_str() != fingerprint.as_str())
        })
        .map(|(index, _)| index)
        .collect()
}

/// Writes to a sibling temp file first so readers never observe partial JSON.
///
/// The writer is a closure so this helper never needs to name the graph type.
fn atomic_replace(out_dir: &Path, write: impl FnOnce(&Path) -> Result<PathBuf>) -> Result<()> {
    let temp_dir = out_dir.join(format!(".fnscope-tmp-{}", std::process::id()));

    let result = (|| -> Result<()> {
        std::fs::create_dir_all(&temp_dir)?;
        let staged = write(&temp_dir)?;
        let final_path = out_dir.join("fn-graph.json");

        // std::fs::rename cannot replace an existing file on Windows.
        if final_path.exists() {
            std::fs::remove_file(&final_path)?;
        }
        std::fs::rename(staged, final_path)?;
        Ok(())
    })();

    let _ = std::fs::remove_dir_all(&temp_dir);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::Write,
        time::{Duration, SystemTime},
    };

    /// A uniquely named temp directory that always cleans itself up.
    struct TestDir(PathBuf);

    impl TestDir {
        /// Creates a fresh directory below the platform temp directory.
        fn new(label: &str) -> Self {
            let unique = format!(
                "fnscope-watch-{label}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        /// Borrows the directory path.
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Writes a file and pins its modified time so fingerprints are comparable.
    fn write_file(dir: &TestDir, name: &str, contents: &str, modified: SystemTime) {
        let path = dir.path().join(name);
        let mut file = fs::File::create(path).expect("create file");
        file.write_all(contents.as_bytes()).expect("write file");
        file.set_modified(modified).expect("set modified time");
    }

    #[test]
    fn fingerprint_is_equal_for_identical_dirs_and_changes_after_edit() {
        let first_time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let later_time = first_time + Duration::from_secs(5);

        let first = TestDir::new("first");
        let second = TestDir::new("second");

        write_file(&first, "index.ts", "export const x = 1;", first_time);
        write_file(&second, "index.ts", "export const x = 1;", first_time);
        assert_eq!(
            fingerprint(first.path()).unwrap(),
            fingerprint(second.path()).unwrap()
        );

        write_file(&second, "index.ts", "export const x = 12345;", later_time);
        assert_ne!(
            fingerprint(first.path()).unwrap(),
            fingerprint(second.path()).unwrap()
        );
    }

    #[test]
    fn workspace_fingerprints_are_independent_per_scope() {
        let first_time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let later_time = first_time + Duration::from_secs(5);
        let alpha = TestDir::new("workspace-alpha");
        let beta = TestDir::new("workspace-beta");

        write_file(&alpha, "main.ts", "export const x = 1;", first_time);
        write_file(&beta, "main.ts", "export const x = 2;", first_time);
        let before = [
            fingerprint(alpha.path()).unwrap(),
            fingerprint(beta.path()).unwrap(),
        ];

        write_file(&alpha, "main.ts", "export const x = 12345;", later_time);
        let after = [
            fingerprint(alpha.path()).unwrap(),
            fingerprint(beta.path()).unwrap(),
        ];

        assert_ne!(before[0], after[0]);
        assert_eq!(before[1], after[1]);
        assert_eq!(
            changed_scope_indices(
                &[Some(before[0].clone()), Some(before[1].clone())],
                &[after[0].clone(), after[1].clone()],
            ),
            vec![0]
        );
    }

    #[test]
    fn first_poll_is_not_reported_as_a_change() {
        let current = vec!["alpha".to_owned(), "beta".to_owned()];
        assert!(changed_scope_indices(&[None, None], &current).is_empty());
        assert_eq!(
            changed_scope_indices(&[Some("old".to_owned()), None], &current),
            vec![0]
        );
    }
}
