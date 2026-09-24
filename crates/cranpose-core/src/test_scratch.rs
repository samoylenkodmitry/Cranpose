//! Where a test writes real files.
//!
//! Enabled by the `test-helpers` feature. Every crate in the workspace that
//! needs a file on disk during a test asks here, so there is one answer to
//! where those files go rather than one per crate that drifts from the rest.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

/// A unique, empty directory under the workspace `target/test-output`.
///
/// Never the system temporary directory: on Linux that is tmpfs, so a test
/// writing a payload there writes it to RAM, and a failure leaves nothing
/// under `target` to look at afterwards.
/// `apps/desktop-demo/tests/source_hygiene_aliases.rs` enforces that across
/// the workspace.
///
/// `manifest_dir` is the caller's own `env!("CARGO_MANIFEST_DIR")`. Its last
/// component names the subdirectory, so two crates asking for the same `tag`
/// get different directories; the workspace root is found by walking up to
/// the directory holding `Cargo.lock` rather than by counting `..` hops,
/// which differ per crate and are wrong the moment one moves.
pub fn test_scratch_dir(manifest_dir: &str, tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let manifest = PathBuf::from(manifest_dir);
    let owner = manifest.file_name().map_or_else(
        || "workspace".to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let path = workspace_root(&manifest)
        .join("target/test-output")
        .join(owner)
        .join(format!("{tag}-{}-{unique}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch directory under target/test-output");
    path
}

fn workspace_root(manifest: &Path) -> PathBuf {
    manifest
        .ancestors()
        .find(|directory| directory.join("Cargo.lock").is_file())
        .unwrap_or(manifest)
        .to_path_buf()
}

#[cfg(test)]
#[path = "tests/test_scratch_tests.rs"]
mod tests;
