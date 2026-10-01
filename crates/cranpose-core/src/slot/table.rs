use std::rc::Rc;

use super::{
    AnchorRegistry, DeferredDrop, GroupRecord, MovableIndex, NodeRecord, PayloadAnchorRegistry,
    SlotLifecycleCoordinator, SlotWriteSessionState, debug::SlotTableDiagnostics,
    growth::GrowthSlack, payload_store::PayloadStore,
};

mod metadata;
mod mutation;
mod values;

#[cfg(test)]
pub(crate) use values::ValueSlotError;

struct SlotStorageIdentity(Rc<()>);

impl SlotStorageIdentity {
    fn new() -> Self {
        Self(Rc::new(()))
    }

    fn id(&self) -> usize {
        Rc::as_ptr(&self.0).addr()
    }
}

pub(crate) struct SlotWriteSession<'a> {
    pub(super) table: &'a mut SlotTable,
    pub(in crate::slot) lifecycle: &'a mut SlotLifecycleCoordinator,
    pub(in crate::slot) state: &'a mut SlotWriteSessionState,
}

pub struct SlotTable {
    storage_id: SlotStorageIdentity,
    pub(super) groups: Vec<GroupRecord>,
    pub(super) payloads: PayloadStore,
    pub(super) nodes: Vec<NodeRecord>,
    pub(super) anchors: AnchorRegistry,
    pub(super) payload_anchors: PayloadAnchorRegistry,
    pub(super) movables: MovableIndex,
    pub(super) diagnostics: SlotTableDiagnostics,
    next_group_generation: u32,
}

impl SlotTable {
    pub fn new() -> Self {
        Self {
            storage_id: SlotStorageIdentity::new(),
            groups: Vec::new(),
            payloads: PayloadStore::default(),
            nodes: Vec::new(),
            anchors: AnchorRegistry::new(),
            payload_anchors: PayloadAnchorRegistry::new(),
            movables: MovableIndex::default(),
            diagnostics: SlotTableDiagnostics::default(),
            next_group_generation: 1,
        }
    }

    pub(crate) fn write_session<'a>(
        &'a mut self,
        lifecycle: &'a mut SlotLifecycleCoordinator,
        state: &'a mut SlotWriteSessionState,
    ) -> SlotWriteSession<'a> {
        SlotWriteSession {
            table: self,
            lifecycle,
            state,
        }
    }

    pub(crate) fn storage_id(&self) -> usize {
        self.storage_id.id()
    }

    #[cfg(test)]
    pub(in crate::slot) fn set_next_group_generation_for_test(&mut self, generation: u32) {
        self.next_group_generation = generation;
    }

    #[cfg(test)]
    pub(in crate::slot) fn set_next_group_anchor_for_test(&mut self, next_anchor: usize) {
        self.anchors.set_next_anchor_for_test(next_anchor);
    }

    #[cfg(test)]
    pub(in crate::slot) fn set_next_payload_anchor_for_test(&mut self, next_anchor: usize) {
        self.payload_anchors.set_next_id_for_test(next_anchor);
    }

    pub(crate) fn compact_storage(&mut self) {
        self.groups.shrink_to_fit();
        self.payloads.compact();
        self.nodes.shrink_to_fit();
        self.anchors.shrink_to_fit();
        self.payload_anchors.shrink_to_fit();
        self.movables.shrink_to_fit();
    }

    pub(crate) fn storage_capacity(&self) -> usize {
        self.groups.capacity() + self.payloads.capacity() + self.nodes.capacity()
    }

    pub(crate) fn trim_growth_slack(&mut self) {
        self.groups.trim_growth_slack();
        self.payloads.trim_growth_slack();
        self.nodes.trim_growth_slack();
        self.anchors.trim_growth_slack();
        self.payload_anchors.trim_growth_slack();
    }

    pub(crate) fn take_effect_drops(&mut self) -> Vec<DeferredDrop> {
        let mut drops = Vec::new();
        self.payloads.for_each_mut(|payload| {
            if let Some(fresh) = payload.payload_type.fresh {
                let old = std::mem::replace(&mut payload.value, fresh());
                drops.push(DeferredDrop::payload(old));
            }
        });
        drops
    }

    pub(crate) fn take_all_drops(&mut self) -> Vec<DeferredDrop> {
        let mut drops = Vec::with_capacity(self.payloads.len());
        self.payloads
            .drain_rev(|payload| drops.push(payload.into_deferred_drop()));
        self.groups.clear();
        self.nodes.clear();
        self.anchors.clear();
        self.payload_anchors.clear();
        self.movables.clear();
        drops
    }
}

impl Default for SlotTable {
    fn default() -> Self {
        Self::new()
    }
}
