//! The saved form of a back stack: `1 <next id>`, then ` <id> <length>
//! <route>` for each entry, bottom first. The length counts the route's
//! bytes, so a route's saved form may hold any text. The leading `1` is the
//! version of this form.

use std::fmt::Write;

use cranpose_services::{SaveableScope, Saver};

use super::{BackStack, Entry};

const VERSION: &str = "1";

pub(super) fn encode<R>(stack: &BackStack<R>, saver: &Saver<R>) -> String {
    let mut saved = format!("{VERSION} {}", stack.next_id);
    for entry in &stack.entries {
        let route = saver.save(&entry.route);
        let _ = write!(saved, " {} {} {route}", entry.id, route.len());
    }
    saved
}

/// The back stack `saved` holds, up to the first entry that cannot be read,
/// or `None` when not even the bottom one can.
pub(super) fn decode<R>(
    saved: &str,
    saver: &Saver<R>,
    saveables: &SaveableScope,
) -> Option<BackStack<R>> {
    let rest = saved.strip_prefix(VERSION)?.strip_prefix(' ')?;
    let (next_id, mut rest) = rest.split_once(' ')?;
    let next_id: u64 = next_id.parse().ok()?;
    let mut entries: Vec<Entry<R>> = Vec::new();
    while let Some((id, route, tail)) = entry(rest) {
        let in_order = id < next_id && entries.last().is_none_or(|last| last.id < id);
        let Some(route) = saver.restore(route).filter(|_| in_order) else {
            break;
        };
        entries.push(Entry::new(id, route, saveables));
        rest = tail;
    }
    (!entries.is_empty()).then(|| BackStack {
        entries,
        next_id,
        saveables: saveables.clone(),
    })
}

/// The id and saved route of the first entry in `saved`, and what follows.
fn entry(saved: &str) -> Option<(u64, &str, &str)> {
    let (id, rest) = saved.split_once(' ')?;
    let (length, rest) = rest.split_once(' ')?;
    let (route, rest) = rest.split_at_checked(length.parse().ok()?)?;
    let rest = match rest.strip_prefix(' ') {
        Some(rest) => rest,
        None if rest.is_empty() => rest,
        None => return None,
    };
    Some((id.parse().ok()?, route, rest))
}
