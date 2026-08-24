//! Records the commit the demo was built from.
//!
//! The Source view fetches each tab's file from GitHub at this ref rather than
//! at `main`, so what a reader sees is the code that is actually running. A
//! build outside a git checkout, or one that wants to pin a published tag,
//! sets `CRANPOSE_SOURCE_REF` instead.

use std::{path::Path, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=CRANPOSE_SOURCE_REF");

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    if let Some(head) = workspace.as_deref().map(|root| root.join(".git/HEAD")) {
        if head.exists() {
            println!("cargo:rerun-if-changed={}", head.display());
        }
    }

    let reference = std::env::var("CRANPOSE_SOURCE_REF")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| head_commit(workspace.as_deref()))
        // A tarball with no git and no override: the default branch is the only
        // honest guess left, and the view says so rather than implying a match.
        .unwrap_or_else(|| String::from("main"));

    println!("cargo:rustc-env=CRANPOSE_SOURCE_REF={reference}");
}

fn head_commit(workspace: Option<&Path>) -> Option<String> {
    let workspace = workspace?;
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let commit = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!commit.is_empty()).then_some(commit)
}
