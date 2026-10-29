use std::rc::Rc;

use cranpose_core::collections::pass_aged::PassAgedCache;
use cranpose_ui_graphics::{HashBits, MemoryHeadroom};
use smallvec::SmallVec;

use super::{
    measure::{PreparedTextLayout, TextFrame},
    style::TextStyle,
    text_key::TextKey,
};

/// Prepared layouts kept across nodes. Each node keeps its own, so this
/// serves the items a list composes again, a few screens of them.
const TEXT_PREPARED_CACHE_CAPACITY: usize = 1024;
// Layouts kept for one text at different widths.
const WIDTHS_PER_TEXT: usize = 2;

// The layouts of one text at different widths, the newest first.
type Widths = SmallVec<[Rc<PreparedTextLayout>; WIDTHS_PER_TEXT]>;

// What the cache holds of a text for a frame.
pub(super) enum Cached {
    // A layout that breaks into the same lines, aligned to the frame.
    Held(Rc<PreparedTextLayout>),
    Missed(Miss),
}

// What the cache holds of a text it holds no layout of for a frame.
#[derive(Clone, Copy)]
pub(super) enum Miss {
    // Layouts at other widths, which a new one joins.
    OtherWidths,
    Absent,
}

// When the cache keeps a layout it missed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CacheAdmission {
    // At once: a node's first text, which a list composes again.
    First,
    // Once asked for again since the doorkeeper last forgot: text changed
    // in place or drawn by a draw scope, whose strings come back only
    // sometimes, as a stat's do, or never, as a ticker's.
    Second,
}

// The layouts recent passes used, shared across nodes.
pub(super) struct LayoutCache {
    prepared: PassAgedCache<TextKey, Widths>,
    // The keys of layouts missed once, by which a text missed again goes
    // into the cache: a string drawn every frame keeps its second layout,
    // and one drawn once keeps nothing.
    seen: HashBits,
    // The host's memory headroom when the pass began. A tight heap keeps
    // no layout of a text that changes in place.
    headroom: MemoryHeadroom,
}

impl LayoutCache {
    pub(super) fn new() -> Self {
        Self {
            prepared: PassAgedCache::with_capacity_at_least_one(TEXT_PREPARED_CACHE_CAPACITY),
            seen: HashBits::default(),
            headroom: MemoryHeadroom::default(),
        }
    }

    // Drops every layout and every key seen.
    pub(super) fn clear(&mut self) {
        self.prepared.clear();
        self.seen = HashBits::default();
    }

    // What the cache holds of `key` for `frame`: a layout that breaks into
    // the same lines, aligned to its widths, or layouts at other widths
    // only, or nothing.
    pub(super) fn cached(&mut self, key: TextKey, frame: TextFrame, style: &TextStyle) -> Cached {
        let Some(widths) = self.prepared.get_mut(&key) else {
            return Cached::Missed(Miss::Absent);
        };
        let Some(held) = widths
            .iter()
            .find(|prepared| prepared.layout.holds(frame.min_width, frame.max_width))
            .map(Rc::clone)
        else {
            return Cached::Missed(Miss::OtherWidths);
        };
        let aligned = held.at_widths(frame, style.paragraph_style.text_align);
        if !Rc::ptr_eq(&aligned, &held) {
            remember(widths, &aligned);
        }
        Cached::Held(aligned)
    }

    // Puts `prepared`, laid out for `key` after `miss`, beside the layouts
    // the cache holds of `key` at other widths, or keeps it as `admission`
    // says when it held none.
    pub(super) fn keep(
        &mut self,
        key: TextKey,
        prepared: &Rc<PreparedTextLayout>,
        (miss, admission): (Miss, CacheAdmission),
    ) {
        if let Miss::OtherWidths = miss {
            if let Some(widths) = self.prepared.get_mut(&key) {
                remember(widths, prepared);
            }
            return;
        }
        let admitted = match admission {
            CacheAdmission::First => true,
            CacheAdmission::Second if self.headroom == MemoryHeadroom::Ample => {
                let hash = key.mixed_hash();
                self.seen.contains(hash) || {
                    self.seen.insert(hash);
                    false
                }
            }
            CacheAdmission::Second => false,
        };
        if admitted {
            self.prepared
                .push(key, SmallVec::from_elem(Rc::clone(prepared), 1));
        }
    }

    // Starts a layout pass on a host with `headroom`: drops the layouts no
    // pass asked for in the last `idle_passes`.
    pub(super) fn begin_pass(&mut self, idle_passes: u64, headroom: MemoryHeadroom) {
        self.prepared.begin_pass(idle_passes, drop);
        self.headroom = headroom;
    }
}

// Puts `prepared` first among the layouts of one text, keeping the newest.
fn remember(widths: &mut Widths, prepared: &Rc<PreparedTextLayout>) {
    widths.insert(0, Rc::clone(prepared));
    widths.truncate(WIDTHS_PER_TEXT);
}
