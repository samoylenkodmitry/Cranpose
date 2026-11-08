//! The records a renderer draws, kept in arenas: fixed-size records in rows,
//! each record's items in one span, side arenas for what few records hold.

mod child_order;
mod content_hash;
mod facts;
mod heap_bytes;
mod items;
mod journal;
mod layer_props;
mod layer_ref;
mod own_marks;
mod reach;
mod records;
mod slab;
mod spec;
mod store;

use child_order::ChildOrder;
pub use items::{Item, ItemKind};
pub(crate) use items::{Span, SpanPool};
use journal::Journal;
pub use journal::{JournalCursor, JournalEntry, JournalRead, RecordEvent, RecordEvents};
pub use layer_props::LayerProps;
pub use layer_ref::{ItemRef, LayerRef, RecordChange};
pub use reach::{BLUR_PIXEL_SLACK, Extent, Reach, ShadowCaster};
pub(crate) use records::{
    GeoRecord, GraphicsLayerId, PropsId, ReachState, TransformId, TreeRecord, WarmRecord,
};
pub use records::{RecordFlags, RecordKind};
pub(crate) use slab::Slab;
pub use store::{RecordSeed, SceneStore, SceneVersion};
