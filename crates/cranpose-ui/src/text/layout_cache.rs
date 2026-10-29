use std::rc::Rc;

use cranpose_core::collections::pass_aged::PassAgedCache;
use cranpose_ui_graphics::MemoryHeadroom;
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
// Passes a recent layout stays without a lookup: a string the last pass
// drew is still there.
const RECENT_IDLE_PASSES: u64 = 1;
// The keys the cache remembers of the texts it laid out last into its
// recent part.
const RETURNING_KEYS: usize = 64;

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
    // Layouts at other widths in a part, which a new one joins.
    OtherWidths(Shelf),
    Absent,
}

// A part of the cache.
#[derive(Clone, Copy)]
pub(super) enum Shelf {
    // Keeps a layout while passes ask for it.
    Kept,
    // Keeps a layout while the last pass asked for it.
    Recent,
}

// Which part of the cache keeps a layout it missed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CacheAdmission {
    // The kept part: a node's first text, which a list composes again.
    First,
    // The recent part, and the kept part once the text comes back after it
    // left: text changed in place or drawn by a draw scope, whose strings
    // come back only sometimes, as a stat's do, or never, as a ticker's.
    Second,
}

// The layouts recent passes used, shared across nodes.
pub(super) struct LayoutCache {
    kept: PassAgedCache<TextKey, Widths>,
    // A number drawn anew each frame keeps one or two layouts here, and a
    // string drawn every frame keeps its first.
    recent: PassAgedCache<TextKey, Widths>,
    // The hashes of the keys of the texts laid out last into `recent`: one
    // laid out again after it left came back, and goes into `kept`.
    returning: ReturningKeys,
    // The host's memory headroom when the pass began. A tight heap keeps
    // no layout of a text that changes in place.
    headroom: MemoryHeadroom,
}

impl LayoutCache {
    pub(super) fn new() -> Self {
        Self {
            kept: PassAgedCache::with_capacity_at_least_one(TEXT_PREPARED_CACHE_CAPACITY),
            recent: PassAgedCache::with_capacity_at_least_one(TEXT_PREPARED_CACHE_CAPACITY),
            returning: ReturningKeys::default(),
            headroom: MemoryHeadroom::default(),
        }
    }

    // Drops every layout and every key it remembers.
    pub(super) fn clear(&mut self) {
        self.kept.clear();
        self.recent.clear();
        self.returning = ReturningKeys::default();
    }

    // What the cache holds of `key` for `frame`, looked for first in the
    // part `admission` keeps it in: a layout that breaks into the same
    // lines, aligned to its widths, or layouts at other widths only, or
    // nothing.
    pub(super) fn cached(
        &mut self,
        key: TextKey,
        frame: TextFrame,
        (style, admission): (&TextStyle, CacheAdmission),
    ) -> Cached {
        let shelves = match admission {
            CacheAdmission::First => [Shelf::Kept, Shelf::Recent],
            CacheAdmission::Second => [Shelf::Recent, Shelf::Kept],
        };
        for shelf in shelves {
            let Some(widths) = self.shelf(shelf).get_mut(&key) else {
                continue;
            };
            let Some(held) = widths
                .iter()
                .find(|prepared| prepared.layout.holds(frame.min_width, frame.max_width))
                .map(Rc::clone)
            else {
                return Cached::Missed(Miss::OtherWidths(shelf));
            };
            let aligned = held.at_widths(frame, style.paragraph_style.text_align);
            if !Rc::ptr_eq(&aligned, &held) {
                remember(widths, &aligned);
            }
            return Cached::Held(aligned);
        }
        Cached::Missed(Miss::Absent)
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
        let shelf = match miss {
            Miss::OtherWidths(shelf) => {
                if let Some(widths) = self.shelf(shelf).get_mut(&key) {
                    remember(widths, prepared);
                }
                return;
            }
            Miss::Absent => match admission {
                CacheAdmission::First => Shelf::Kept,
                CacheAdmission::Second if self.headroom == MemoryHeadroom::Tight => return,
                CacheAdmission::Second => {
                    let hash = key.mixed_hash();
                    if self.returning.contains(hash) {
                        Shelf::Kept
                    } else {
                        self.returning.put(hash);
                        Shelf::Recent
                    }
                }
            },
        };
        self.shelf(shelf)
            .push(key, SmallVec::from_elem(Rc::clone(prepared), 1));
    }

    // Starts a layout pass on a host with `headroom`: drops the kept
    // layouts no pass asked for in the last `idle_passes`, and the recent
    // ones the last pass did not ask for.
    pub(super) fn begin_pass(&mut self, idle_passes: u64, headroom: MemoryHeadroom) {
        self.kept.begin_pass(idle_passes, drop);
        self.recent.begin_pass(RECENT_IDLE_PASSES, drop);
        self.headroom = headroom;
    }

    fn shelf(&mut self, shelf: Shelf) -> &mut PassAgedCache<TextKey, Widths> {
        match shelf {
            Shelf::Kept => &mut self.kept,
            Shelf::Recent => &mut self.recent,
        }
    }
}

// Hashes in a ring: once full, each new one takes the place of the next in
// turn.
#[derive(Default)]
struct ReturningKeys {
    hashes: Vec<u64>,
    next: usize,
}

impl ReturningKeys {
    fn contains(&self, hash: u64) -> bool {
        self.hashes.contains(&hash)
    }

    fn put(&mut self, hash: u64) {
        if self.hashes.len() < RETURNING_KEYS {
            self.hashes
                .reserve_exact(RETURNING_KEYS - self.hashes.len());
            self.hashes.push(hash);
        } else if let Some(slot) = self.hashes.get_mut(self.next) {
            *slot = hash;
            self.next = (self.next + 1) % RETURNING_KEYS;
        }
    }
}

// Puts `prepared` first among the layouts of one text, keeping the newest.
fn remember(widths: &mut Widths, prepared: &Rc<PreparedTextLayout>) {
    widths.insert(0, Rc::clone(prepared));
    widths.truncate(WIDTHS_PER_TEXT);
}
