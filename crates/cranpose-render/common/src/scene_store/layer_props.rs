// What a record holds beyond its fixed-size parts: a graphics layer, pointer
// handling, bounds of its own. Only records that have one of them take an
// entry.

use std::cell::Cell;

use cranpose_ui::{Point, Rect};

use crate::{
    graph::{CachePolicy, ContentChanges, HitTestNode, IsolationReasons, LayerProperties},
    raster_cache::LayerRasterCacheHashes,
};

/// The properties of a record that has a graphics layer, pointer handling,
/// layer bounds other than its node rect, or translated content.
#[derive(Clone, Default)]
pub struct LayerProps {
    pub(crate) layer_bounds: Option<Rect>,
    pub(crate) graphics_layer: LayerProperties,
    pub(crate) origin_in_parent: Point,
    pub(crate) translated_content_offset: Point,
    pub(crate) hit_test: Option<HitTestNode>,
    pub(crate) isolation: IsolationReasons,
    pub(crate) cache_policy: CachePolicy,
    pub(crate) cache_hashes: Cell<Option<LayerRasterCacheHashes>>,
    pub(crate) content_changes: ContentChanges,
}
