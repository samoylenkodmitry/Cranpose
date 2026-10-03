use std::{fmt::Write as _, path::Path, process::Command};

use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};

struct PreviewMetadata {
    id: String,
    entry: Option<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
        .unwrap_or_else(|| String::from("main"));

    println!("cargo:rustc-env=CRANPOSE_SOURCE_REF={reference}");
    let workspace = workspace.ok_or("desktop-demo requires its workspace root")?;
    generate_guide_previews(&workspace)?;
    Ok(())
}

fn generate_guide_previews(workspace: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let guide = workspace.join("docs/guide.md");
    println!("cargo:rerun-if-changed={}", guide.display());
    let markdown = std::fs::read_to_string(guide)?;
    let mut generated = String::new();
    let mut registrations = String::from("static PREVIEWS: &[Preview] = &[\n");
    let mut active = None;
    let mut code = String::new();
    let mut names = std::collections::HashSet::new();
    for event in Parser::new(&markdown) {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                active = preview_metadata(&info)?;
                code.clear();
            }
            Event::Text(text) if active.is_some() => code.push_str(&text),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(PreviewMetadata { id, entry }) = active.take() {
                    if names.contains(&id) {
                        return Err(format!("duplicate guide preview name: {id}").into());
                    }
                    writeln!(generated, "mod {id} {{")?;
                    if let Some(entry) = entry {
                        writeln!(generated, "{code}\npub(super) fn render() {{ {entry}(); }}")?;
                    } else {
                        writeln!(generated, "use cranpose::prelude::*;\n#[composable]\npub(super) fn render() {{\n{code}\n}}")?;
                    }
                    writeln!(generated, "}}")?;
                    writeln!(
                        registrations,
                        "Preview {{ id: {id:?}, source: {code:?}, render: {id}::render }},"
                    )?;
                    names.insert(id);
                }
            }
            _ => {}
        }
    }
    registrations.push_str("];\n");
    generated.push_str(&registrations);
    let destination =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    std::fs::write(destination.join("guide_previews.rs"), generated)?;
    Ok(())
}

fn preview_metadata(info: &str) -> Result<Option<PreviewMetadata>, Box<dyn std::error::Error>> {
    let Some(id) = info
        .split_whitespace()
        .find_map(|part| part.strip_prefix("preview="))
    else {
        return Ok(None);
    };
    if info.split_whitespace().next() != Some("rust") {
        return Err(format!("guide preview {id} requires a Rust fence").into());
    }
    if !id.chars().all(|c| c.is_ascii_lowercase() || c == '_') || id.is_empty() {
        return Err(format!("invalid guide preview name: {id}").into());
    }
    let entry = info
        .split_whitespace()
        .find_map(|part| part.strip_prefix("entry="));
    Ok(Some(PreviewMetadata {
        id: id.to_owned(),
        entry: entry.map(str::to_owned),
    }))
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
