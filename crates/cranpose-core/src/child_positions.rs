use crate::{ChildList, NodeId, collections::map::HashMap};

pub(crate) fn build_child_positions(children: &[NodeId]) -> HashMap<NodeId, usize> {
    let mut positions = HashMap::default();
    positions.reserve(children.len());
    for (index, &child) in children.iter().enumerate() {
        positions.insert(child, index);
    }
    positions
}

fn refresh_child_positions(
    current: &[NodeId],
    positions: &mut HashMap<NodeId, usize>,
    start: usize,
    end: usize,
) {
    if current.is_empty() || start >= current.len() {
        return;
    }
    let end = end.min(current.len() - 1);
    for (offset, &child) in current[start..=end].iter().enumerate() {
        positions.insert(child, start + offset);
    }
}

pub(crate) fn insert_child_into_diff_state(
    current: &mut ChildList,
    positions: &mut HashMap<NodeId, usize>,
    index: usize,
    child: NodeId,
) {
    let index = index.min(current.len());
    current.insert(index, child);
    refresh_child_positions(current, positions, index, current.len() - 1);
}

pub(crate) fn move_child_in_diff_state(
    current: &mut ChildList,
    positions: &mut HashMap<NodeId, usize>,
    from_index: usize,
    target_index: usize,
) -> usize {
    let child = current.remove(from_index);
    let to_index = target_index.min(current.len());
    current.insert(to_index, child);
    refresh_child_positions(
        current,
        positions,
        from_index.min(to_index),
        from_index.max(to_index),
    );
    to_index
}
