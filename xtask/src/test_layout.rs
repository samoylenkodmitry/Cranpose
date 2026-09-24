use std::path::{Path, PathBuf};

const ROBOT_RUNNERS: &str = "apps/desktop-demo/robot-runners";

pub(crate) fn run_at(root: &Path) -> Result<(), String> {
    let mut problems: Vec<String> = unlinked_integration_tests(root)?
        .iter()
        .map(|path| {
            format!(
                "  {} is not a module of its crate's tests/integration.rs",
                path.display()
            )
        })
        .collect();
    problems.extend(unlisted_robot_runners(root)?.iter().map(|path| {
        format!(
            "  {} is not in the runners! table of {ROBOT_RUNNERS}/main.rs",
            path.display()
        )
    }));
    let mut sections = Vec::new();
    if !problems.is_empty() {
        sections.push(format!(
            "these tests are compiled by nothing:\n{}",
            problems.join("\n")
        ));
    }
    let inline: Vec<String> = inline_test_modules(root)?
        .iter()
        .map(|(path, line)| format!("  {}:{line}", path.display()))
        .collect();
    if !inline.is_empty() {
        sections.push(format!(
            "these test modules are inline; move each into a tests/ folder beside its file \
             with scripts/dev/move_inline_tests.py:\n{}",
            inline.join("\n")
        ));
    }
    if sections.is_empty() {
        println!(
            "test-layout: every integration test file and robot runner is linked, and no test \
             module is inline"
        );
        return Ok(());
    }
    Err(format!("test-layout: {}", sections.join("\ntest-layout: ")))
}

pub(crate) fn inline_test_modules(root: &Path) -> Result<Vec<(PathBuf, usize)>, String> {
    let mut inline = Vec::new();
    for member in workspace_members(root)? {
        let mut sources = Vec::new();
        collect_sources(&root.join(&member).join("src"), &mut sources);
        for path in sources {
            let source = std::fs::read_to_string(&path)
                .map_err(|error| format!("read {}: {error}", path.display()))?;
            inline.extend(
                inline_test_module_lines(&source)
                    .into_iter()
                    .map(|line| (path.clone(), line)),
            );
        }
    }
    inline.sort();
    Ok(inline)
}

fn collect_sources(dir: &Path, sources: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|name| name != "tests" && name != "test")
            {
                collect_sources(&path, sources);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push(path);
        }
    }
}

fn inline_test_module_lines(source: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut attributes = String::new();
    let mut depth = 0_i64;
    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if depth > 0 || trimmed.starts_with("#[") {
            attributes.push_str(trimmed);
            attributes.push(' ');
            depth += bracket_balance(trimmed);
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if opens_inline_module(trimmed) && gated_on_test(&attributes) {
            found.push(index + 1);
        }
        attributes.clear();
    }
    found
}

fn bracket_balance(line: &str) -> i64 {
    line.chars().fold(0, |balance, ch| match ch {
        '[' => balance + 1,
        ']' => balance - 1,
        _ => balance,
    })
}

fn opens_inline_module(line: &str) -> bool {
    let item = ["pub(crate) ", "pub(super) ", "pub "]
        .iter()
        .find_map(|visibility| line.strip_prefix(visibility))
        .unwrap_or(line);
    item.starts_with("mod ") && item.ends_with('{')
}

fn gated_on_test(attributes: &str) -> bool {
    let compact: String = attributes
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect();
    compact.split("#[").any(|attribute| {
        attribute
            .strip_prefix("cfg(")
            .and_then(|rest| rest.strip_suffix(")]"))
            .is_some_and(|predicate| {
                predicate == "test"
                    || predicate
                        .strip_prefix("all(")
                        .and_then(|rest| rest.strip_suffix(')'))
                        .is_some_and(|arguments| top_level_arguments(arguments).contains(&"test"))
            })
    })
}

fn top_level_arguments(list: &str) -> Vec<&str> {
    let mut arguments = Vec::new();
    let mut depth = 0_i64;
    let mut start = 0;
    for (index, ch) in list.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                arguments.push(&list[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    arguments.push(&list[start..]);
    arguments
}

pub(crate) fn unlisted_robot_runners(root: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = root.join(ROBOT_RUNNERS);
    let Ok(table) = std::fs::read_to_string(dir.join("main.rs")) else {
        return Ok(Vec::new());
    };
    let listed = runner_table(&table);
    let entries =
        std::fs::read_dir(&dir).map_err(|error| format!("read {}: {error}", dir.display()))?;
    let mut unlisted = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !stem.starts_with("robot_") || path.extension().is_none_or(|extension| extension != "rs")
        {
            continue;
        }
        let source = std::fs::read_to_string(&path)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        let is_runner = source
            .lines()
            .any(|line| line.starts_with("pub(crate) fn main("));
        if is_runner && !listed.iter().any(|name| name == stem) {
            unlisted.push(path);
        }
    }
    unlisted.sort();
    Ok(unlisted)
}

fn runner_table(source: &str) -> Vec<String> {
    source
        .split_once("runners! {")
        .and_then(|(_, rest)| rest.split_once('}'))
        .map(|(body, _)| {
            body.split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
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

pub(crate) fn workspace_members(root: &Path) -> Result<Vec<String>, String> {
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
