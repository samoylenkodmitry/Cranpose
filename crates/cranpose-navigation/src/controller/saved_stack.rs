//! The saved form of a back stack: `2 <next id>`, then the entries, then the
//! segments that pops with `save_state` kept, each a list of entries. A list
//! of entries is `<count>` followed by ` <id> <length> <route>` for each
//! entry, bottom first. The length counts the route's bytes, so a route's
//! saved form may hold any text. The leading number is the version of this
//! form; version `1`, without the counts and the segments, still reads.

use std::fmt::Write;

use cranpose_services::{SaveableScope, Saver};

use super::{BackStack, Entry};

const VERSION: &str = "2";
const VERSION_ONE: &str = "1";

pub(super) fn encode<R>(stack: &BackStack<R>, saver: &Saver<R>) -> String {
    let mut saved = format!("{VERSION} {}", stack.next_id);
    encode_entries(&mut saved, &stack.entries, saver);
    let _ = write!(saved, " {}", stack.saved.len());
    for segment in &stack.saved {
        encode_entries(&mut saved, segment, saver);
    }
    saved
}

fn encode_entries<R>(saved: &mut String, entries: &[Entry<R>], saver: &Saver<R>) {
    let _ = write!(saved, " {}", entries.len());
    for entry in entries {
        let route = saver.save(&entry.route);
        let _ = write!(saved, " {} {} {route}", entry.id, route.len());
    }
}

/// The back stack `saved` holds, up to the first entry that cannot be read,
/// with each kept segment whose entries all read, or `None` when not even
/// the bottom entry can be read.
pub(super) fn decode<R>(
    saved: &str,
    saver: &Saver<R>,
    saveables: &SaveableScope,
) -> Option<BackStack<R>> {
    if let Some(rest) = saved.strip_prefix(VERSION_ONE) {
        let (next_id, rest) = rest.strip_prefix(' ')?.split_once(' ')?;
        let next_id: u64 = next_id.parse().ok()?;
        let mut raw = Vec::new();
        let mut rest = rest;
        while let Some((id, route, tail)) = entry(rest) {
            raw.push((id, route));
            rest = tail;
        }
        let entries = readable_prefix(&raw, next_id, saver, saveables);
        return stack(next_id, entries, Vec::new(), saveables);
    }
    let rest = saved.strip_prefix(VERSION)?.strip_prefix(' ')?;
    let (next_id, rest) = rest.split_once(' ')?;
    let next_id: u64 = next_id.parse().ok()?;
    let (raw, mut rest) = entries(rest)?;
    let main = readable_prefix(&raw, next_id, saver, saveables);
    let (count, tail) = number(rest)?;
    rest = tail;
    let mut segments = Vec::new();
    for _ in 0..count {
        let (raw, tail) = entries(rest)?;
        rest = tail;
        let segment = readable_prefix(&raw, next_id, saver, saveables);
        if !segment.is_empty() && segment.len() == raw.len() {
            segments.push(segment);
        }
    }
    stack(next_id, main, segments, saveables)
}

fn stack<R>(
    next_id: u64,
    entries: Vec<Entry<R>>,
    saved: Vec<Vec<Entry<R>>>,
    saveables: &SaveableScope,
) -> Option<BackStack<R>> {
    (!entries.is_empty()).then(|| BackStack {
        entries,
        saved,
        next_id,
        saveables: saveables.clone(),
    })
}

/// The entries of `raw` up to the first one whose route cannot be read or
/// whose id is out of order.
fn readable_prefix<R>(
    raw: &[(u64, &str)],
    next_id: u64,
    saver: &Saver<R>,
    saveables: &SaveableScope,
) -> Vec<Entry<R>> {
    let mut entries: Vec<Entry<R>> = Vec::with_capacity(raw.len());
    for &(id, route) in raw {
        let in_order = id < next_id && entries.last().is_none_or(|last| last.id < id);
        let Some(route) = saver.restore(route).filter(|_| in_order) else {
            break;
        };
        entries.push(Entry::new(id, route, saveables));
    }
    entries
}

/// A counted list of entries at the start of `saved`, and what follows.
fn entries(saved: &str) -> Option<(Vec<(u64, &str)>, &str)> {
    let (count, mut rest) = number(saved)?;
    let mut raw = Vec::with_capacity(count.min(64));
    for _ in 0..count {
        let (id, route, tail) = entry(rest)?;
        raw.push((id, route));
        rest = tail;
    }
    Some((raw, rest))
}

/// The number at the start of `saved`, and what follows its space.
fn number(saved: &str) -> Option<(usize, &str)> {
    let (number, rest) = saved.split_once(' ').unwrap_or((saved, ""));
    Some((number.parse().ok()?, rest))
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
