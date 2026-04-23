use super::{
    AnchorRegistry, DeferredDrop, GroupRecord, NodeRecord, PayloadRecord, SlotLifecycleCoordinator,
    SlotPassMode,
};
use crate::{
    collections::map::HashMap,
    slot_storage::{GroupId, NodeRecordResult, SlotStorage, ValueSlotId},
    AnchorId, NodeId, Owned, ScopeId, SlotDebugSnapshot,
};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

mod metadata;
mod mutation;
mod session;
mod values;

pub(crate) use session::SlotWriteSessionState;

static NEXT_SLOT_STORAGE_ID: AtomicUsize = AtomicUsize::new(1);

pub(crate) struct SlotWriteSession<'a> {
    pub(super) table: &'a mut SlotTable,
    pub(in crate::slot) lifecycle: &'a mut SlotLifecycleCoordinator,
    pub(in crate::slot) state: &'a mut SlotWriteSessionState,
}

pub struct SlotTable {
    storage_id: usize,
    runtime_state: Option<Rc<crate::composer::ComposerRuntimeState>>,
    pub(super) groups: Vec<GroupRecord>,
    pub(super) payloads: Vec<PayloadRecord>,
    pub(super) nodes: Vec<NodeRecord>,
    pub(super) anchors: AnchorRegistry,
    pub(super) payload_anchor_to_location: HashMap<usize, (AnchorId, usize)>,
    pub(super) scope_anchor_to_group: HashMap<ScopeId, AnchorId>,
    next_group_generation: u32,
    pub(super) next_payload_anchor: usize,
}

impl SlotTable {
    pub fn new() -> Self {
        Self {
            storage_id: NEXT_SLOT_STORAGE_ID.fetch_add(1, Ordering::Relaxed),
            runtime_state: None,
            groups: Vec::new(),
            payloads: Vec::new(),
            nodes: Vec::new(),
            anchors: AnchorRegistry::new(),
            payload_anchor_to_location: HashMap::default(),
            scope_anchor_to_group: HashMap::default(),
            next_group_generation: 1,
            next_payload_anchor: 1,
        }
    }

    pub(crate) fn write_session<'a>(
        &'a mut self,
        lifecycle: &'a mut SlotLifecycleCoordinator,
        state: &'a mut SlotWriteSessionState,
        _mode: SlotPassMode,
    ) -> SlotWriteSession<'a> {
        SlotWriteSession {
            table: self,
            lifecycle,
            state,
        }
    }

    pub(crate) fn storage_id(&self) -> usize {
        self.storage_id
    }

    pub(crate) fn runtime_state(&self) -> Option<Rc<crate::composer::ComposerRuntimeState>> {
        self.runtime_state.clone()
    }

    pub(crate) fn bind_runtime_state(&mut self, state: &Rc<crate::composer::ComposerRuntimeState>) {
        self.runtime_state = Some(Rc::clone(state));
    }

    pub(crate) fn compact_storage(&mut self) {
        self.groups.shrink_to_fit();
        self.payloads.shrink_to_fit();
        self.nodes.shrink_to_fit();
        self.anchors.shrink_to_fit();
        self.payload_anchor_to_location.shrink_to_fit();
        self.scope_anchor_to_group.shrink_to_fit();
    }

    pub(crate) fn take_all_drops(&mut self) -> Vec<DeferredDrop> {
        let payload_count = self.payloads.len();
        let mut drops = Vec::with_capacity(self.groups.len() + payload_count);
        for payload in self.payloads.drain(..).rev() {
            drops.push(DeferredDrop::Value(payload.value));
        }
        self.groups.clear();
        self.nodes.clear();
        self.anchors.clear();
        self.payload_anchor_to_location.clear();
        self.scope_anchor_to_group.clear();
        drops
    }
}

impl Default for SlotTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SlotStorage for SlotWriteSession<'_> {
    type Group = GroupId;
    type ValueSlot = ValueSlotId;

    fn begin_group(
        &mut self,
        input: crate::slot_storage::BeginGroupInput<super::DetachedSubtree>,
    ) -> crate::slot_storage::GroupStart<Self::Group> {
        SlotWriteSession::begin_group(self, input)
    }

    fn finish_group_body(&mut self) -> super::FinishGroupResult {
        SlotWriteSession::finish_group_body(self)
    }

    fn end_group(&mut self) {
        SlotWriteSession::end_group(self);
    }

    fn skip_group(&mut self) {
        SlotWriteSession::skip_group(self);
    }

    fn set_group_scope(&mut self, group: Self::Group, scope: ScopeId) {
        SlotWriteSession::set_group_scope(self, group, scope);
    }

    fn begin_recompose_at_scope(&mut self, scope: ScopeId) -> Option<Self::Group> {
        SlotWriteSession::begin_recompose_at_scope(self, scope)
    }

    fn end_recompose(&mut self) {
        SlotWriteSession::end_recompose(self);
    }

    fn value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Self::ValueSlot {
        SlotWriteSession::value_slot(self, init)
    }

    fn read_value<T: 'static>(&self, slot: Self::ValueSlot) -> &T {
        self.table.read_value(slot)
    }

    fn read_value_mut<T: 'static>(&mut self, slot: Self::ValueSlot) -> &mut T {
        self.table.read_value_mut(slot)
    }

    fn write_value<T: 'static>(&mut self, slot: Self::ValueSlot, value: T) {
        self.table.write_value(slot, value);
    }

    fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        SlotWriteSession::remember(self, init)
    }

    fn record_node(&mut self, id: NodeId, generation: u32) -> NodeRecordResult {
        SlotWriteSession::record_node(self, id, generation)
    }

    fn nodes_in_current_group(&self) -> Vec<NodeId> {
        SlotWriteSession::nodes_in_current_group(self)
    }

    fn validate(&self) -> Result<(), super::SlotInvariantError> {
        self.table.validate()
    }

    fn debug_snapshot(&self) -> SlotDebugSnapshot {
        self.table.debug_snapshot()
    }
}
