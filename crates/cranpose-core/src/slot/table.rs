use super::{
    AnchorRegistry, DeferredDrop, GroupRecord, NodeRecord, PayloadAnchorRegistry, PayloadRecord,
    ScopeIndex, SlotLifecycleCoordinator, SlotTableMutationDebugStats, SlotWriteSessionState,
};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

mod metadata;
mod mutation;
mod values;

#[cfg(any(test, debug_assertions))]
pub(in crate::slot) use mutation::SlotMutationGuard;

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
    pub(super) payload_anchors: PayloadAnchorRegistry,
    pub(super) scope_index: ScopeIndex,
    pub(super) mutation_debug_stats: SlotTableMutationDebugStats,
    next_group_generation: u32,
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
            payload_anchors: PayloadAnchorRegistry::new(),
            scope_index: ScopeIndex::new(),
            mutation_debug_stats: SlotTableMutationDebugStats::default(),
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
        self.payload_anchors.shrink_to_fit();
        self.scope_index.shrink_to_fit();
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
