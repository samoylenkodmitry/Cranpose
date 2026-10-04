use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use fluent_syntax::ast::{Entry, Resource};
use proc_macro2::{TokenStream, TokenTree};

use super::TranslationCall;

struct Extracted {
    call: TranslationCall,
    locations: Vec<String>,
    comments: BTreeSet<String>,
}

/// Extracts `tr!` calls from Rust files, including calls nested inside other macros.
/// Conflicting explicit IDs are errors; identical entries share their source locations.
pub fn extract_sources(root: &Path, source_locale: &str) -> Result<String, String> {
    let source_locale = crate::Locale::parse(source_locale)
        .map_err(|error| error.to_string())?
        .to_string();
    let mut messages = BTreeMap::new();
    visit_path(root, &mut messages)?;
    let mut output = String::new();
    for entry in messages.values() {
        if entry.call.source_locale != source_locale {
            return Err(format!(
                "{} uses source locale {}, expected {source_locale}",
                entry.call.id, entry.call.source_locale
            ));
        }
        for comment in &entry.comments {
            write_comment(&mut output, Some(comment));
        }
        if let Some(context) = &entry.call.context {
            write_comment(&mut output, Some(&format!("Context: {context}")));
        }
        for location in &entry.locations {
            output.push_str(&format!("# Source: {location}\n"));
        }
        output.push_str(&entry.call.resource());
        output.push('\n');
    }
    Ok(output)
}

/// Updates extracted entries while preserving handwritten Fluent messages and translator notes.
/// Entries previously marked `Source:` but no longer present are removed from the source catalog.
pub fn merge_source_catalog(existing: &str, extracted: &str) -> Result<String, String> {
    let previous =
        fluent_syntax::parser::parse(existing).map_err(|(_, errors)| format!("{errors:?}"))?;
    let mut next =
        fluent_syntax::parser::parse(extracted).map_err(|(_, errors)| format!("{errors:?}"))?;
    let mut retained = Vec::new();
    for entry in previous.body {
        if let Entry::Message(old) = &entry {
            let replacement = next.body.iter_mut().find_map(|entry| match entry {
                Entry::Message(message) if message.id == old.id => Some(message),
                _ => None,
            });
            if let Some(replacement) = replacement {
                if let (Some(old_notes), Some(notes)) = (&old.comment, &mut replacement.comment) {
                    for note in &old_notes.content {
                        if !note.starts_with("Source:")
                            && !note.starts_with("Context:")
                            && !notes.content.contains(note)
                        {
                            notes.content.push(note);
                        }
                    }
                }
                continue;
            }
            if old.comment.as_ref().is_some_and(|comment| {
                comment
                    .content
                    .iter()
                    .any(|line| line.starts_with("Source:"))
            }) {
                continue;
            }
        }
        retained.push(entry);
    }
    retained.extend(next.body);
    Ok(fluent_syntax::serializer::serialize(&Resource {
        body: retained,
    }))
}

fn write_comment(output: &mut String, comment: Option<&str>) {
    if let Some(comment) = comment {
        for line in comment.lines() {
            output.push_str(&format!("# {line}\n"));
        }
    }
}

fn visit_path(path: &Path, messages: &mut BTreeMap<String, Extracted>) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        visit_directory(path, messages)?;
    } else if path.extension().is_some_and(|extension| extension == "rs") {
        let source = fs::read_to_string(path).map_err(|error| error.to_string())?;
        let tokens = source
            .parse()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        visit_tokens(tokens, path, messages)?;
    }
    Ok(())
}

fn visit_directory(path: &Path, messages: &mut BTreeMap<String, Extracted>) -> Result<(), String> {
    let mut children = fs::read_dir(path)
        .map_err(|error| error.to_string())?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    children.sort();
    for child in children {
        if child
            .file_name()
            .is_some_and(|name| name == "target" || name == ".git")
        {
            continue;
        }
        visit_path(&child, messages)?;
    }
    Ok(())
}

fn visit_tokens(
    tokens: TokenStream,
    path: &Path,
    messages: &mut BTreeMap<String, Extracted>,
) -> Result<(), String> {
    let mut tokens = tokens.into_iter().peekable();
    while let Some(token) = tokens.next() {
        match token {
            TokenTree::Ident(name)
                if (name == "tr" || name == "message")
                    && matches!(tokens.peek(), Some(TokenTree::Punct(punct)) if punct.as_char() == '!') =>
            {
                tokens.next();
                if let Some(TokenTree::Group(group)) = tokens.next() {
                    let location = format!("{}:{}", path.display(), name.span().start().line);
                    let call = syn::parse2::<TranslationCall>(group.stream())
                        .map_err(|error| format!("{location}: {error}"))?;
                    add_message(messages, call, location)?;
                }
            }
            TokenTree::Group(group) => visit_tokens(group.stream(), path, messages)?,
            _ => {}
        }
    }
    Ok(())
}

fn add_message(
    messages: &mut BTreeMap<String, Extracted>,
    mut call: TranslationCall,
    location: String,
) -> Result<(), String> {
    if let Some(previous) = messages.get_mut(&call.id) {
        if previous.call.pattern != call.pattern
            || previous.call.context != call.context
            || previous.call.source_locale != call.source_locale
        {
            return Err(format!(
                "conflicting message `{}` at {location} and {}",
                call.id,
                previous.locations.join(", ")
            ));
        }
        previous.locations.push(location);
        previous.comments.extend(call.comment.take());
    } else {
        let comments = call.comment.take().into_iter().collect();
        messages.insert(
            call.id.clone(),
            Extracted {
                call,
                locations: vec![location],
                comments,
            },
        );
    }
    Ok(())
}
