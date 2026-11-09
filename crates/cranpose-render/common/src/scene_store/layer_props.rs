// What a record holds beyond its fixed-size parts and its graphics layer:
// pointer handling, bounds of its own, offscreen reasons. Only records that
// have one of them take an entry.

use cranpose_core::SceneUpdate;
use cranpose_ui::{Point, Rect};

use crate::graph::{CachePolicy, HitTestNode, IsolationReasons};

/// The properties of a record that has pointer handling, layer bounds other
/// than its node rect, translated content, a reason to draw offscreen or a
/// cache policy.
#[derive(Clone, Default)]
pub struct LayerProps {
    /// The bounds of the layer, where its clip cuts, its transforms pivot and
    /// an offscreen pass draws it, when they differ from the node rect.
    pub layer_bounds: Option<Rect>,
    /// Translation of the content when the record moves its content as one
    /// translated block.
    pub translated_content_offset: Point,
    /// Pointer handling of the node, or `None` when the node has none.
    pub hit_test: Option<HitTestNode>,
    /// The reasons the record is drawn through its own offscreen buffer.
    pub isolation: IsolationReasons,
    /// Whether a renderer may keep the record's pixels between frames.
    pub cache_policy: CachePolicy,
    pub(crate) content_run: ContentRun,
}

impl LayerProps {
    // Whether every property reads as its default, so the record needs no
    // entry. The content run goes with the entry.
    pub(crate) fn is_default(&self) -> bool {
        self.layer_bounds.is_none()
            && self.translated_content_offset == Point::default()
            && self.hit_test.is_none()
            && !self.isolation.has_any()
            && self.cache_policy == CachePolicy::None
    }
}

// The last scene update that changed what a record draws, and how many
// updates in a row up to it did.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ContentRun {
    last: SceneUpdate,
    run: u32,
}

impl ContentRun {
    pub(crate) fn note(&mut self, update: SceneUpdate) {
        if self.run > 0 && self.last == update {
            return;
        }
        self.run = if self.run > 0 && self.last.next() == update {
            self.run + 1
        } else {
            1
        };
        self.last = update;
    }

    pub(crate) fn churns(self, update: SceneUpdate) -> bool {
        self.run >= 2 && self.last == update
    }
}
