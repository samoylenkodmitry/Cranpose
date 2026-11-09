// The fixed-size records of a scene store, one of each per row: what every
// walk reads, what damage and culling read, and what the scene update reads.

use std::num::NonZeroU32;

use cranpose_core::{NodeId, RecordId, SceneUpdate};
use cranpose_ui::Rect;

use super::{Reach, Span};
use crate::graph::ProjectiveTransform;

/// What a record draws for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RecordKind {
    /// The layer of a layout node, with its children.
    Layer,
    /// The layer around a node's layer that holds the draws chained before
    /// the node's graphics layer.
    Wrapper,
    /// Part of one draw command's draws that a `DrawScope` transform or clip
    /// groups, in the phase they paint in among its node's own draws.
    DrawGroup,
    /// A layout node drawn in its parent's layer: a leaf with no layer
    /// properties, pointer handling or window origins to publish.
    Leaf,
}

// Which reach a record's rect stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum ReachState {
    Empty,
    Finite,
    Unknown,
}

/// Facts about a record that its walks branch on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordFlags(u16);

impl std::ops::BitOr for RecordFlags {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl RecordFlags {
    /// The record cuts its content to its layer bounds.
    pub const CLIP_TO_BOUNDS: Self = Self(1);
    /// The record lies in a motion context such as an animated scroll.
    pub const MOTION_ANIMATED: Self = Self(1 << 1);
    /// The record moves its content as one translated block.
    pub const TRANSLATED_CONTENT: Self = Self(1 << 2);
    /// The record's node publishes live window origins: a text field's
    /// popup anchor, a scroll container's viewport rect.
    pub const ORIGIN_SINKS: Self = Self(1 << 3);
    /// The record casts an elevation shadow.
    pub const CASTS_SHADOW: Self = Self(1 << 4);
    /// The record draws its draws after its children: a draw group of the
    /// draws after the children.
    pub const AFTER_CHILDREN: Self = Self(1 << 5);
    /// The record has a graphics layer of its own.
    pub const HAS_GRAPHICS_LAYER: Self = Self(1 << 6);
    /// The record's layer bounds differ from its node rect.
    pub const HAS_LAYER_BOUNDS: Self = Self(1 << 7);
    // A change beneath the record left its reach to be worked out again.
    pub(crate) const REACH_STALE: Self = Self(1 << 8);
    // The update in progress made the record.
    pub(crate) const INSERTED: Self = Self(1 << 9);
    // The update in progress may have changed the record's own draws, which
    // its end compares with what they were.
    pub(crate) const REDRAW_PENDING: Self = Self(1 << 10);
    // The record keeps properties in the store's side slab.
    pub(crate) const HAS_PROPS: Self = Self(1 << 11);
    // The flags the store keeps itself, which `SceneStore::set_flags` leaves.
    pub(crate) const STORE_KEPT: Self = Self(
        Self::HAS_PROPS.0
            | Self::HAS_GRAPHICS_LAYER.0
            | Self::HAS_LAYER_BOUNDS.0
            | Self::REACH_STALE.0
            | Self::INSERTED.0
            | Self::REDRAW_PENDING.0,
    );

    /// Whether every flag of `other` is set.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// These flags with those of `other` set.
    #[must_use]
    pub fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    // These flags of those of `other`.
    pub(crate) fn only(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// These flags with those of `other` cleared.
    #[must_use]
    pub fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(C, align(32))]
pub(crate) struct TreeRecord {
    pub(crate) parent: Option<RecordId>,
    pub(crate) items: Span,
    pub(crate) kind: RecordKind,
    pub(crate) reach_state: ReachState,
    pub(crate) flags: RecordFlags,
    pub(crate) content_rev: SceneUpdate,
    pub(crate) place_rev: SceneUpdate,
    pub(crate) subtree_rev: SceneUpdate,
    pub(crate) transform: Option<TransformId>,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C, align(32))]
pub(crate) struct GeoRecord {
    pub(crate) placement: [f32; 2],
    pub(crate) reach: Rect,
    pub(crate) size: [f32; 2],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WarmRecord {
    pub(crate) node: Option<NodeId>,
    pub(crate) props: Option<PropsId>,
    pub(crate) graphics_layer: Option<GraphicsLayerId>,
    pub(crate) content_offset: [f32; 2],
    // The marks of the record's own draws as an update last left them.
    pub(crate) own_marks: Span,
    // Where the record's own draws reach, beside its children's.
    pub(crate) own_reach: Reach,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PropsId(NonZeroU32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TransformId(NonZeroU32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GraphicsLayerId(NonZeroU32);

macro_rules! slot_id {
    ($name:ident) => {
        impl $name {
            pub(crate) fn from_index(index: usize) -> Self {
                Self(
                    u32::try_from(index + 1)
                        .ok()
                        .and_then(NonZeroU32::new)
                        .expect("a store holds fewer than 2^32 - 1 entries of a kind"),
                )
            }

            pub(crate) fn index(self) -> usize {
                (self.0.get() - 1) as usize
            }
        }
    };
}

slot_id!(PropsId);
slot_id!(TransformId);
slot_id!(GraphicsLayerId);

impl GeoRecord {
    pub(crate) fn node_rect(&self) -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: self.size[0],
            height: self.size[1],
        }
    }
}

const _: () = assert!(
    size_of::<TreeRecord>() == 32,
    "a tree record fills half a cache line"
);
const _: () = assert!(
    size_of::<GeoRecord>() == 32,
    "a geometry record fills half a cache line"
);

// Whether two pairs hold the same numbers, bit for bit.
pub(super) fn same_pair(a: [f32; 2], b: [f32; 2]) -> bool {
    a[0].to_bits() == b[0].to_bits() && a[1].to_bits() == b[1].to_bits()
}

// Whether two transforms hold the same numbers, bit for bit.
pub(super) fn same_bits(a: ProjectiveTransform, b: ProjectiveTransform) -> bool {
    let (a, b) = (a.matrix(), b.matrix());
    a.as_flattened()
        .iter()
        .zip(b.as_flattened())
        .all(|(a, b)| a.to_bits() == b.to_bits())
}
