//! Puts an application's usage descriptions into its `Info.plist`.
//!
//! ```text
//! cranpose-plist --usage target/cranpose/my-app-usage.plist --into build/Info.plist
//! ```
//!
//! The usage file is what a build script wrote from the application's
//! declaration. Each top-level key it holds, with its string, array or
//! dictionary, replaces the one in the property list, or is added before the
//! closing tag when the list has none.

use std::{env, fs, ops::Range, process::ExitCode};

fn main() -> ExitCode {
    let mut usage = None;
    let mut into = None;
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--usage" => usage = arguments.next(),
            "--into" => into = arguments.next(),
            other => {
                eprintln!("cranpose-plist: {other} is not an argument it takes");
                return ExitCode::FAILURE;
            }
        }
    }

    let (Some(usage), Some(into)) = (usage, into) else {
        eprintln!("cranpose-plist --usage <file> --into <Info.plist>");
        return ExitCode::FAILURE;
    };

    let usage_text = match fs::read_to_string(&usage) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cranpose-plist: reading {usage}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let plist = match fs::read_to_string(&into) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cranpose-plist: reading {into}: {error}");
            return ExitCode::FAILURE;
        }
    };

    let merged = merge(&plist, &entries(&usage_text));
    if let Err(error) = fs::write(&into, merged) {
        eprintln!("cranpose-plist: writing {into}: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Each top-level key of a property list with its value element as written:
/// a string, or a whole array or dictionary.
fn entries(plist: &str) -> Vec<(&str, &str)> {
    entry_spans(plist)
        .into_iter()
        .map(|(key, value)| (&plist[key], &plist[value]))
        .collect()
}

/// The spans of each top-level key name and of its value element.
fn entry_spans(plist: &str) -> Vec<(Range<usize>, Range<usize>)> {
    let mut found = Vec::new();
    let Some(root) = plist.find("<dict>") else {
        return found;
    };
    let mut at = root + "<dict>".len();
    while let Some((key, value)) = next_entry(plist, at) {
        at = value.end;
        found.push((key, value));
    }
    found
}

/// The spans of the key name and of the value element of the entry that
/// starts after `from`, while it is still inside the dictionary.
fn next_entry(plist: &str, from: usize) -> Option<(Range<usize>, Range<usize>)> {
    let rest = plist.get(from..)?;
    let open = rest.find('<')?;
    if !rest[open..].starts_with("<key>") {
        return None;
    }
    let name_start = from + open + "<key>".len();
    let name_end = name_start + plist[name_start..].find("</key>")?;
    let after_key = name_end + "</key>".len();
    let value_start = after_key + plist[after_key..].find('<')?;
    let value_end = element_end(plist, value_start)?;
    Some((name_start..name_end, value_start..value_end))
}

/// Where the element that opens at `start` ends, past nested elements of
/// its own name.
fn element_end(plist: &str, start: usize) -> Option<usize> {
    let tag_end = start + plist[start..].find('>')? + 1;
    let tag = &plist[start..tag_end];
    if tag.ends_with("/>") {
        return Some(tag_end);
    }
    let name = tag.trim_start_matches('<').trim_end_matches('>');
    let (opening, closing) = (format!("<{name}>"), format!("</{name}>"));
    let mut depth = 1;
    let mut at = tag_end;
    while depth > 0 {
        let close = at + plist[at..].find(&closing)?;
        let nested = plist[at..close].matches(&opening).count();
        depth += nested;
        depth -= 1;
        at = close + closing.len();
    }
    Some(at)
}

/// Writes each entry into the property list, in place of the value of the
/// key it already holds or before the closing tag.
fn merge(plist: &str, entries: &[(&str, &str)]) -> String {
    let mut text = plist.to_string();
    for (key, value) in entries {
        let held = entry_spans(&text)
            .into_iter()
            .find(|(name, _)| text[name.clone()] == **key);
        text = match held {
            Some((_, span)) => format!("{}{value}{}", &text[..span.start], &text[span.end..]),
            None => add_entry(&text, key, value),
        };
    }
    text
}

fn add_entry(text: &str, key: &str, value: &str) -> String {
    let Some(at) = text.rfind("</dict>") else {
        return text.to_string();
    };
    format!(
        "{}\t<key>{key}</key>\n\t{value}\n{}",
        &text[..at],
        &text[at..]
    )
}
