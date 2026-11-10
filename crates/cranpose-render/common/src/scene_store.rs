//! The records a renderer draws, kept in arenas: fixed-size records in rows,
//! each record's items in one span, side arenas for what few records hold.

mod child_order;
mod content_hash;
mod facts;
mod growth_slack;
mod hash_cache;
mod heap_bytes;
mod items;
mod journal;
mod layer_props;
mod layer_ref;
mod leaf_write;
mod own_marks;
mod placement;
mod reach;
mod record_props;
mod recordings;
mod records;
mod resize_vec;
mod shape_chunks;
mod slab;
mod spec;
mod store;

use child_order::ChildOrder;
use facts::Facts;
pub use facts::OwnDrawFacts;
use hash_cache::HashCache;
pub use items::{Item, ItemKind};
pub(crate) use items::{ItemLists, Span, SpanPool};
use journal::Journal;
pub use journal::{JournalCursor, JournalEntry, JournalRead, RecordEvent, RecordEvents};
pub use layer_props::LayerProps;
pub use layer_ref::{ItemRef, LayerRef, RecordChanges};
pub use leaf_write::Leaf;
pub use placement::Placement;
pub use reach::{BLUR_PIXEL_SLACK, Extent, Reach, ShadowCaster};
pub(crate) use records::{
    GeoRecord, GraphicsLayerId, PropsId, ReachState, RecordNode, TransformId, TreeRecord,
    WarmRecord,
};
pub use records::{RecordFlags, RecordKind};
use shape_chunks::ShapeChunks;
pub use shape_chunks::{ShapeRef, ShapeRun};
pub(crate) use slab::Slab;
pub use store::{RecordSeed, SceneStore, SceneVersion};
