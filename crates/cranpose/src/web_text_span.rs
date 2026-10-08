//! The stretch of text an edit replaced, worked out from the text before and
//! after it, so the web's hidden editor hands a field only the text a person
//! changed.

/// Where two texts differ: the byte at which the change starts, the byte in
/// the old text at which the replaced text ends, and the byte in the new text
/// at which the text put in its place ends. Every offset sits on a character
/// boundary of its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChangedSpan {
    pub(crate) start: usize,
    pub(crate) removed_end: usize,
    pub(crate) inserted_end: usize,
}

/// The span by which `after` differs from `before`, or nothing when the two
/// are the same. The common start is taken first, and the common end only
/// from what follows it, so the two never overlap.
pub(crate) fn changed_span(before: &str, after: &str) -> Option<ChangedSpan> {
    if before == after {
        return None;
    }
    let start = before
        .char_indices()
        .zip(after.chars())
        .find_map(|((at, old), new)| (old != new).then_some(at))
        .unwrap_or_else(|| before.len().min(after.len()));
    let common_end: usize = before
        .get(start..)?
        .chars()
        .rev()
        .zip(after.get(start..)?.chars().rev())
        .take_while(|(old, new)| old == new)
        .map(|(old, _)| old.len_utf8())
        .sum();
    Some(ChangedSpan {
        start,
        removed_end: before.len() - common_end,
        inserted_end: after.len() - common_end,
    })
}

#[cfg(test)]
#[path = "tests/web_text_span_tests.rs"]
mod tests;
