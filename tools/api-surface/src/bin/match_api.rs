//! Joins a `dump-compose-api` JSON file and a `dump-cranpose-api` JSON file
//! on a case- and separator-insensitive name key, as a first-pass,
//! reviewable candidate correspondence -- not a verdict. See
//! `docs/compose_api_parity.md` for how the curated verdicts sit on top of
//! this generated join.
use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;

/// Collapses a name to lowercase alphanumerics only, so `fillMaxSize`,
/// `fill_max_size`, and `FillMaxSize` compare equal. This is a coarse
/// heuristic: two unrelated names that happen to squash to the same key
/// will collide, which is why every row this produces is a candidate for
/// human review, not a verdict.
pub fn squash(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The identifier after the last `::`, for a Cranpose ident that may be
/// qualified as `Type::method`.
pub fn leaf_name(ident: &str) -> &str {
    ident.rsplit("::").next().unwrap_or(ident)
}

#[derive(Serialize, Clone)]
struct ComposeCandidate {
    module: String,
    package: String,
    class: String,
    class_kind: String,
    member_kind: String,
    name: String,
    raw: String,
}

#[derive(Serialize)]
struct MatchRow {
    crate_name: String,
    module_path: String,
    ident: String,
    kind: String,
    composable: bool,
    signature: String,
    compose_candidates: Vec<ComposeCandidate>,
}

fn load_compose_index(
    path: &std::path::Path,
) -> Result<(HashMap<String, Vec<ComposeCandidate>>, usize)> {
    let raw: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let modules = raw
        .get("modules")
        .and_then(Value::as_object)
        .context("missing `modules`")?;
    let mut index: HashMap<String, Vec<ComposeCandidate>> = HashMap::new();
    let mut total = 0usize;
    for (module, entries) in modules {
        let entries = entries.as_array().context("module entries not an array")?;
        for e in entries {
            total += 1;
            let name = e.get("name").and_then(Value::as_str).unwrap_or_default();
            let key = squash(name);
            index.entry(key).or_default().push(ComposeCandidate {
                module: module.clone(),
                package: e
                    .get("package")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                class: e
                    .get("class")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                class_kind: e
                    .get("class_kind")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                member_kind: e
                    .get("member_kind")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                name: name.to_string(),
                raw: e
                    .get("raw")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }
    Ok((index, total))
}

struct CranposeFlatItem {
    crate_name: String,
    module_path: String,
    ident: String,
    kind: String,
    composable: bool,
    signature: String,
}

fn load_cranpose_flat(path: &std::path::Path) -> Result<Vec<CranposeFlatItem>> {
    let raw: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let crates = raw.as_array().context("expected a JSON array of crates")?;
    let mut out = vec![];
    for c in crates {
        let crate_name = c
            .get("crate_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let items = c
            .get("items")
            .and_then(Value::as_array)
            .context("crate missing `items`")?;
        for it in items {
            let module_path = it
                .get("module_path")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let ident = it
                .get("ident")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let kind = it
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let composable = it
                .get("composable")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let signature = it
                .get("signature")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            out.push(CranposeFlatItem {
                crate_name: crate_name.clone(),
                module_path: module_path.clone(),
                ident,
                kind: kind.clone(),
                composable,
                signature,
            });
            if let Some(members) = it.get("members").and_then(Value::as_array) {
                for m in members {
                    let member_name = m.get("name").and_then(Value::as_str).unwrap_or_default();
                    let member_sig = m
                        .get("signature")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let parent_ident = it.get("ident").and_then(Value::as_str).unwrap_or_default();
                    out.push(CranposeFlatItem {
                        crate_name: crate_name.clone(),
                        module_path: format!("{module_path}::{parent_ident}"),
                        ident: format!("{parent_ident}::{member_name}"),
                        kind: format!("{kind}_member"),
                        composable: false,
                        signature: member_sig,
                    });
                }
            }
        }
    }
    Ok(out)
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut compose_path: Option<PathBuf> = None;
    let mut cranpose_path: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--compose" => {
                compose_path = Some(PathBuf::from(
                    args.next().context("--compose needs a value")?,
                ))
            }
            "--cranpose" => {
                cranpose_path = Some(PathBuf::from(
                    args.next().context("--cranpose needs a value")?,
                ))
            }
            "--out" => out_path = Some(PathBuf::from(args.next().context("--out needs a value")?)),
            other => bail!("unknown argument: {other}"),
        }
    }
    let compose_path = compose_path.context("--compose is required")?;
    let cranpose_path = cranpose_path.context("--cranpose is required")?;
    let out_path = out_path.context("--out is required")?;

    let (compose_index, compose_total) = load_compose_index(&compose_path)?;
    let cranpose_items = load_cranpose_flat(&cranpose_path)?;

    let mut matched = 0usize;
    let mut rows = vec![];
    let mut matched_compose_keys: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for it in &cranpose_items {
        let key = squash(leaf_name(&it.ident));
        let candidates = compose_index.get(&key).cloned().unwrap_or_default();
        if !candidates.is_empty() {
            matched += 1;
            matched_compose_keys.insert(key);
        }
        rows.push(MatchRow {
            crate_name: it.crate_name.clone(),
            module_path: it.module_path.clone(),
            ident: it.ident.clone(),
            kind: it.kind.clone(),
            composable: it.composable,
            signature: it.signature.clone(),
            compose_candidates: candidates,
        });
    }
    let compose_entries_matched: usize = matched_compose_keys
        .iter()
        .map(|k| compose_index.get(k).map(Vec::len).unwrap_or(0))
        .sum();

    std::fs::write(&out_path, serde_json::to_string_pretty(&rows)?)?;
    println!(
        "cranpose items: {} matched={} unmatched={}",
        cranpose_items.len(),
        matched,
        cranpose_items.len() - matched
    );
    println!(
        "compose entries considered: {compose_total}, distinct keys: {}",
        compose_index.len()
    );
    println!(
        "compose entries with >=1 cranpose match by key: {compose_entries_matched} / {compose_total}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squash_collapses_case_and_separators() {
        assert_eq!(squash("fillMaxSize"), squash("fill_max_size"));
        assert_eq!(squash("FillMaxSize"), squash("fill_max_size"));
    }

    #[test]
    fn squash_distinguishes_different_words() {
        assert_ne!(squash("padding"), squash("paddingValues"));
    }

    #[test]
    fn leaf_name_strips_qualifier() {
        assert_eq!(leaf_name("Modifier::padding"), "padding");
        assert_eq!(leaf_name("padding"), "padding");
    }
}
