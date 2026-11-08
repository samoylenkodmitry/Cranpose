// What a record holds beyond its fixed-size parts: a graphics layer, pointer
// handling, bounds of its own. Only records that have one of them take an
// entry.

use std::cell::Cell;

use cranpose_core::SceneUpdate;
use cranpose_ui::{Point, Rect};

use crate::{
    graph::{CachePolicy, HitTestNode, IsolationReasons, LayerProperties},
    raster_cache::LayerRasterCacheHashes,
};

/// The properties of a record that has a graphics layer, pointer handling,
/// layer bounds other than its node rect, or translated content.
#[derive(Clone, Default)]
pub struct LayerProps {
    /// The bounds of the layer, where its clip cuts, its transforms pivot and
    /// an offscreen pass draws it, when they differ from the node rect.
    pub layer_bounds: Option<Rect>,
    /// The graphics layer properties; they read as defaults when none were set.
    pub graphics_layer: LayerProperties,
    /// Translation of the content when the record moves its content as one
    /// translated block.
    pub translated_content_offset: Point,
    /// Pointer handling of the node, or `None` when the node has none.
    pub hit_test: Option<HitTestNode>,
    /// The reasons the record is drawn through its own offscreen buffer.
    pub isolation: IsolationReasons,
    /// Whether a renderer may keep the record's pixels between frames.
    pub cache_policy: CachePolicy,
    pub(crate) cache_hashes: Cell<Option<LayerRasterCacheHashes>>,
    pub(crate) content_run: ContentRun,
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
