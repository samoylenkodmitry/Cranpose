pub fn find_word_start(text: &str, pos: usize) -> usize {
    if pos == 0 || text.is_empty() {
        return 0;
    }

    let chars_before: Vec<(usize, char)> = text[..pos.min(text.len())].char_indices().collect();

    if chars_before.is_empty() {
        return 0;
    }

    let mut idx = chars_before.len();

    while idx > 0 {
        let c = chars_before[idx - 1].1;
        if c.is_alphanumeric() || c == '_' {
            break;
        }
        idx -= 1;
    }

    while idx > 0 {
        let c = chars_before[idx - 1].1;
        if !c.is_alphanumeric() && c != '_' {
            break;
        }
        idx -= 1;
    }

    if idx == 0 { 0 } else { chars_before[idx].0 }
}

pub fn find_word_end(text: &str, pos: usize) -> usize {
    let len = text.len();
    if pos >= len || text.is_empty() {
        return len;
    }

    let chars_after: Vec<(usize, char)> = text[pos.min(len)..]
        .char_indices()
        .map(|(i, c)| (pos + i, c))
        .collect();

    if chars_after.is_empty() {
        return len;
    }

    let mut idx = 0;

    while idx < chars_after.len() {
        let c = chars_after[idx].1;
        if c.is_alphanumeric() || c == '_' {
            break;
        }
        idx += 1;
    }

    while idx < chars_after.len() {
        let c = chars_after[idx].1;
        if !c.is_alphanumeric() && c != '_' {
            break;
        }
        idx += 1;
    }

    if idx >= chars_after.len() {
        len
    } else {
        chars_after[idx].0
    }
}

pub fn find_word_boundaries(text: &str, pos: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }

    let pos = pos.min(text.len());

    let char_at_pos = text[pos..].chars().next();

    let is_word_char = |c: char| c.is_alphanumeric() || c == '_';

    if char_at_pos.is_none_or(|c| !is_word_char(c)) {
        let char_before = text[..pos].chars().last();
        if char_before.is_none_or(|c| !is_word_char(c)) {
            return (pos, pos);
        }
    }

    let mut start = pos;
    for (i, c) in text[..pos].char_indices().rev() {
        if !is_word_char(c) {
            start = i + c.len_utf8();
            break;
        }
        start = i;
    }

    let mut end = pos;
    for (i, c) in text[pos..].char_indices() {
        if !is_word_char(c) {
            end = pos + i;
            break;
        }
        end = pos + i + c.len_utf8();
    }

    (start, end)
}

#[cfg(test)]
#[path = "tests/word_boundaries_tests.rs"]
mod tests;
