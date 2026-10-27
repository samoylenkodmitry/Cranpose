//! Which draw read which state: a draw scope's reads, observed while it
//! records, schedule the redraw of its node when the state changes.

use cranpose_core::{
    NodeId, SnapshotStateObserver, collections::map::HashSet, current_runtime_handle,
};

use super::{
    request_render_invalidation, require_current_app_context, schedule_draw_repass,
    schedule_draw_repass_for_app_context, schedule_layer_property_repass_for_app_context,
    with_draw_observer,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DrawObservationScope {
    node_id: NodeId,
    modifier_index: usize,
    command_index: usize,
    kind: DrawObservationKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum DrawObservationKind {
    Content,
    LayerProperties,
}

impl DrawObservationScope {
    pub(crate) fn new(node_id: NodeId, modifier_index: usize, command_index: usize) -> Self {
        Self {
            node_id,
            modifier_index,
            command_index,
            kind: DrawObservationKind::Content,
        }
    }

    pub(crate) fn layer_properties(node_id: NodeId, modifier_index: usize) -> Self {
        Self {
            node_id,
            modifier_index,
            command_index: 0,
            kind: DrawObservationKind::LayerProperties,
        }
    }
}

pub(super) fn new_draw_observer() -> SnapshotStateObserver {
    let observer = SnapshotStateObserver::new(|callback| {
        if let Some(runtime) = current_runtime_handle() {
            runtime.enqueue_ui_task(callback);
        } else {
            callback();
        }
    });
    observer.start();
    observer
}

struct CurrentDrawNodeGuard {
    previous: Option<NodeId>,
}

impl Drop for CurrentDrawNodeGuard {
    fn drop(&mut self) {
        crate::thread_state::cells(|cells| cells.current_draw_node.set(self.previous));
    }
}

pub(crate) fn observe_draw_reads<R>(scope: DrawObservationScope, block: impl FnOnce() -> R) -> R {
    let context = require_current_app_context("draw observer access");
    let context_id = context.id;
    let _guard = CurrentDrawNodeGuard {
        previous: crate::thread_state::cells(|cells| {
            cells.current_draw_node.replace(Some(scope.node_id))
        }),
    };
    context.draw_observer.observe_reads(
        scope,
        move |scope| match scope.kind {
            DrawObservationKind::Content => {
                schedule_draw_repass_for_app_context(context_id, scope.node_id);
            }
            DrawObservationKind::LayerProperties => {
                schedule_layer_property_repass_for_app_context(context_id, scope.node_id);
            }
        },
        block,
    )
}

/// The draw-phase animation contract: a draw closure that advances its own
/// spring or clock (a Cell no observation can see) must schedule the next
/// frame's re-record of ITS OWN node — a bare render invalidation only
/// re-presents the retained scene, which would freeze the animation
/// mid-flight. Callable only from inside a recording draw closure; anywhere
/// else it degrades to a plain frame request.
pub fn request_current_draw_redraw() {
    if let Some(node_id) = crate::thread_state::cells(|cells| cells.current_draw_node.get()) {
        schedule_draw_repass(node_id);
    }
    request_render_invalidation();
}

pub(crate) fn clear_draw_observations_for_node(node_id: NodeId) {
    with_draw_observer(|observer| {
        observer.clear_if(|scope| {
            scope
                .downcast_ref::<DrawObservationScope>()
                .is_some_and(|scope| scope.node_id == node_id)
        });
    });
}

/// Removes draw observations whose owners are absent from the retained scene.
pub fn prune_draw_observations_to_nodes(retained: &HashSet<NodeId>) {
    with_draw_observer(|observer| {
        observer.clear_if(|scope| {
            scope
                .downcast_ref::<DrawObservationScope>()
                .is_some_and(|scope| !retained.contains(&scope.node_id))
        });
    });
}
