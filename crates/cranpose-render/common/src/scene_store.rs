//! The records a renderer draws, kept in arenas: fixed-size records in rows,
//! each record's items in one span, side arenas for what few records hold.

mod content_hash;
mod facts;
mod graph_export;
mod items;
mod layer_props;
mod layer_ref;
mod records;
mod slab;
mod store;

pub(crate) use graph_export::export;
pub(crate) use items::ItemPool;
pub use items::{Item, ItemKind, ItemSpan};
pub use layer_props::LayerProps;
pub use layer_ref::{ItemRef, LayerRef, RecordChange};
pub(crate) use records::{GeoRecord, PropsId, TransformId, TreeRecord, WarmRecord};
pub use records::{ReachState, RecordFlags, RecordKind};
pub(crate) use slab::Slab;
pub use store::{RecordSeed, SceneStore};
