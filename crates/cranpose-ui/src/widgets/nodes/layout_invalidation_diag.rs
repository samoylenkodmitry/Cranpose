// The `CRANPOSE_LAYOUT_INVALIDATION_DISPATCH_DIAG` log of the layout
// invalidations a node's modifier chain dispatches: every node's for `all`,
// one node's for its id.

use cranpose_core::NodeId;
use cranpose_foundation::{ModifierInvalidation, NodeCapabilities};

use crate::modifier::ModifierChainHandle;

#[derive(Clone, Copy)]
enum LayoutInvalidationDispatchDiag {
    Disabled,
    All,
    Node(NodeId),
}

fn layout_invalidation_dispatch_diag() -> LayoutInvalidationDispatchDiag {
    cranpose_core::DIAGNOSTICS
        .layout_invalidation_dispatch_diag
        .with(|value| match value {
            None => LayoutInvalidationDispatchDiag::Disabled,
            Some("all") => LayoutInvalidationDispatchDiag::All,
            Some(value) => value.parse::<NodeId>().map_or(
                LayoutInvalidationDispatchDiag::Disabled,
                LayoutInvalidationDispatchDiag::Node,
            ),
        })
}

pub(super) fn log_layout_invalidation_dispatch(
    id: NodeId,
    invalidation: &ModifierInvalidation,
    curr_caps: NodeCapabilities,
    prev_caps: NodeCapabilities,
    chain: &ModifierChainHandle,
) {
    let enabled = match layout_invalidation_dispatch_diag() {
        LayoutInvalidationDispatchDiag::Disabled => false,
        LayoutInvalidationDispatchDiag::All => true,
        LayoutInvalidationDispatchDiag::Node(target) => target == id,
    };
    if enabled {
        let modifier = chain.modifier();
        log::warn!(
            "[layout-invalidation-dispatch] node={id} invalidation={invalidation:?} curr_caps={curr_caps:?} prev_caps={prev_caps:?} modifier={modifier}"
        );
    }
}
