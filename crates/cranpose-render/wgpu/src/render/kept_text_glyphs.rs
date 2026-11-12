// The glyph quads of texts the scene store holds, kept with the record of
// each text. A text draws from its kept quads again while its record keeps
// its own draws, its color and scale stay and the glyph atlas it points
// into stays. A text cut at its scissor's edges that draws twice in a row
// at one place keeps the instances it drew, and draws from them while it
// stays there. A change rebuilds the quads of that text alone.

use std::num::NonZeroU32;

use cranpose_core::StoreId;
use cranpose_render_common::scene_store::RecordChanges;
use cranpose_ui_graphics::Color;

use super::{
    GlyphExtent, GlyphInstance, GlyphQuads, GlyphRunBounds, TEXT_GLYPH_RUN_IDLE_FRAMES,
    text_glyph_cache::{CachedTextGlyphQuad, TextGlyphRunCacheKey},
};

// Frames between two sweeps: a sweep frees the texts idle for
// [`TEXT_GLYPH_RUN_IDLE_FRAMES`] and keeps the cached runs of the others,
// which no text draws from while its quads are kept, from idling out.
const KEPT_SWEEP_FRAMES: u64 = TEXT_GLYPH_RUN_IDLE_FRAMES / 2;

// What decides the quads of a kept text besides its record: its color and
// the scale it rasters at, which its layers set, and the glyph atlas the
// quads point into.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct KeptFacts {
    color: [u32; 4],
    scale: u32,
    atlas: u64,
}

impl KeptFacts {
    pub(super) fn of(color: Color, text_scale: f32, atlas_generation: u64) -> Self {
        Self {
            color: [
                color.0.to_bits(),
                color.1.to_bits(),
                color.2.to_bits(),
                color.3.to_bits(),
            ],
            scale: text_scale.to_bits(),
            atlas: atlas_generation,
        }
    }
}

// Where a text draws, as what its cut instances depend on besides its quads:
// the edges its quads are cut to, from its origin, and the origin of the
// target they land in. A text that moves by whole pixels keeps its place:
// its instances move with it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct DrawnAt {
    cut: Option<[u32; 4]>,
    offset: [u32; 2],
}

impl DrawnAt {
    // The place of a text at `origin` cut to `cut` in a target at `offset`;
    // `None` unless all of them lie on whole pixels, where moving the
    // instances adds no rounding.
    pub(super) fn of(origin: [f32; 2], cut: Option<[f32; 4]>, offset: [f32; 2]) -> Option<Self> {
        let whole = |value: &f32| value.abs() <= MOST_WHOLE_PIXEL && value.fract() == 0.0;
        if !origin
            .iter()
            .chain(&offset)
            .chain(cut.iter().flatten())
            .all(whole)
        {
            return None;
        }
        Some(Self {
            cut: cut.map(|[left, top, right, bottom]| {
                [
                    left - origin[0],
                    top - origin[1],
                    right - origin[0],
                    bottom - origin[1],
                ]
                .map(f32::to_bits)
            }),
            offset: offset.map(f32::to_bits),
        })
    }
}

// The largest pixel coordinate whose sums with other such coordinates are
// whole in `f32`.
const MOST_WHOLE_PIXEL: f32 = 8_388_608.0;

enum KeptBody {
    // The quads of the text's run, from its origin. `last` is where the text
    // drew last with some of its quads cut.
    Quads {
        quads: Vec<CachedTextGlyphQuad>,
        last: Option<DrawnAt>,
    },
    // The instances the text drew at `at`, from its origin, which a draw
    // there appends again.
    Drawn {
        at: DrawnAt,
        kind: GlyphQuads,
        extent: GlyphExtent,
        instances: Vec<GlyphInstance>,
    },
}

pub(super) struct KeptText {
    changes: RecordChanges,
    facts: KeptFacts,
    run_key: TextGlyphRunCacheKey,
    bounds: GlyphRunBounds,
    body: KeptBody,
    seen: u64,
}

impl KeptText {
    pub(super) fn bounds(&self) -> GlyphRunBounds {
        self.bounds
    }

    // The quads of the text, unless it keeps the instances it drew.
    pub(super) fn quads(&self) -> Option<&[CachedTextGlyphQuad]> {
        match &self.body {
            KeptBody::Quads { quads, .. } => Some(quads),
            KeptBody::Drawn { .. } => None,
        }
    }

    // The instances the text drew at `at`, from its origin, with the kind of
    // their draw and their extent.
    pub(super) fn drawn_at(
        &self,
        at: DrawnAt,
    ) -> Option<(&[GlyphInstance], GlyphQuads, GlyphExtent)> {
        match &self.body {
            KeptBody::Drawn {
                at: drawn,
                kind,
                extent,
                instances,
            } if *drawn == at => Some((instances, *kind, *extent)),
            _ => None,
        }
    }

    // Notes that the text, its origin at `origin`, drew `instances` at `at`
    // with some of its quads cut. A second draw there in a row keeps them,
    // from the origin, in place of the quads.
    pub(super) fn note_drawn(
        &mut self,
        (at, origin): (DrawnAt, [f32; 2]),
        (instances, kind, extent): (&[GlyphInstance], GlyphQuads, GlyphExtent),
    ) {
        match &mut self.body {
            KeptBody::Quads { last, .. } if *last != Some(at) => *last = Some(at),
            KeptBody::Quads { .. } => {
                let back = origin.map(|value| -value);
                self.body = KeptBody::Drawn {
                    at,
                    kind,
                    extent: extent.moved(back),
                    instances: instances.iter().map(|glyph| glyph.moved(back)).collect(),
                };
            }
            KeptBody::Drawn { .. } => {}
        }
    }
}

// The texts of one store by the row of their record.
struct StoreRows {
    store: StoreId,
    slots: Vec<Option<NonZeroU32>>,
}

// The kept texts of a renderer. A text takes the slot of its record's row;
// a slot idle for [`TEXT_GLYPH_RUN_IDLE_FRAMES`] frames is freed, so what is
// kept follows what the last frames drew.
#[derive(Default)]
pub(super) struct KeptTextGlyphs {
    stores: Vec<StoreRows>,
    texts: Vec<Option<KeptText>>,
    free: Vec<usize>,
    frame: u64,
    swept: u64,
}

impl KeptTextGlyphs {
    // Opens frame number `frame`. Every [`KEPT_SWEEP_FRAMES`] frames it frees
    // the idle texts and hands `keep_run` the key of the cached run of each
    // text drawn since the last sweep.
    pub(super) fn begin_frame(
        &mut self,
        frame: u64,
        mut keep_run: impl FnMut(TextGlyphRunCacheKey),
    ) {
        self.frame = frame;
        if !frame.is_multiple_of(KEPT_SWEEP_FRAMES) {
            return;
        }
        let swept = std::mem::replace(&mut self.swept, frame);
        for slot in 0..self.texts.len() {
            let Some(text) = &self.texts[slot] else {
                continue;
            };
            if frame - text.seen >= TEXT_GLYPH_RUN_IDLE_FRAMES {
                self.free_slot(slot);
            } else if text.seen > swept {
                keep_run(text.run_key);
            }
        }
        while self.texts.last().is_some_and(Option::is_none) {
            self.texts.pop();
        }
        let len = self.texts.len();
        self.free.retain(|&slot| slot < len);
    }

    // The kept text of the record `changes` names, when its record kept its
    // own draws since the text was kept and `facts` are the same.
    pub(super) fn get(
        &mut self,
        changes: RecordChanges,
        facts: KeptFacts,
    ) -> Option<&mut KeptText> {
        let slot = self.slot_of(changes)?;
        let text = self.texts[slot].as_mut()?;
        if text.facts != facts || !text.changes.same_own(changes) {
            return None;
        }
        text.seen = self.frame;
        Some(text)
    }

    // Keeps `quads`, the quads of a text of the record `changes` names, in
    // place of what the record's slot held. `run_key` names the cached run
    // the quads were made of.
    pub(super) fn keep(
        &mut self,
        (changes, facts): (RecordChanges, KeptFacts),
        (run_key, bounds): (TextGlyphRunCacheKey, GlyphRunBounds),
        quads: impl Iterator<Item = CachedTextGlyphQuad>,
    ) {
        let slot = self.slot_for(changes);
        let seen = self.frame;
        let text = self.texts[slot].get_or_insert_with(|| KeptText {
            changes,
            facts,
            run_key,
            bounds,
            body: KeptBody::Quads {
                quads: Vec::new(),
                last: None,
            },
            seen,
        });
        text.changes = changes;
        text.facts = facts;
        text.run_key = run_key;
        text.bounds = bounds;
        text.seen = seen;
        match &mut text.body {
            KeptBody::Quads { quads: kept, last } => {
                kept.clear();
                kept.extend(quads);
                *last = None;
            }
            body @ KeptBody::Drawn { .. } => {
                *body = KeptBody::Quads {
                    quads: quads.collect(),
                    last: None,
                };
            }
        }
    }

    fn slot_of(&self, changes: RecordChanges) -> Option<usize> {
        let rows = self
            .stores
            .iter()
            .find(|rows| rows.store == changes.store())?;
        let slot = (*rows.slots.get(changes.record().id.index())?)?;
        Some(slot.get() as usize - 1)
    }

    // The slot of the record's row, taken when the row has none.
    fn slot_for(&mut self, changes: RecordChanges) -> usize {
        let store = self
            .stores
            .iter()
            .position(|rows| rows.store == changes.store())
            .unwrap_or_else(|| {
                self.stores.push(StoreRows {
                    store: changes.store(),
                    slots: Vec::new(),
                });
                self.stores.len() - 1
            });
        let row = changes.record().id.index();
        let slots = &mut self.stores[store].slots;
        if slots.len() <= row {
            slots.resize(row + 1, None);
        }
        if let Some(slot) = slots[row] {
            return slot.get() as usize - 1;
        }
        let slot = self.free.pop().unwrap_or_else(|| {
            self.texts.push(None);
            self.texts.len() - 1
        });
        slots[row] = u32::try_from(slot + 1).ok().and_then(NonZeroU32::new);
        slot
    }

    fn free_slot(&mut self, slot: usize) {
        let Some(text) = self.texts[slot].take() else {
            return;
        };
        let store = text.changes.store();
        if let Some(rows) = self.stores.iter_mut().find(|rows| rows.store == store)
            && let Some(row) = rows.slots.get_mut(text.changes.record().id.index())
        {
            *row = None;
        }
        self.free.push(slot);
    }
}
