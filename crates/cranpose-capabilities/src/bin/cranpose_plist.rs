//! Puts an application's usage descriptions into its `Info.plist`.
//!
//! ```text
//! cranpose-plist --usage target/cranpose/my-app-usage.plist --into build/Info.plist
//! ```
//!
//! The usage file is what a build script wrote from the application's
//! declaration. Each key it holds replaces the one in the property list, or is
//! added before the closing tag when the list has none.

use std::{env, fs, process::ExitCode};

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

    let entries = match fs::read_to_string(&usage) {
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

    let merged = merge(&plist, &pairs(&entries));
    if let Err(error) = fs::write(&into, merged) {
        eprintln!("cranpose-plist: writing {into}: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// The key and value of each entry in a usage file.
fn pairs(entries: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut key = None;
    for line in entries.lines() {
        let line = line.trim();
        if let Some(name) = between(line, "<key>", "</key>") {
            key = Some(name.to_string());
        } else if let Some(value) = between(line, "<string>", "</string>") {
            if let Some(name) = key.take() {
                found.push((name, value.to_string()));
            }
        }
    }
    found
}

fn between<'a>(line: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = line.strip_prefix(open)?;
    start.strip_suffix(close)
}

/// Writes each pair into the property list, in place of the key it already
/// holds or before the closing tag.
fn merge(plist: &str, pairs: &[(String, String)]) -> String {
    let mut text = plist.to_string();
    for (key, value) in pairs {
        text = match key_at(&text, key) {
            Some(at) => replace_value(&text, at, value),
            None => add_entry(&text, key, value),
        };
    }
    text
}

fn key_at(text: &str, key: &str) -> Option<usize> {
    text.find(&format!("<key>{key}</key>"))
}

fn replace_value(text: &str, key_at: usize, value: &str) -> String {
    let tail = &text[key_at..];
    let Some(open) = tail.find("<string>") else {
        return text.to_string();
    };
    let Some(close) = tail[open..].find("</string>") else {
        return text.to_string();
    };
    let start = key_at + open + "<string>".len();
    let end = key_at + open + close;
    format!("{}{value}{}", &text[..start], &text[end..])
}

fn add_entry(text: &str, key: &str, value: &str) -> String {
    let Some(at) = text.rfind("</dict>") else {
        return text.to_string();
    };
    format!(
        "{}\t<key>{key}</key>\n\t<string>{value}</string>\n{}",
        &text[..at],
        &text[at..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLIST: &str = "<plist version=\"1.0\">\n<dict>\n\t<key>CFBundleName</key>\n\t<string>App</string>\n</dict>\n</plist>\n";

    #[test]
    fn a_usage_file_reads_as_key_and_value() {
        let text = "\t<key>NSCameraUsageDescription</key>\n\t<string>Reads receipts.</string>\n";
        assert_eq!(
            pairs(text),
            vec![(
                String::from("NSCameraUsageDescription"),
                String::from("Reads receipts.")
            )]
        );
    }

    #[test]
    fn a_new_key_lands_before_the_closing_tag() {
        let merged = merge(
            PLIST,
            &[(
                String::from("NSCameraUsageDescription"),
                String::from("Reads receipts."),
            )],
        );
        assert!(merged.contains(
            "<key>NSCameraUsageDescription</key>\n\t<string>Reads receipts.</string>\n</dict>"
        ));
        assert!(merged.contains("<key>CFBundleName</key>"));
    }

    #[test]
    fn a_key_the_list_holds_takes_the_new_sentence() {
        let once = merge(
            PLIST,
            &[(
                String::from("NSCameraUsageDescription"),
                String::from("First."),
            )],
        );
        let twice = merge(
            &once,
            &[(
                String::from("NSCameraUsageDescription"),
                String::from("Second."),
            )],
        );
        assert_eq!(twice.matches("NSCameraUsageDescription").count(), 1);
        assert!(twice.contains("<string>Second.</string>"));
        assert!(!twice.contains("First."));
    }
}
