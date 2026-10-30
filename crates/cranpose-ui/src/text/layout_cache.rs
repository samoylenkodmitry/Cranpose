use std::{num::NonZeroU32, rc::Rc};

use cranpose_core::collections::pass_aged::PassAgedCache;
use cranpose_ui_graphics::MemoryHeadroom;
use smallvec::SmallVec;

use super::{
    TextAlign,
    measure::{PreparedTextLayout, TextFrame},
    style::TextStyle,
    text_key::TextKey,
};

/// Prepared layouts kept across nodes. Each node keeps its own, so this
/// serves the items a list composes again, a few screens of them.
const TEXT_PREPARED_CACHE_CAPACITY: usize = 1024;
// Layouts kept for one text at different widths.
const WIDTHS_PER_TEXT: usize = 2;
// Layouts of draw scopes' text kept until a pass draws none of them, the
// last drawn taking the place of the one drawn longest ago.
const DRAWN_LAYOUTS: usize = 8;
// The tags of 128 hashes, 512 bytes.
const RETURNING_SLOTS: usize = 128;

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

// How the cache keeps a layout it missed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CacheAdmission {
    // At once: a node's first text, which a list composes again.
    First,
    // Once it comes back: text a node changed in place, whose strings come
    // back only sometimes, as a stat's do, or never, as a ticker's. The
    // node keeps its own layout meanwhile.
    Changed,
    // Until a pass draws it no more, and for longer once it comes back: text
    // a draw scope draws, with no node to keep its layout.
    Drawn,
}

// The layouts recent passes used, shared across nodes.
pub(super) struct LayoutCache {
    kept: PassAgedCache<TextKey, Widths>,
    // A number drawn anew each frame keeps one or two layouts here, and a
    // string drawn every frame keeps its first.
    drawn: DrawnLayouts,
    // The texts changed in place or drawn lately: one laid out again while
    // here came back, and goes into `kept`.
    returning: ReturningKeys,
    // The host's memory headroom when the pass began. A tight heap keeps
    // no layout of a text that changes in place.
    headroom: MemoryHeadroom,
}

impl LayoutCache {
    pub(super) fn new() -> Self {
        Self {
            kept: PassAgedCache::with_capacity_at_least_one(TEXT_PREPARED_CACHE_CAPACITY),
            drawn: DrawnLayouts::default(),
            returning: ReturningKeys::default(),
            headroom: MemoryHeadroom::default(),
        }
    }

    // Drops every layout and every key it remembers.
    pub(super) fn clear(&mut self) {
        self.kept.clear();
        self.drawn = DrawnLayouts::default();
        self.returning = ReturningKeys::default();
    }

    // What the cache holds of `key` for `frame`, laid out as `admission`
    // says: a layout that breaks into the same lines, aligned to its widths,
    // or layouts at other widths only, or nothing.
    pub(super) fn cached(
        &mut self,
        key: TextKey,
        frame: TextFrame,
        (style, admission): (&TextStyle, CacheAdmission),
    ) -> Cached {
        let align = style.paragraph_style.text_align;
        if admission == CacheAdmission::Drawn
            && let Some(drawn) = self.drawn.get(key, frame, align)
        {
            return Cached::Held(drawn);
        }
        let Some(widths) = self.kept.get_mut(&key) else {
            return Cached::Missed(Miss::Absent);
        };
        let Some(held) = widths
            .iter()
            .find(|prepared| prepared.layout.holds(frame.min_width, frame.max_width))
            .map(Rc::clone)
        else {
            return Cached::Missed(Miss::OtherWidths);
        };
        let aligned = held.at_widths((frame.min_width, frame.max_width), align, None);
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
            if let Some(widths) = self.kept.get_mut(&key) {
                remember(widths, prepared);
            }
            return;
        }
        match admission {
            CacheAdmission::First => {}
            _ if self.headroom == MemoryHeadroom::Tight => return,
            CacheAdmission::Changed | CacheAdmission::Drawn => {
                let hash = key.mixed_hash();
                if !self.returning.contains(hash) {
                    self.returning.put(hash);
                    if admission == CacheAdmission::Drawn {
                        self.drawn.push(key, Rc::clone(prepared));
                    }
                    return;
                }
            }
        }
        self.kept
            .push(key, SmallVec::from_elem(Rc::clone(prepared), 1));
    }

    // Starts a layout pass on a host with `headroom`: drops the kept
    // layouts no pass asked for in the last `idle_passes`, and the drawn
    // ones the last pass did not draw.
    pub(super) fn begin_pass(&mut self, idle_passes: u64, headroom: MemoryHeadroom) {
        self.kept.begin_pass(idle_passes, drop);
        self.drawn.begin_pass();
        self.headroom = headroom;
    }
}

// Puts `prepared` first among the layouts of one text, keeping the newest.
fn remember(widths: &mut Widths, prepared: &Rc<PreparedTextLayout>) {
    widths.truncate(WIDTHS_PER_TEXT - 1);
    widths.insert(0, Rc::clone(prepared));
}

// The layouts of draw scopes' text the last pass drew, and the pass now.
#[derive(Default)]
struct DrawnLayouts {
    slots: Vec<DrawnLayout>,
    pass: u64,
}

struct DrawnLayout {
    key: TextKey,
    prepared: Rc<PreparedTextLayout>,
    // The pass that drew it last.
    pass: u64,
}

impl DrawnLayouts {
    // The layout of `key`, aligned to `frame`, marked drawn in this pass;
    // `None`, leaving no layout of `key`, when it breaks into other lines.
    fn get(
        &mut self,
        key: TextKey,
        frame: TextFrame,
        align: TextAlign,
    ) -> Option<Rc<PreparedTextLayout>> {
        let index = self.slots.iter().position(|slot| slot.key == key)?;
        let pass = self.pass;
        let slot = self.slots.get_mut(index)?;
        if !slot.prepared.layout.holds(frame.min_width, frame.max_width) {
            self.slots.swap_remove(index);
            return None;
        }
        slot.prepared = slot
            .prepared
            .at_widths((frame.min_width, frame.max_width), align, None);
        slot.pass = pass;
        Some(Rc::clone(&slot.prepared))
    }

    fn push(&mut self, key: TextKey, prepared: Rc<PreparedTextLayout>) {
        let drawn = DrawnLayout {
            key,
            prepared,
            pass: self.pass,
        };
        if self.slots.len() < DRAWN_LAYOUTS {
            self.slots.reserve_exact(DRAWN_LAYOUTS - self.slots.len());
            self.slots.push(drawn);
        } else if let Some(oldest) = self.slots.iter_mut().min_by_key(|slot| slot.pass) {
            *oldest = drawn;
        }
    }

    // Starts a pass: drops the layouts the last pass did not draw.
    fn begin_pass(&mut self) {
        self.pass += 1;
        let pass = self.pass;
        self.slots.retain(|slot| pass - slot.pass <= 1);
    }
}

// The tags of hashes, each in the slot its hash picks, the newest in place
// of the one before it.
#[derive(Default)]
struct ReturningKeys {
    tags: Vec<Option<NonZeroU32>>,
}

impl ReturningKeys {
    fn contains(&self, hash: u64) -> bool {
        let (slot, tag) = slot_and_tag(hash);
        self.tags.get(slot) == Some(&Some(tag))
    }

    fn put(&mut self, hash: u64) {
        if self.tags.is_empty() {
            self.tags = vec![None; RETURNING_SLOTS];
        }
        let (slot, tag) = slot_and_tag(hash);
        if let Some(kept) = self.tags.get_mut(slot) {
            *kept = Some(tag);
        }
    }
}

// The slot of `hash`, from its top bits, and its tag, from its low bits.
fn slot_and_tag(hash: u64) -> (usize, NonZeroU32) {
    let slot = usize::try_from(hash >> (u64::BITS - RETURNING_SLOTS.ilog2())).unwrap_or(0);
    let low = u32::try_from(hash & u64::from(u32::MAX)).unwrap_or(0);
    (slot, NonZeroU32::MIN | low)
}
