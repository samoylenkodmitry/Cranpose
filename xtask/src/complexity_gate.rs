use std::{collections::BTreeMap, fs, path::Path, process::Command};

use serde_json::Value;

use super::gate_diff;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FunctionMetric {
    pub(crate) name: String,
    pub(crate) start: Option<usize>,
    pub(crate) end: Option<usize>,
    pub(crate) cyclomatic: Option<i64>,
    pub(crate) lines: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Limits {
    pub(crate) max_cyclomatic: i64,
    pub(crate) max_function_lines: i64,
}

#[derive(Clone, Copy)]
enum Measure {
    Cyclomatic,
    Lines,
}

impl Measure {
    const ALL: [Self; 2] = [Self::Cyclomatic, Self::Lines];

    fn of(self, function: &FunctionMetric) -> Option<i64> {
        match self {
            Self::Cyclomatic => function.cyclomatic,
            Self::Lines => function.lines,
        }
    }

    fn limit(self, limits: Limits) -> i64 {
        match self {
            Self::Cyclomatic => limits.max_cyclomatic,
            Self::Lines => limits.max_function_lines,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Cyclomatic => "cyclomatic complexity",
            Self::Lines => "lines of code",
        }
    }
}

const ANONYMOUS: &str = "<anonymous>";

pub(crate) fn load_limits(config_path: &Path) -> Result<Limits, String> {
    let config = super::load_toml(config_path)?;
    let read = |key: &str| {
        config
            .get("complexity")
            .and_then(toml::Value::as_table)
            .and_then(|table| table.get(key))
            .and_then(toml::Value::as_integer)
            .ok_or_else(|| format!("{}: missing complexity.{key}", config_path.display()))
    };
    Ok(Limits {
        max_cyclomatic: read("max_cyclomatic")?,
        max_function_lines: read("max_function_lines")?,
    })
}

pub(crate) fn functions_in(space: &Value, out: &mut Vec<FunctionMetric>, inside_function: bool) {
    let is_function = space.get("kind").and_then(Value::as_str) == Some("function");
    let raw_name = space.get("name").and_then(Value::as_str);
    let is_named = raw_name.is_some_and(|name| !name.is_empty() && name != ANONYMOUS);
    if is_function && (is_named || !inside_function) {
        let metric = |group: &str, name: &str| {
            space
                .get("metrics")
                .and_then(|metrics| metrics.get(group))
                .and_then(|values| values.get(name))
                .and_then(Value::as_f64)
                .map(|value| value as i64)
        };
        let cyclomatic = metric("cyclomatic", "sum");
        let lines = metric("loc", "ploc");
        out.push(FunctionMetric {
            name: if is_named {
                raw_name
                    .expect("is_named implies raw_name is Some")
                    .to_owned()
            } else {
                ANONYMOUS.to_owned()
            },
            start: space
                .get("start_line")
                .and_then(Value::as_u64)
                .map(|v| v as usize),
            end: space
                .get("end_line")
                .and_then(Value::as_u64)
                .map(|v| v as usize),
            cyclomatic,
            lines,
        });
    }
    if let Some(children) = space.get("spaces").and_then(Value::as_array) {
        for child in children {
            functions_in(child, out, inside_function || is_function);
        }
    }
}

fn find_json_files_recursive(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(find_json_files_recursive(&path));
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            found.push(path);
        }
    }
    found
}

pub(crate) fn analyze_files(
    rust_code_analysis_cli: &Path,
    files: &[String],
    out_dir: &Path,
    cwd: &Path,
) -> Result<BTreeMap<String, Vec<FunctionMetric>>, String> {
    let mut command = Command::new(rust_code_analysis_cli);
    command
        .arg("-m")
        .arg("-O")
        .arg("json")
        .arg("-o")
        .arg(out_dir)
        .current_dir(cwd);
    for file in files {
        command.arg("-p").arg(file);
    }
    let output = command.output().map_err(|error| {
        format!("complexity-gate: failed to run rust-code-analysis-cli: {error}")
    })?;

    let produced = find_json_files_recursive(out_dir);
    if !output.status.success() && produced.is_empty() {
        return Err(format!(
            "complexity-gate: rust-code-analysis-cli failed with no output\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        eprintln!("{}", stderr.trim());
    }

    let mut by_file = BTreeMap::new();
    for file in files {
        let json_path = out_dir.join(format!("{file}.json"));
        if !json_path.exists() {
            eprintln!("complexity-gate: no analysis produced for {file}, skipping");
            continue;
        }
        let text = match fs::read_to_string(&json_path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("complexity-gate: could not read analysis for {file}: {error}");
                continue;
            }
        };
        let data: Value = match serde_json::from_str(&text) {
            Ok(data) => data,
            Err(error) => {
                eprintln!("complexity-gate: could not parse analysis for {file}: {error}");
                continue;
            }
        };
        let mut functions = Vec::new();
        functions_in(&data, &mut functions, false);
        by_file.insert(file.clone(), functions);
    }
    Ok(by_file)
}

fn old_values_by_name(functions: &[FunctionMetric], measure: Measure) -> BTreeMap<&str, Vec<i64>> {
    let mut by_name: BTreeMap<&str, Vec<i64>> = BTreeMap::new();
    for func in functions {
        if func.name == ANONYMOUS {
            continue;
        }
        let Some(value) = measure.of(func) else {
            continue;
        };
        by_name.entry(&func.name).or_default().push(value);
    }
    by_name
}

pub(crate) fn find_violations(
    ranges: &gate_diff::RangesByFile,
    new_functions_by_file: &BTreeMap<String, Vec<FunctionMetric>>,
    old_functions_by_file: &BTreeMap<String, Vec<FunctionMetric>>,
    limits: Limits,
) -> Vec<String> {
    let mut violations = Vec::new();
    for (file, functions) in new_functions_by_file {
        let file_ranges = ranges.get(file).map(Vec::as_slice).unwrap_or_default();
        let old_functions = old_functions_by_file
            .get(file)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for measure in Measure::ALL {
            let old_by_name = old_values_by_name(old_functions, measure);
            let limit = measure.limit(limits);
            let mut cursor: BTreeMap<&str, usize> = BTreeMap::new();
            for func in functions {
                let occurrence = cursor.entry(&func.name).or_default();
                let index = *occurrence;
                *occurrence += 1;

                let (Some(start), Some(end), Some(new_value)) =
                    (func.start, func.end, measure.of(func))
                else {
                    continue;
                };
                if !gate_diff::any_intersect(file_ranges, (start, end)) || new_value <= limit {
                    continue;
                }
                let old_value = old_by_name
                    .get(func.name.as_str())
                    .and_then(|values| values.get(index))
                    .copied();
                let reason = match old_value {
                    Some(old) if new_value <= old => continue,
                    Some(old) => format!("was {old}, is now {new_value}"),
                    None => format!("is new at {new_value}"),
                };
                violations.push(format!(
                    "{file}:{start}-{end} {} {} {reason} (limit {limit})",
                    func.name,
                    measure.label()
                ));
            }
        }
    }
    violations
}

fn old_functions_for(
    root: &Path,
    base_sha: &str,
    files: &[String],
    tmp_dir: &Path,
    rust_code_analysis_cli: &Path,
) -> Result<BTreeMap<String, Vec<FunctionMetric>>, String> {
    let old_src = tmp_dir.join("old-src");
    fs::create_dir(&old_src).map_err(|error| {
        format!(
            "complexity-gate: failed to create `{}`: {error}",
            old_src.display()
        )
    })?;
    let old_files = gate_diff::write_old_blobs(root, base_sha, files, &old_src)?;
    if old_files.is_empty() {
        return Ok(BTreeMap::new());
    }
    let old_out = tmp_dir.join("old-out");
    fs::create_dir(&old_out).map_err(|error| {
        format!(
            "complexity-gate: failed to create `{}`: {error}",
            old_out.display()
        )
    })?;
    analyze_files(rust_code_analysis_cli, &old_files, &old_out, &old_src)
}

pub(crate) fn run_at(root: &Path, base: &str, config_path: &Path) -> Result<(), String> {
    let limits = load_limits(config_path)?;
    let ranges = gate_diff::changed_ranges(root, base, "*.rs")?;
    if ranges.is_empty() {
        println!("complexity-gate: no changed Rust files against {base}");
        return Ok(());
    }

    let rust_code_analysis_cli = gate_diff::resolve_cargo_tool(root, "rust-code-analysis-cli", "")?;
    let base_sha = gate_diff::merge_base(root, base)?;

    let tmp = tempfile::tempdir()
        .map_err(|error| format!("complexity-gate: failed to create a temp dir: {error}"))?;

    let new_out = tmp.path().join("new-out");
    fs::create_dir(&new_out).map_err(|error| {
        format!(
            "complexity-gate: failed to create `{}`: {error}",
            new_out.display()
        )
    })?;
    let files: Vec<String> = ranges.keys().cloned().collect();
    let new_functions_by_file = analyze_files(&rust_code_analysis_cli, &files, &new_out, root)?;
    let old_functions_by_file =
        old_functions_for(root, &base_sha, &files, tmp.path(), &rust_code_analysis_cli)?;

    let violations = find_violations(
        &ranges,
        &new_functions_by_file,
        &old_functions_by_file,
        limits,
    );
    if !violations.is_empty() {
        return Err(super::violations_message(
            format!(
                "complexity-gate: {} function(s) over the limit:",
                violations.len()
            ),
            &violations,
        ));
    }

    let total_ranges: usize = ranges.values().map(Vec::len).sum();
    println!("complexity-gate: {total_ranges} changed line range(s) clean");
    Ok(())
}
