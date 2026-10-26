//! The measurement cache of a layout node and its dirty flags.

use std::{
    cell::{Cell, RefCell},
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_ui_layout::Constraints;

use super::layout_node::{DescendantDirt, LayoutDirt};
use crate::layout::MeasuredNode;

#[derive(Clone)]
struct MeasurementCacheEntry {
    constraints: Constraints,
    measured: Rc<MeasuredNode>,
    /// The other incoming constraints the measurement holds for.
    hold: Option<cranpose_ui_layout::ConstraintsHold>,
    /// Whether no child's size entered the measurement: see
    /// [`cranpose_ui_layout::MeasurePolicy::ignores_child_sizes`].
    ignores_child_sizes: bool,
}

/// Selects which intrinsic size of a node to query, together with the size on the other axis.
#[derive(Clone, Copy, Debug)]
pub enum IntrinsicKind {
    /// Minimum intrinsic width for the given height, in logical pixels.
    MinWidth(f32),
    /// Maximum intrinsic width for the given height, in logical pixels.
    MaxWidth(f32),
    /// Minimum intrinsic height for the given width, in logical pixels.
    MinHeight(f32),
    /// Maximum intrinsic height for the given width, in logical pixels.
    MaxHeight(f32),
}

impl IntrinsicKind {
    fn discriminant(&self) -> u8 {
        match self {
            Self::MinWidth(_) => 0,
            Self::MaxWidth(_) => 1,
            Self::MinHeight(_) => 2,
            Self::MaxHeight(_) => 3,
        }
    }

    fn value_bits(&self) -> u32 {
        match self {
            Self::MinWidth(value)
            | Self::MaxWidth(value)
            | Self::MinHeight(value)
            | Self::MaxHeight(value) => value.to_bits(),
        }
    }
}

impl PartialEq for IntrinsicKind {
    fn eq(&self, other: &Self) -> bool {
        self.discriminant() == other.discriminant() && self.value_bits() == other.value_bits()
    }
}

impl Eq for IntrinsicKind {}

impl Hash for IntrinsicKind {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.discriminant().hash(state);
        self.value_bits().hash(state);
    }
}

#[derive(Default)]
struct NodeCacheState {
    epoch: u64,
    measurement: Option<MeasurementCacheEntry>,
    /// The measurement the cache last let go of. It is never served: the
    /// node's next measure takes it to fill its memory in place of new
    /// allocations.
    spare: Option<Rc<MeasuredNode>>,
    intrinsics: Vec<(IntrinsicKind, f32)>,
    /// Whether a parent read intrinsic sizes since the cache was cleared.
    intrinsics_read: bool,
}

impl NodeCacheState {
    fn retire_measurement(&mut self) {
        if let Some(entry) = self.measurement.take() {
            self.spare = Some(entry.measured);
        }
    }
}

// What a node's layout shares with its parent's record of the node: the
// measurement cache and the node's dirty flags, which the parent's keep walk
// reads there without looking the node up.
#[derive(Default)]
pub(crate) struct NodeCache {
    cache: RefCell<NodeCacheState>,
    dirt: Cell<LayoutDirt>,
}

impl NodeCache {
    /// The flags of the node whose cache this is: dirty for a node that
    /// keeps its flags elsewhere.
    pub(crate) fn dirt(&self) -> LayoutDirt {
        self.dirt.get()
    }

    pub(super) fn set_dirt(&self, dirt: LayoutDirt) {
        self.dirt.set(dirt);
    }

    /// Clears the node's layout flag and its descendants' flags, and returns
    /// the flags as they were, which [`Self::restore_dirt`] puts back.
    pub(crate) fn take_layout_dirt(&self) -> LayoutDirt {
        let dirt = self.dirt();
        self.clear_layout_dirt();
        dirt
    }

    /// Puts back flags [`Self::take_layout_dirt`] took.
    pub(crate) fn restore_dirt(&self, dirt: LayoutDirt) {
        self.set_dirt(dirt);
    }

    /// Clears the node's layout flag and its descendants' flags: the pass
    /// laid it out.
    pub(crate) fn clear_layout_dirt(&self) {
        self.set_dirt(LayoutDirt {
            layout: false,
            descendant: DescendantDirt::default(),
            ..self.dirt()
        });
    }

    pub(crate) fn clear(&self) {
        let mut state = self.cache.borrow_mut();
        state.retire_measurement();
        state.intrinsics.clear();
        state.intrinsics_read = false;
        state.epoch = 0;
    }

    pub(crate) fn activate(&self, epoch: u64) {
        let mut state = self.cache.borrow_mut();
        if state.epoch != epoch {
            state.retire_measurement();
            state.intrinsics.clear();
            state.intrinsics_read = false;
            state.epoch = epoch;
        }
    }

    /// The node's last measurement, whatever constraints it was made under:
    /// the one the cache holds, or else the one it last let go of.
    pub(crate) fn held_measurement(&self) -> Option<Rc<MeasuredNode>> {
        let state = self.cache.borrow();
        state
            .measurement
            .as_ref()
            .map(|entry| &entry.measured)
            .or(state.spare.as_ref())
            .cloned()
    }

    /// Takes the measurement the cache last let go of, or else the one it
    /// holds, for a measure of the node that replaces it.
    pub(crate) fn take_previous_measurement(&self) -> Option<Rc<MeasuredNode>> {
        let mut state = self.cache.borrow_mut();
        state
            .spare
            .take()
            .or_else(|| state.measurement.take().map(|entry| entry.measured))
    }

    /// Drops the intrinsic sizes, which depend on every node below: one of
    /// them changed. A parent's earlier read still counts for
    /// [`Self::has_intrinsics`].
    pub(crate) fn forget_intrinsics(&self) {
        self.cache.borrow_mut().intrinsics.clear();
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.cache.borrow().epoch
    }

    /// The measurement the cache holds for `constraints`: one made under
    /// them, or one whose hold contains them. A caller asks only while the
    /// node and everything below it are clean.
    pub(crate) fn get_measurement(&self, constraints: Constraints) -> Option<Rc<MeasuredNode>> {
        let state = self.cache.borrow();
        state
            .measurement
            .as_ref()
            .filter(|entry| {
                entry.constraints == constraints
                    || entry.hold.is_some_and(|hold| hold.contains(constraints))
            })
            .map(|entry| Rc::clone(&entry.measured))
    }

    /// The constraints of the measurement the cache holds.
    pub(crate) fn measured_constraints(&self) -> Option<Constraints> {
        self.cache
            .borrow()
            .measurement
            .as_ref()
            .map(|entry| entry.constraints)
    }

    /// Whether a parent read intrinsic sizes from this cache.
    pub(crate) fn has_intrinsics(&self) -> bool {
        self.cache.borrow().intrinsics_read
    }

    /// The hold of the measurement the cache has, when it is `measured`.
    pub(crate) fn hold_of(
        &self,
        measured: &Rc<MeasuredNode>,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        self.cache
            .borrow()
            .measurement
            .as_ref()
            .filter(|entry| Rc::ptr_eq(&entry.measured, measured))
            .and_then(|entry| entry.hold)
    }

    /// Stores `measured` for `constraints`. The hold of a measurement the
    /// cache already has stays with it, as does whether it ignores its
    /// children's sizes.
    pub(crate) fn store_measurement(&self, constraints: Constraints, measured: Rc<MeasuredNode>) {
        let mut state = self.cache.borrow_mut();
        let (hold, ignores_child_sizes) = state
            .measurement
            .as_ref()
            .filter(|entry| Rc::ptr_eq(&entry.measured, &measured))
            .map_or((None, false), |entry| {
                (entry.hold, entry.ignores_child_sizes)
            });
        state.measurement = Some(MeasurementCacheEntry {
            constraints,
            measured,
            hold,
            ignores_child_sizes,
        });
    }

    /// Stores `measured` for `constraints` and the other constraints `hold`
    /// says it holds for.
    pub(crate) fn store_held_measurement(
        &self,
        constraints: Constraints,
        measured: Rc<MeasuredNode>,
        hold: Option<cranpose_ui_layout::ConstraintsHold>,
        ignores_child_sizes: bool,
    ) {
        self.cache.borrow_mut().measurement = Some(MeasurementCacheEntry {
            constraints,
            measured,
            hold,
            ignores_child_sizes,
        });
    }

    /// Stores `measured`, a measurement the cache has, for `constraints`
    /// alone: a child of it measured to another size, which the other
    /// constraints its hold names may not take.
    pub(crate) fn store_resized_measurement(
        &self,
        constraints: Constraints,
        measured: Rc<MeasuredNode>,
    ) {
        self.store_held_measurement(constraints, measured, None, true);
    }

    /// Runs `read` on the measurement the cache holds, with whether it
    /// ignores its children's sizes; `None` while it holds none.
    pub(crate) fn read_measurement<R>(
        &self,
        read: impl FnOnce(&MeasuredNode, bool) -> R,
    ) -> Option<R> {
        let state = self.cache.borrow();
        state
            .measurement
            .as_ref()
            .map(|entry| read(&entry.measured, entry.ignores_child_sizes))
    }

    /// Whether `measured`, the measurement the cache has, ignores its
    /// children's sizes.
    pub(crate) fn ignores_child_sizes_of(&self, measured: &Rc<MeasuredNode>) -> bool {
        self.cache
            .borrow()
            .measurement
            .as_ref()
            .is_some_and(|entry| entry.ignores_child_sizes && Rc::ptr_eq(&entry.measured, measured))
    }

    pub(crate) fn get_intrinsic(&self, kind: &IntrinsicKind) -> Option<f32> {
        let state = self.cache.borrow();
        state
            .intrinsics
            .iter()
            .find(|(stored_kind, _)| stored_kind == kind)
            .map(|(_, value)| *value)
    }

    pub(crate) fn store_intrinsic(&self, kind: IntrinsicKind, value: f32) {
        let mut state = self.cache.borrow_mut();
        state.intrinsics_read = true;
        if let Some((_, existing)) = state
            .intrinsics
            .iter_mut()
            .find(|(stored_kind, _)| stored_kind == &kind)
        {
            *existing = value;
        } else {
            state.intrinsics.push((kind, value));
        }
    }
}
