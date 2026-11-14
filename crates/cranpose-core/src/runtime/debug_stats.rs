/// Sizes of a runtime's queues and tables, for leak and memory checks. A `_len` field counts
/// entries and a `_cap` field the capacity reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimeDebugStats {
    /// Node updates queued and not yet applied.
    pub node_updates_len: usize,
    /// Capacity reserved for the node updates.
    pub node_updates_cap: usize,
    /// Recompose scopes that were invalidated and not yet recomposed.
    pub invalid_scopes_len: usize,
    /// Entries in the queue of invalidated scopes.
    pub scope_queue_len: usize,
    /// Capacity reserved for the invalid scope queue.
    pub scope_queue_cap: usize,
    /// Frame callbacks waiting for the next frame.
    pub frame_callbacks_len: usize,
    /// Capacity reserved for the frame callbacks.
    pub frame_callbacks_cap: usize,
    /// UI-thread closures queued by `enqueue_ui_task` and not yet run.
    pub local_tasks_len: usize,
    /// Capacity reserved for the local tasks.
    pub local_tasks_cap: usize,
    /// UI continuations registered and waiting for a value.
    pub ui_conts_len: usize,
    /// Capacity reserved for the UI continuations.
    pub ui_conts_cap: usize,
    /// Futures from `spawn_ui` that have not finished.
    pub tasks_len: usize,
    /// Capacity reserved for the spawned tasks.
    pub tasks_cap: usize,
    /// States kept alive on behalf of the runtime itself, not of a Rust owner.
    pub external_state_owners_len: usize,
    /// Capacity reserved for the external state owners.
    pub external_state_owners_cap: usize,
    /// Messages posted from other threads that the UI thread has not run yet.
    pub ui_dispatcher_pending: usize,
}
