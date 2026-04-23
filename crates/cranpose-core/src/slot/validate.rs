use super::{AnchorState, NodeLifecycle, SlotLifecycleCoordinator, SlotTable};
use crate::{collections::map::HashMap, slot_storage::GroupKey, AnchorId, NodeId, ScopeId};

#[cfg_attr(not(debug_assertions), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SlotInvariantError {
    GroupAnchorCountMismatch {
        expected: usize,
        actual: usize,
    },
    AnchorMismatch {
        anchor: AnchorId,
        expected: usize,
        actual: Option<AnchorState>,
    },
    InvalidParent {
        group_index: usize,
        expected: AnchorId,
        actual: AnchorId,
    },
    BadDepth {
        group_index: usize,
        expected: u32,
        actual: u32,
    },
    BadSubtreeLen {
        group_index: usize,
        expected: u32,
        actual: u32,
    },
    BadSubtreeNodeCount {
        group_index: usize,
        expected: u32,
        actual: u32,
    },
    PayloadStartMismatch {
        group_index: usize,
        expected: usize,
        actual: usize,
    },
    PayloadOutOfRange {
        group_index: usize,
        start: usize,
        len: usize,
        payload_count: usize,
    },
    PayloadAnchorCountMismatch {
        expected: usize,
        actual: usize,
    },
    ScopeIndexCountMismatch {
        expected: usize,
        actual: usize,
    },
    PayloadOwnerMismatch {
        payload_anchor: usize,
        expected: AnchorId,
        actual: AnchorId,
    },
    PayloadLocationMismatch {
        payload_anchor: usize,
        expected: (AnchorId, usize),
        actual: Option<(AnchorId, usize)>,
    },
    NodeStartMismatch {
        group_index: usize,
        expected: usize,
        actual: usize,
    },
    NodeOutOfRange {
        group_index: usize,
        start: usize,
        len: usize,
        node_count: usize,
    },
    NodeCountMismatch {
        expected: usize,
        actual: usize,
    },
    NodeOwnerMismatch {
        node_id: NodeId,
        expected: AnchorId,
        actual: AnchorId,
    },
    NodeLifecycleMismatch {
        node_id: NodeId,
        expected: NodeLifecycle,
        actual: NodeLifecycle,
    },
    DuplicateSiblingKey {
        parent_anchor: AnchorId,
        key: GroupKey,
    },
    ScopeIndexMismatch {
        scope_id: ScopeId,
        expected: AnchorId,
        actual: Option<AnchorId>,
    },
    RetainedSubtreeAnchorStillActive {
        root_key: GroupKey,
        anchor: AnchorId,
        active_index: usize,
    },
    RetainedRootHasActiveParent {
        root_key: GroupKey,
        parent_anchor: AnchorId,
    },
    RetainedNodeLifecycleMismatch {
        root_key: GroupKey,
        node_id: NodeId,
        actual: NodeLifecycle,
    },
    WriterFrameOutOfBounds {
        frame_index: usize,
        group_anchor: AnchorId,
        field: &'static str,
        value: usize,
        min: usize,
        max: usize,
    },
}

impl SlotTable {
    pub(crate) fn debug_verify(&self, _lifecycle: Option<&SlotLifecycleCoordinator>) {
        if crate::slot_validation_diagnostics_enabled() {
            if let Err(err) = self.validate() {
                panic!("slot table invariant violation: {err:?}");
            }
        }
    }

    pub(crate) fn validate(&self) -> Result<(), SlotInvariantError> {
        let mut stack: Vec<(AnchorId, usize)> = Vec::new();
        let mut sibling_keys: HashMap<(AnchorId, GroupKey), usize> = HashMap::default();
        let payload_count = self.total_payload_count();
        let mut expected_payload_start = 0usize;
        let node_count = self.total_node_count();
        let mut expected_node_start = 0usize;
        let scope_count = self
            .groups
            .iter()
            .filter(|group| group.scope_id.is_some())
            .count();

        for (index, group) in self.groups.iter().enumerate() {
            match self.anchors.state(group.anchor) {
                Some(AnchorState::Active(actual)) if actual == index => {}
                actual => {
                    return Err(SlotInvariantError::AnchorMismatch {
                        anchor: group.anchor,
                        expected: index,
                        actual,
                    });
                }
            }

            while let Some((_, end)) = stack.last() {
                if *end <= index {
                    stack.pop();
                } else {
                    break;
                }
            }

            let expected_parent = stack
                .last()
                .map(|(anchor, _)| *anchor)
                .unwrap_or(AnchorId::INVALID);
            if group.parent_anchor != expected_parent {
                return Err(SlotInvariantError::InvalidParent {
                    group_index: index,
                    expected: expected_parent,
                    actual: group.parent_anchor,
                });
            }

            let expected_depth = stack.len() as u32;
            if group.depth != expected_depth {
                return Err(SlotInvariantError::BadDepth {
                    group_index: index,
                    expected: expected_depth,
                    actual: group.depth,
                });
            }

            let subtree_end = index + group.subtree_len as usize;
            if subtree_end == index || subtree_end > self.groups.len() {
                return Err(SlotInvariantError::BadSubtreeLen {
                    group_index: index,
                    expected: 0,
                    actual: group.subtree_len,
                });
            }

            if sibling_keys
                .insert((group.parent_anchor, group.key), index)
                .is_some()
            {
                return Err(SlotInvariantError::DuplicateSiblingKey {
                    parent_anchor: group.parent_anchor,
                    key: group.key,
                });
            }

            let payload_start = group.payload_start as usize;
            if payload_start != expected_payload_start {
                return Err(SlotInvariantError::PayloadStartMismatch {
                    group_index: index,
                    expected: expected_payload_start,
                    actual: payload_start,
                });
            }
            let Some(payload_range) = self.group_payload_range_checked(index) else {
                return Err(SlotInvariantError::PayloadOutOfRange {
                    group_index: index,
                    start: payload_start,
                    len: group.payload_len as usize,
                    payload_count,
                });
            };
            for (payload_index, payload) in self.payloads[payload_range.clone()].iter().enumerate()
            {
                if payload.owner != group.anchor {
                    return Err(SlotInvariantError::PayloadOwnerMismatch {
                        payload_anchor: payload.anchor,
                        expected: group.anchor,
                        actual: payload.owner,
                    });
                }
                let expected_location = (group.anchor, payload_index);
                let actual = self.payload_locations.get(payload.anchor);
                if actual != Some(expected_location) {
                    return Err(SlotInvariantError::PayloadLocationMismatch {
                        payload_anchor: payload.anchor,
                        expected: expected_location,
                        actual,
                    });
                }
            }
            expected_payload_start = payload_range.end;

            if let Some(scope_id) = group.scope_id {
                let actual = self.scope_anchor_to_group.get(&scope_id).copied();
                if actual != Some(group.anchor) {
                    return Err(SlotInvariantError::ScopeIndexMismatch {
                        scope_id,
                        expected: group.anchor,
                        actual,
                    });
                }
            }

            let node_start = group.node_start as usize;
            if node_start != expected_node_start {
                return Err(SlotInvariantError::NodeStartMismatch {
                    group_index: index,
                    expected: expected_node_start,
                    actual: node_start,
                });
            }
            let Some(node_range) = self.group_node_range_checked(index) else {
                return Err(SlotInvariantError::NodeOutOfRange {
                    group_index: index,
                    start: node_start,
                    len: group.node_len as usize,
                    node_count,
                });
            };
            for node in &self.nodes[node_range.clone()] {
                if node.owner != group.anchor {
                    return Err(SlotInvariantError::NodeOwnerMismatch {
                        node_id: node.id,
                        expected: group.anchor,
                        actual: node.owner,
                    });
                }
                if node.lifecycle != NodeLifecycle::Active {
                    return Err(SlotInvariantError::NodeLifecycleMismatch {
                        node_id: node.id,
                        expected: NodeLifecycle::Active,
                        actual: node.lifecycle,
                    });
                }
            }
            expected_node_start = node_range.end;

            stack.push((group.anchor, subtree_end));
        }

        if self.anchors.active_len() != self.groups.len() {
            return Err(SlotInvariantError::GroupAnchorCountMismatch {
                expected: self.groups.len(),
                actual: self.anchors.active_len(),
            });
        }

        if expected_node_start != node_count {
            return Err(SlotInvariantError::NodeCountMismatch {
                expected: expected_node_start,
                actual: node_count,
            });
        }

        if self.payload_locations.len() != payload_count {
            return Err(SlotInvariantError::PayloadAnchorCountMismatch {
                expected: payload_count,
                actual: self.payload_locations.len(),
            });
        }

        if self.scope_anchor_to_group.len() != scope_count {
            return Err(SlotInvariantError::ScopeIndexCountMismatch {
                expected: scope_count,
                actual: self.scope_anchor_to_group.len(),
            });
        }

        for (payload_anchor, (owner, payload_index)) in self.payload_locations.iter() {
            let Some(group_index) = self.anchors.active_index(owner) else {
                return Err(SlotInvariantError::PayloadLocationMismatch {
                    payload_anchor,
                    expected: (owner, payload_index),
                    actual: None,
                });
            };
            let actual = self
                .group_payload_records_at(group_index)
                .get(payload_index)
                .map(|payload| (payload.owner, payload.anchor));
            if actual != Some((owner, payload_anchor)) {
                return Err(SlotInvariantError::PayloadLocationMismatch {
                    payload_anchor,
                    expected: (owner, payload_index),
                    actual: Some((owner, payload_index)),
                });
            }
        }

        let mut expected_subtree_len = vec![1u32; self.groups.len()];
        let mut expected_subtree_node_count = self
            .groups
            .iter()
            .enumerate()
            .map(|(group_index, _)| self.group_node_len_at(group_index) as u32)
            .collect::<Vec<_>>();
        for index in (0..self.groups.len()).rev() {
            let parent_anchor = self.groups[index].parent_anchor;
            if !parent_anchor.is_valid() {
                continue;
            }
            let parent_index = self
                .anchors
                .active_index(parent_anchor)
                .expect("validated parents must resolve");
            expected_subtree_len[parent_index] += expected_subtree_len[index];
            expected_subtree_node_count[parent_index] += expected_subtree_node_count[index];
        }

        for (index, group) in self.groups.iter().enumerate() {
            if group.subtree_len != expected_subtree_len[index] {
                return Err(SlotInvariantError::BadSubtreeLen {
                    group_index: index,
                    expected: expected_subtree_len[index],
                    actual: group.subtree_len,
                });
            }
            if group.subtree_node_count != expected_subtree_node_count[index] {
                return Err(SlotInvariantError::BadSubtreeNodeCount {
                    group_index: index,
                    expected: expected_subtree_node_count[index],
                    actual: group.subtree_node_count,
                });
            }
        }

        Ok(())
    }
}
