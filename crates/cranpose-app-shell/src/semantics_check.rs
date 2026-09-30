use cranpose_core::{MemoryApplier, NodeId};
use cranpose_ui::{SemanticsNode, SemanticsTree};

pub(crate) fn assert_matches_rebuild(
    applier: &mut MemoryApplier,
    root: NodeId,
    kept: Option<&SemanticsTree>,
) {
    let rebuilt = match cranpose_ui::build_semantics_tree_from_applier(applier, root) {
        Ok(rebuilt) => rebuilt,
        Err(err) => {
            log::debug!("could not rebuild the semantics tree to check an update: {err}");
            return;
        }
    };
    if kept == rebuilt.as_ref() || format!("{kept:?}") == format!("{rebuilt:?}") {
        return;
    }
    let difference = match (kept, rebuilt.as_ref()) {
        (Some(kept), Some(rebuilt)) => first_difference(kept.root(), rebuilt.root())
            .unwrap_or_else(|| "the top modal".to_owned()),
        _ => format!(
            "the root: kept {}, rebuilt {}",
            kept.is_some(),
            rebuilt.is_some()
        ),
    };
    panic!("an updated semantics tree differs from a rebuilt one at {difference}");
}

fn first_difference(kept: &SemanticsNode, rebuilt: &SemanticsNode) -> Option<String> {
    let node_only = |node: &SemanticsNode| SemanticsNode {
        children: Vec::new(),
        ..node.clone()
    };
    let (kept_node, rebuilt_node) = (node_only(kept), node_only(rebuilt));
    if kept_node != rebuilt_node && format!("{kept_node:?}") != format!("{rebuilt_node:?}") {
        return Some(format!(
            "node #{}: kept {kept_node:?}, rebuilt {rebuilt_node:?}",
            kept.node_id
        ));
    }
    let ids = |node: &SemanticsNode| {
        node.children
            .iter()
            .map(|child| child.node_id)
            .collect::<Vec<_>>()
    };
    if ids(kept) != ids(rebuilt) {
        return Some(format!(
            "the children of #{}: kept {:?}, rebuilt {:?}",
            kept.node_id,
            ids(kept),
            ids(rebuilt)
        ));
    }
    kept.children
        .iter()
        .zip(&rebuilt.children)
        .find_map(|(kept, rebuilt)| first_difference(kept, rebuilt))
}

#[cfg(test)]
#[path = "tests/semantics_check_tests.rs"]
mod tests;
