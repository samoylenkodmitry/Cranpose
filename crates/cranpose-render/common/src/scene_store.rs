//! The records a renderer draws, kept in arenas: fixed-size records in rows,
//! each record's items in one span, side arenas for what few records hold.

mod items;
mod layer_props;
mod records;
mod slab;
mod store;

pub(crate) use items::ItemPool;
pub use items::{Item, ItemKind, ItemSpan};
pub use layer_props::LayerProps;
pub(crate) use records::{GeoRecord, PropsId, TransformId, TreeRecord, WarmRecord};
pub use records::{ReachState, RecordFlags, RecordKind};
pub(crate) use slab::Slab;
pub use store::{RecordSeed, SceneStore};
