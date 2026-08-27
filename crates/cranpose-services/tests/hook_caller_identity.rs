//! Every service hook keys its composition state by the application's call
//! site, not by its own source line.
//!
//! `rememberEventStream` takes its identity from `#[track_caller]`. A wrapper
//! that omits the attribute hands the framework its own line instead, so two
//! calls to that wrapper in one composition collapse onto a single key and
//! fall back to sibling position — the fragility the source-keyed hooks exist
//! to remove. Every wrapper in this crate carries it; this keeps the next one
//! from landing without it.

use std::{fs, path::PathBuf};

/// Returns the wrappers that call `rememberEventStream` without declaring
/// `#[track_caller]` on the enclosing `pub fn`.
fn wrappers_missing_track_caller() -> Vec<String> {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();

    let entries = fs::read_dir(&src).unwrap_or_else(|err| panic!("read {src:?}: {err}"));
    for entry in entries {
        let path = entry.expect("directory entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {path:?}: {err}"));
        if !source.contains("rememberEventStream(") {
            continue;
        }

        // Walk each `pub fn`, and report the ones whose body reaches
        // rememberEventStream while their attributes omit #[track_caller].
        let lines: Vec<&str> = source.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if !line.trim_start().starts_with("pub fn ") {
                continue;
            }
            let body_end = lines[index + 1..]
                .iter()
                .position(|candidate| candidate.starts_with("pub fn ") || *candidate == "}")
                .map_or(lines.len(), |offset| index + 1 + offset);
            let body = lines[index..body_end].join("\n");
            if !body.contains("rememberEventStream(") {
                continue;
            }

            let attributes = lines[..index]
                .iter()
                .rev()
                .take_while(|candidate| {
                    let trimmed = candidate.trim_start();
                    trimmed.starts_with('#') || trimmed.starts_with("///") || trimmed.is_empty()
                })
                .any(|candidate| candidate.contains("track_caller"));
            if !attributes {
                let name = line.trim_start().trim_start_matches("pub fn ");
                let name = name.split(['(', '<']).next().unwrap_or(name);
                let file = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("?");
                offenders.push(format!("{file}: {name}"));
            }
        }
    }
    offenders.sort();
    offenders
}

#[test]
fn every_event_stream_wrapper_keys_by_its_caller() {
    let offenders = wrappers_missing_track_caller();
    assert!(
        offenders.is_empty(),
        "these wrappers call rememberEventStream without #[track_caller], so they key \
         composition state by their own source line instead of the application's: {offenders:#?}"
    );
}

#[test]
fn the_scan_actually_finds_wrappers_to_check() {
    // A scan that matched nothing would pass the test above forever.
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut wrappers = 0usize;
    for entry in fs::read_dir(&src).expect("read src") {
        let path = entry.expect("directory entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("read source");
        wrappers += source.matches("rememberEventStream(").count();
    }
    assert!(
        wrappers >= 5,
        "the wrapper scan found only {wrappers} call sites; the pattern it greps for has moved"
    );
}
