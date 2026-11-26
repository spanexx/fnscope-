//! Manifest persistence with validation on both read and write.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fnscope_model::FnGraph;

/// Name of the graph document inside the output directory.
const MANIFEST_FILE: &str = "fn-graph.json";

/// Writes a validated graph as pretty JSON and returns the file it created.
pub fn write_manifest(graph: &FnGraph, out_dir: &Path) -> Result<PathBuf> {
    graph.validate().map_err(anyhow::Error::msg)?;
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("creating output directory {}", out_dir.display()))?;

    let path = out_dir.join(MANIFEST_FILE);
    let json = serde_json::to_string_pretty(graph).context("serializing function graph")?;
    std::fs::write(&path, json).with_context(|| format!("writing manifest {}", path.display()))?;
    Ok(path)
}

/// Reads, parses and validates a manifest file with a descriptive error.
pub fn read_manifest(path: &Path) -> Result<FnGraph> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading manifest {}", path.display()))?;
    let graph: FnGraph = serde_json::from_str(&text)
        .with_context(|| format!("parsing manifest {}", path.display()))?;
    graph.validate().map_err(anyhow::Error::msg)?;
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use fnscope_model::FnRecord;

    use super::*;

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    /// Removes a uniquely named scratch directory even when a test fails.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock is after epoch")
                .as_nanos();
            let name = format!(
                "fnscope-core-{}-{}-{}",
                std::process::id(),
                nanos,
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            );
            let path = std::env::temp_dir().join(name);
            std::fs::create_dir_all(&path).expect("create scratch directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sample_graph() -> FnGraph {
        FnGraph {
            tool: "fnscope".to_owned(),
            schema_version: "1.0.0".to_owned(),
            generated_at: "2026-11-25T00:00:00Z".to_owned(),
            root_dir: "src".to_owned(),
            functions: vec![FnRecord {
                id: "example".to_owned(),
                name: "example".to_owned(),
                module: "src".to_owned(),
                file_path: "src/example.ts".to_owned(),
                line: 3,
                ast_hash: format!("sha256:{}", "a".repeat(64)),
                ..FnRecord::default()
            }],
        }
    }

    #[test]
    fn manifest_round_trips_through_disk() {
        let temp = TempDir::new();
        let out_dir = temp.path().join("nested").join(".fnscope");
        let graph = sample_graph();

        let path = write_manifest(&graph, &out_dir).expect("manifest must write");
        assert_eq!(path, out_dir.join(MANIFEST_FILE));
        assert!(path.is_file());

        let decoded = read_manifest(&path).expect("manifest must read");
        assert_eq!(decoded, graph);
    }

    #[test]
    fn write_manifest_rejects_invalid_graphs() {
        let temp = TempDir::new();
        let mut graph = sample_graph();
        graph.tool = "foreign-tool".to_owned();

        assert!(write_manifest(&graph, temp.path()).is_err());
        assert!(!temp.path().join(MANIFEST_FILE).exists());
    }

    #[test]
    fn read_manifest_reports_bad_json_and_invalid_graphs() {
        let temp = TempDir::new();
        let malformed = temp.path().join("malformed.json");
        std::fs::write(&malformed, "{ not json").expect("write malformed file");
        assert!(read_manifest(&malformed).is_err());

        let mut graph = sample_graph();
        graph.functions[0].ast_hash = "md5:nope".to_owned();
        let path = write_manifest(&graph, temp.path());
        assert!(path.is_err(), "invalid graphs must never be written");
    }
}
