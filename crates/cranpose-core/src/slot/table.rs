use super::debug::SlotTableDiagnostics;
use super::{
    AnchorRegistry, DeferredDrop, GroupRecord, NodeRecord, PayloadAnchorRegistry, PayloadRecord,
    ScopeIndex, SlotLifecycleCoordinator, SlotWriteSessionState,
};
use std::rc::Rc;

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
    pub(super) payloads: Vec<PayloadRecord>,
    pub(super) nodes: Vec<NodeRecord>,
    pub(super) anchors: AnchorRegistry,
    pub(super) payload_anchors: PayloadAnchorRegistry,
    pub(super) scope_index: ScopeIndex,
    pub(super) diagnostics: SlotTableDiagnostics,
    next_group_generation: u32,
}

impl SlotTable {
    pub fn new() -> Self {
        Self {
            storage_id: SlotStorageIdentity::new(),
            groups: Vec::new(),
            payloads: Vec::new(),
            nodes: Vec::new(),
            anchors: AnchorRegistry::new(),
            payload_anchors: PayloadAnchorRegistry::new(),
            scope_index: ScopeIndex::new(),
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
        self.payloads.shrink_to_fit();
        self.nodes.shrink_to_fit();
        self.anchors.shrink_to_fit();
        self.payload_anchors.shrink_to_fit();
        self.scope_index.shrink_to_fit();
    }

    pub(crate) fn take_effect_drops(&mut self) -> Vec<DeferredDrop> {
        let mut drops = Vec::new();
        for payload in self.payloads.iter_mut() {
            let Some(fresh) = payload.fresh else {
                continue;
            };
            let old = std::mem::replace(&mut payload.value, fresh());
            drops.push(DeferredDrop::payload(old));
        }
        drops
    }

    pub(crate) fn take_all_drops(&mut self) -> Vec<DeferredDrop> {
        let payload_count = self.payloads.len();
        let mut drops = Vec::with_capacity(payload_count);
        for payload in self.payloads.drain(..).rev() {
            drops.push(payload.into_deferred_drop());
        }
        self.groups.clear();
        self.nodes.clear();
        self.anchors.clear();
        self.payload_anchors.clear();
        self.scope_index.clear();
        drops
    }
}

impl Default for SlotTable {
    fn default() -> Self {
        Self::new()
    }
}
