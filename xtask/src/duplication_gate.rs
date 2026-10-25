use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
    sync::LazyLock,
};

use regex::Regex;
use serde_json::Value;

use super::{gate_diff, gate_diff::Span};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DuplicateSide {
    pub(crate) name: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Duplicate {
    pub(crate) first_file: DuplicateSide,
    pub(crate) second_file: DuplicateSide,
    pub(crate) lines: usize,
    pub(crate) fragment: String,
}

pub(crate) struct DuplicationConfig {
    pub(crate) min_lines: u64,
    pub(crate) min_tokens: u64,
    pub(crate) ignore_globs: Vec<String>,
}

pub(crate) fn load_duplication_config(config_path: &Path) -> Result<DuplicationConfig, String> {
    let config = super::load_toml(config_path)?;
    let table = config
        .get("duplication")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("{}: missing [duplication] table", config_path.display()))?;
    let min_lines = table
        .get("min_lines")
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| format!("{}: missing duplication.min_lines", config_path.display()))?
        as u64;
    let min_tokens = table
        .get("min_tokens")
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| format!("{}: missing duplication.min_tokens", config_path.display()))?
        as u64;
    let ignore_globs = table
        .get("ignore_globs")
        .and_then(toml::Value::as_array)
        .map(|globs| {
            globs
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok(DuplicationConfig {
        min_lines,
        min_tokens,
        ignore_globs,
    })
}

fn duplicate_side_from_json(value: &Value) -> Option<DuplicateSide> {
    Some(DuplicateSide {
        name: value.get("name")?.as_str()?.to_owned(),
        start: value.get("start")?.as_u64()? as usize,
        end: value.get("end")?.as_u64()? as usize,
    })
}

fn duplicate_from_json(value: &Value) -> Option<Duplicate> {
    Some(Duplicate {
        first_file: duplicate_side_from_json(value.get("firstFile")?)?,
        second_file: duplicate_side_from_json(value.get("secondFile")?)?,
        lines: value.get("lines")?.as_u64()? as usize,
        fragment: value
            .get("fragment")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_default(),
    })
}

pub(crate) fn parse_duplicates(report: &Value) -> Vec<Duplicate> {
    report
        .get("duplicates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(duplicate_from_json)
        .collect()
}

pub(crate) fn run_jscpd(
    jscpd: &Path,
    min_lines: u64,
    min_tokens: u64,
    ignore_globs: &[String],
    out_dir: &Path,
    cwd: &Path,
) -> Result<Vec<Duplicate>, String> {
    let mut command = Command::new(jscpd);
    command
        .arg("--min-lines")
        .arg(min_lines.to_string())
        .arg("--min-tokens")
        .arg(min_tokens.to_string())
        .arg("-f")
        .arg("rust")
        .arg("-r")
        .arg("json")
        .arg("-o")
        .arg(out_dir)
        .current_dir(cwd);
    if !ignore_globs.is_empty() {
        command.arg("--ignore").arg(ignore_globs.join(","));
    }
    command.arg(".");
    let output = command
        .output()
        .map_err(|error| format!("duplication-gate: failed to run jscpd: {error}"))?;

    let report_path = out_dir.join("jscpd-report.json");
    if !report_path.exists() {
        return Err(format!(
            "duplication-gate: jscpd produced no report\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = fs::read_to_string(&report_path)
        .map_err(|error| format!("duplication-gate: failed to read jscpd report: {error}"))?;
    let report: Value = serde_json::from_str(&text)
        .map_err(|error| format!("duplication-gate: could not parse jscpd report: {error}"))?;
    Ok(parse_duplicates(&report))
}

pub(crate) fn side_span(side: &DuplicateSide) -> (&str, Span) {
    (&side.name, (side.start, side.end))
}

pub(crate) fn file_pair(dup: &Duplicate) -> (String, String) {
    let mut names = [dup.first_file.name.clone(), dup.second_file.name.clone()];
    names.sort();
    let [a, b] = names;
    (a, b)
}

pub(crate) fn touches_diff(dup: &Duplicate, ranges: &gate_diff::RangesByFile) -> bool {
    let (first_file, first_span) = side_span(&dup.first_file);
    let (second_file, second_span) = side_span(&dup.second_file);
    gate_diff::any_intersect(
        ranges
            .get(first_file)
            .map(Vec::as_slice)
            .unwrap_or_default(),
        first_span,
    ) || gate_diff::any_intersect(
        ranges
            .get(second_file)
            .map(Vec::as_slice)
            .unwrap_or_default(),
        second_span,
    )
}

/// Code with its comment lines, all its whitespace and its trailing
/// commas dropped, so code that moved out of a module, reformatted one
/// level shallower and without its comments, still reads the same.
/// rustfmt drops a trailing comma when the shallower indent lets it join
/// a list onto one line, and a clone can end on one whose list closes
/// past the clone.
pub(crate) fn clone_text(source: &str) -> String {
    static MODULE_PATH: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\b(?:[a-z_][a-z0-9_]*::)+").expect("the module path pattern is valid")
    });
    let mut joined = String::with_capacity(source.len());
    for line in source.lines().map(str::trim) {
        if line.starts_with("//") {
            continue;
        }
        for word in MODULE_PATH.replace_all(line, "").split_whitespace() {
            joined.push_str(word);
        }
    }
    let mut text = String::with_capacity(joined.len());
    let mut chars = joined.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == ',' && matches!(chars.peek(), None | Some(')' | ']' | '}' | '>')) {
            continue;
        }
        text.push(ch);
    }
    text
}

/// A clone whose text was in the tree before the diff, at any path and with or without
/// module paths: code that moved, or only lost its qualifications, brings its old clones
/// along without introducing them.
pub(crate) fn text_existed_before(candidate: &Duplicate, old_sources: &[String]) -> bool {
    let text = clone_text(&candidate.fragment);
    !text.is_empty() && old_sources.iter().any(|source| source.contains(&text))
}

pub(crate) fn already_duplicated_before(
    candidate: &Duplicate,
    old_duplicates: &[Duplicate],
) -> bool {
    let candidate_pair = file_pair(candidate);
    for old_dup in old_duplicates {
        if file_pair(old_dup) != candidate_pair {
            continue;
        }
        if old_dup.lines == 0 || candidate.lines == 0 {
            continue;
        }
        let ratio = old_dup.lines as f64 / candidate.lines as f64;
        if (0.5..=2.0).contains(&ratio) {
            return true;
        }
    }
    false
}

pub(crate) fn find_violations(
    new_duplicates: &[Duplicate],
    old_duplicates: &[Duplicate],
    old_sources: &[String],
    ranges: &gate_diff::RangesByFile,
) -> Vec<String> {
    let mut old_by_pair: BTreeMap<(String, String), Vec<Duplicate>> = BTreeMap::new();
    for old_dup in old_duplicates {
        old_by_pair
            .entry(file_pair(old_dup))
            .or_default()
            .push(old_dup.clone());
    }

    let mut violations = Vec::new();
    for dup in new_duplicates {
        if !touches_diff(dup, ranges) {
            continue;
        }
        let comparable = old_by_pair
            .get(&file_pair(dup))
            .map(Vec::as_slice)
            .unwrap_or_default();
        if already_duplicated_before(dup, comparable) || text_existed_before(dup, old_sources) {
            continue;
        }
        let (first_file, first_span) = side_span(&dup.first_file);
        let (second_file, second_span) = side_span(&dup.second_file);
        let first_hit = gate_diff::any_intersect(
            ranges
                .get(first_file)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            first_span,
        );
        let second_hit = gate_diff::any_intersect(
            ranges
                .get(second_file)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            second_span,
        );
        violations.push(format!(
            "{first_file}:{}-{}{} duplicates {second_file}:{}-{}{} ({} lines, introduced by this diff)",
            first_span.0,
            first_span.1,
            if first_hit { " (new)" } else { "" },
            second_span.0,
            second_span.1,
            if second_hit { " (new)" } else { "" },
            dup.lines,
        ));
    }
    violations
}

const JSCPD_INSTALL_HINT: &str = "jscpd also ships an npm package that wraps a prebuilt copy \
    of the same Rust binary, but this project does not require Node.js and will not start \
    requiring it here -- if `cargo install jscpd --locked` cannot run, fix that (network, \
    registry, offline mirror), do not swap in `npm install -g jscpd`.";

pub(crate) fn involved_files(candidates: &[Duplicate]) -> Vec<String> {
    let mut files: BTreeSet<String> = BTreeSet::new();
    for dup in candidates {
        files.insert(dup.first_file.name.clone());
        files.insert(dup.second_file.name.clone());
    }
    files.into_iter().collect()
}

fn old_duplicates_for(
    root: &Path,
    base_sha: &str,
    involved_files: &[String],
    tmp_dir: &Path,
    jscpd: &Path,
    config: &DuplicationConfig,
) -> Result<(Vec<Duplicate>, Vec<String>), String> {
    if involved_files.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let old_src = tmp_dir.join("old-src");
    fs::create_dir(&old_src).map_err(|error| {
        format!(
            "duplication-gate: failed to create `{}`: {error}",
            old_src.display()
        )
    })?;
    let old_files = gate_diff::write_old_blobs(root, base_sha, involved_files, &old_src)?;
    if old_files.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let old_sources = old_files
        .iter()
        .map(|file| {
            fs::read_to_string(old_src.join(file))
                .map(|text| clone_text(&text))
                .map_err(|error| format!("duplication-gate: failed to read `{file}`: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let old_out = tmp_dir.join("old-out");
    fs::create_dir(&old_out).map_err(|error| {
        format!(
            "duplication-gate: failed to create `{}`: {error}",
            old_out.display()
        )
    })?;
    let old_duplicates = run_jscpd(
        jscpd,
        config.min_lines,
        config.min_tokens,
        &config.ignore_globs,
        &old_out,
        &old_src,
    )?;
    Ok((old_duplicates, old_sources))
}

pub(crate) fn run_at(root: &Path, base: &str, config_path: &Path) -> Result<(), String> {
    let config = load_duplication_config(config_path)?;
    let ranges = gate_diff::changed_ranges(root, base, "*.rs")?;
    if ranges.is_empty() {
        println!("duplication-gate: no changed Rust files against {base}");
        return Ok(());
    }

    let jscpd = gate_diff::resolve_cargo_tool(root, "jscpd", JSCPD_INSTALL_HINT)?;
    let base_sha = gate_diff::merge_base(root, base)?;

    let tmp = tempfile::tempdir()
        .map_err(|error| format!("duplication-gate: failed to create a temp dir: {error}"))?;

    let new_out = tmp.path().join("new-out");
    fs::create_dir(&new_out).map_err(|error| {
        format!(
            "duplication-gate: failed to create `{}`: {error}",
            new_out.display()
        )
    })?;
    let duplicates = run_jscpd(
        &jscpd,
        config.min_lines,
        config.min_tokens,
        &config.ignore_globs,
        &new_out,
        root,
    )?;

    let candidates: Vec<Duplicate> = duplicates
        .iter()
        .filter(|dup| touches_diff(dup, &ranges))
        .cloned()
        .collect();
    let mut files_to_check = involved_files(&candidates);
    files_to_check.extend(ranges.keys().cloned());
    files_to_check.sort();
    files_to_check.dedup();
    let (old_duplicates, old_sources) = old_duplicates_for(
        root,
        &base_sha,
        &files_to_check,
        tmp.path(),
        &jscpd,
        &config,
    )?;

    let violations = find_violations(&duplicates, &old_duplicates, &old_sources, &ranges);
    if !violations.is_empty() {
        return Err(super::violations_message(
            format!(
                "duplication-gate: {} clone(s) introduced by this diff:",
                violations.len()
            ),
            &violations,
        ));
    }

    println!(
        "duplication-gate: {} clone(s) in the repo, {} touch the diff, none introduced by it",
        duplicates.len(),
        candidates.len()
    );
    Ok(())
}
