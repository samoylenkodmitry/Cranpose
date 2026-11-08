//! The records a renderer draws, kept in arenas: fixed-size records in rows,
//! each record's items in one span, side arenas for what few records hold.

mod content_hash;
mod facts;
mod heap_bytes;
mod items;
mod journal;
mod layer_props;
mod layer_ref;
mod records;
mod slab;
mod spec;
mod store;

pub(crate) use items::ItemPool;
pub use items::{Item, ItemKind, ItemSpan};
use journal::Journal;
pub use journal::{JournalCursor, JournalEntry, JournalRead, RecordEvent, RecordEvents};
pub use layer_props::LayerProps;
pub use layer_ref::{ItemRef, LayerRef, RecordChange};
pub(crate) use records::{
    GeoRecord, GraphicsLayerId, PropsId, TransformId, TreeRecord, WarmRecord,
};
pub use records::{ReachState, RecordFlags, RecordKind};
pub(crate) use slab::Slab;
pub use store::{RecordSeed, SceneStore, SceneVersion};
