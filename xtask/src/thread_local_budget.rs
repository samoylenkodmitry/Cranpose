use std::path::Path;

use crate::test_layout::{collect_sources, workspace_members};

const FRAMEWORK_CRATES: &str = "crates/";
const BUDGET: usize = 17;

pub(crate) fn run_at(root: &Path) -> Result<(), String> {
    check(root, BUDGET)
}

fn check(root: &Path, budget: usize) -> Result<(), String> {
    let counts = statics_per_crate(root)?;
    let total: usize = counts.iter().map(|(_, count)| count).sum();
    let table = counts
        .iter()
        .map(|(member, count)| format!("  {count:4} {member}"))
        .collect::<Vec<_>>()
        .join("\n");
    match total.cmp(&budget) {
        std::cmp::Ordering::Greater => Err(format!(
            "thread-local-budget: the framework crates declare {total} statics in \
             `thread_local!` blocks, over the budget of {budget}.\n\
             On Android, rustc's targets use emulated TLS without `has-thread-local`, so std \
             backs every `thread_local!` static with its own pthread key, created the first \
             time any thread touches it and never freed. Bionic gives a process about 128 keys, \
             shared with the system libraries, the GPU driver and the application, and std \
             aborts the process with \"fatal runtime error: out of TLS keys\" once none is left.\n\
             Add the new state as a field of the crate's existing thread-local struct instead \
             of a new static.\n{table}"
        )),
        std::cmp::Ordering::Less => Err(format!(
            "thread-local-budget: the framework crates declare {total} statics in \
             `thread_local!` blocks, under the budget of {budget}; lower BUDGET in \
             xtask/src/thread_local_budget.rs to {total} so the saving stays.\n{table}"
        )),
        std::cmp::Ordering::Equal => {
            println!(
                "thread-local-budget: {total} statics in `thread_local!` blocks, within the \
                 budget\n{table}"
            );
            Ok(())
        }
    }
}

fn statics_per_crate(root: &Path) -> Result<Vec<(String, usize)>, String> {
    let mut counts = Vec::new();
    for member in workspace_members(root)? {
        if !member.starts_with(FRAMEWORK_CRATES) {
            continue;
        }
        let mut sources = Vec::new();
        collect_sources(&root.join(&member).join("src"), &mut sources);
        let mut count = 0;
        for source in sources {
            let text = std::fs::read_to_string(&source)
                .map_err(|error| format!("failed to read {}: {error}", source.display()))?;
            count += thread_local_statics(&text);
        }
        if count > 0 {
            counts.push((member, count));
        }
    }
    counts.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    Ok(counts)
}

fn thread_local_statics(source: &str) -> usize {
    let code = code_only(source);
    let mut count = 0;
    let mut rest = code.as_slice();
    while let Some(position) = find(rest, b"thread_local!") {
        let after = &rest[position + b"thread_local!".len()..];
        let Some(open) = after.iter().position(|byte| !byte.is_ascii_whitespace()) else {
            break;
        };
        if !matches!(after[open], b'{' | b'(' | b'[') {
            rest = &after[open..];
            continue;
        }
        let (statics, consumed) = statics_in_block(&after[open..]);
        count += statics;
        rest = &after[open + consumed..];
    }
    count
}

fn statics_in_block(block: &[u8]) -> (usize, usize) {
    let mut depth = 0usize;
    let mut statics = 0;
    for (index, &byte) in block.iter().enumerate() {
        match byte {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return (statics, index + 1);
                }
            }
            b's' if depth == 1 && is_static_keyword(block, index) => statics += 1,
            _ => {}
        }
    }
    (statics, block.len())
}

fn is_static_keyword(code: &[u8], index: usize) -> bool {
    let word = b"static";
    let before_ok = index == 0 || !is_word_byte(code[index - 1]) && code[index - 1] != b'\'';
    let after = index + word.len();
    before_ok
        && code[index..].starts_with(word)
        && code.get(after).is_some_and(u8::is_ascii_whitespace)
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn code_only(source: &str) -> Vec<u8> {
    let bytes = source.as_bytes();
    let mut code = bytes.to_vec();
    let mut index = 0;
    while index < bytes.len() {
        let Some(end) = literal_or_comment_end(bytes, index) else {
            index += 1;
            continue;
        };
        for byte in &mut code[index..end] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        index = end;
    }
    code
}

fn literal_or_comment_end(bytes: &[u8], index: usize) -> Option<usize> {
    match bytes[index] {
        b'/' => match bytes.get(index + 1)? {
            b'/' => Some(line_end(bytes, index)),
            b'*' => Some(block_comment_end(bytes, index)),
            _ => None,
        },
        b'"' => Some(string_end(bytes, index + 1)),
        b'r' => raw_string_hashes(bytes, index).map(|hashes| raw_string_end(bytes, index, hashes)),
        b'\'' => char_literal_end(bytes, index),
        _ => None,
    }
}

fn line_end(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|&byte| byte == b'\n')
        .map_or(bytes.len(), |offset| start + offset)
}

fn block_comment_end(bytes: &[u8], start: usize) -> usize {
    let mut depth = 0usize;
    let mut index = start;
    while index + 1 < bytes.len() {
        match (bytes[index], bytes[index + 1]) {
            (b'/', b'*') => {
                depth += 1;
                index += 2;
            }
            (b'*', b'/') => {
                depth -= 1;
                index += 2;
                if depth == 0 {
                    return index;
                }
            }
            _ => index += 1,
        }
    }
    bytes.len()
}

fn string_end(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

fn raw_string_hashes(bytes: &[u8], start: usize) -> Option<usize> {
    let prefix_ok = match start.checked_sub(1).map(|before| bytes[before]) {
        None => true,
        Some(b'b') => start < 2 || !is_word_byte(bytes[start - 2]),
        Some(before) => !is_word_byte(before),
    };
    if !prefix_ok {
        return None;
    }
    let hashes = bytes[start + 1..]
        .iter()
        .take_while(|&&byte| byte == b'#')
        .count();
    (bytes.get(start + 1 + hashes) == Some(&b'"')).then_some(hashes)
}

fn raw_string_end(bytes: &[u8], start: usize, hashes: usize) -> usize {
    let mut index = start + 2 + hashes;
    while index < bytes.len() {
        let closing = bytes[index + 1..].iter().take(hashes);
        if bytes[index] == b'"'
            && closing.len() == hashes
            && closing.into_iter().all(|&byte| byte == b'#')
        {
            return index + 1 + hashes;
        }
        index += 1;
    }
    bytes.len()
}

fn char_literal_end(bytes: &[u8], start: usize) -> Option<usize> {
    let first = *bytes.get(start + 1)?;
    let close = if first == b'\\' {
        start
            + 2
            + bytes
                .get(start + 2..)?
                .iter()
                .position(|&byte| byte == b'\'')?
    } else {
        start + 1 + utf8_width(first)
    };
    (bytes.get(close) == Some(&b'\'')).then_some(close + 1)
}

fn utf8_width(first: u8) -> usize {
    match first {
        0xF0.. => 4,
        0xE0.. => 3,
        0xC0.. => 2,
        _ => 1,
    }
}

#[cfg(test)]
#[path = "tests/thread_local_budget_tests.rs"]
mod tests;
