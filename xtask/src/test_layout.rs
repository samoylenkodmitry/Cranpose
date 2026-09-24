use std::path::{Path, PathBuf};

pub(crate) fn run_at(root: &Path) -> Result<(), String> {
    let unlinked = unlinked_integration_tests(root)?;
    if unlinked.is_empty() {
        println!("test-layout: every integration test file is linked");
        return Ok(());
    }
    let listed: Vec<String> = unlinked
        .iter()
        .map(|path| format!("  {}", path.display()))
        .collect();
    Err(format!(
        "test-layout: these test files are compiled by nothing; add them as modules of their \
         crate's tests/integration.rs:\n{}",
        listed.join("\n")
    ))
}

pub(crate) fn unlinked_integration_tests(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut unlinked = Vec::new();
    for member in workspace_members(root)? {
        let crate_dir = root.join(&member);
        if !opts_out_of_autotests(&crate_dir)? {
            continue;
        }
        let integration = crate_dir.join("tests").join("integration.rs");
        let Ok(source) = std::fs::read_to_string(&integration) else {
            continue;
        };
        let declared = declared_modules(&source);
        let entries = std::fs::read_dir(crate_dir.join("tests"))
            .map_err(|error| format!("read {}: {error}", crate_dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let is_test_file = path.extension().is_some_and(|extension| extension == "rs");
            if is_test_file && stem != "integration" && !declared.iter().any(|name| name == stem) {
                unlinked.push(path);
            }
        }
    }
    unlinked.sort();
    Ok(unlinked)
}

fn workspace_members(root: &Path) -> Result<Vec<String>, String> {
    let manifest = read_toml(&root.join("Cargo.toml"))?;
    let members = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .ok_or("the root manifest lists no workspace members")?;
    Ok(members
        .iter()
        .filter_map(toml::Value::as_str)
        .map(str::to_owned)
        .collect())
}

fn opts_out_of_autotests(crate_dir: &Path) -> Result<bool, String> {
    let manifest = read_toml(&crate_dir.join("Cargo.toml"))?;
    Ok(manifest
        .get("package")
        .and_then(|package| package.get("autotests"))
        .and_then(toml::Value::as_bool)
        == Some(false))
}

fn read_toml(path: &Path) -> Result<toml::Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    toml::from_str(&text).map_err(|error| format!("parse {}: {error}", path.display()))
}

fn declared_modules(source: &str) -> Vec<String> {
    source
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            line.strip_prefix("pub mod ")
                .or_else(|| line.strip_prefix("mod "))
                .and_then(|rest| rest.strip_suffix(';'))
                .map(str::to_owned)
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/test_layout_tests.rs"]
mod tests;
