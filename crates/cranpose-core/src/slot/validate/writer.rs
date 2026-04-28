use super::super::{ChildCursor, SlotTable, SlotWriteSessionState};
use super::SlotInvariantError;
use crate::AnchorId;

fn writer_frame_out_of_bounds(
    frame_index: usize,
    group_anchor: AnchorId,
    field: &'static str,
    value: usize,
    min: usize,
    max: usize,
) -> SlotInvariantError {
    SlotInvariantError::WriterFrameOutOfBounds {
        frame_index,
        group_anchor,
        field,
        value,
        min,
        max,
    }
}

fn validate_cursor(
    frame_index: usize,
    group_anchor: AnchorId,
    field: &'static str,
    value: usize,
    min: usize,
    max: usize,
) -> Result<(), SlotInvariantError> {
    if value < min || value > max {
        return Err(writer_frame_out_of_bounds(
            frame_index,
            group_anchor,
            field,
            value,
            min,
            max,
        ));
    }
    Ok(())
}

fn validate_child_boundary(
    table: &SlotTable,
    frame_index: usize,
    group_anchor: AnchorId,
    expected_parent: AnchorId,
    next_child_index: usize,
) -> Result<(), SlotInvariantError> {
    let child_range = table.direct_child_range(expected_parent);
    if table
        .direct_child_anchor_at_cursor(ChildCursor::new(expected_parent, next_child_index))
        .is_some()
        || next_child_index == child_range.end()
    {
        return Ok(());
    }

    let actual_parent = table
        .group_parent_anchor_at_index(next_child_index)
        .unwrap_or(AnchorId::INVALID);
    Err(SlotInvariantError::WriterFrameNotAtChildBoundary {
        frame_index,
        group_anchor,
        next_child_index,
        expected_parent,
        actual_parent,
    })
}

impl SlotWriteSessionState {
    pub(crate) fn validate(&mut self, table: &mut SlotTable) -> Result<(), SlotInvariantError> {
        table.flush_payload_location_refreshes(self);
        table.validate()?;

        validate_cursor(
            0,
            AnchorId::INVALID,
            "root.next_child_index",
            self.root.next_child_index,
            0,
            table.group_count(),
        )?;
        if self.root.detach_remaining_children {
            validate_child_boundary(
                table,
                0,
                AnchorId::INVALID,
                AnchorId::INVALID,
                self.root.next_child_index,
            )?;
        }

        let mut parent_group_index = None;
        let mut parent_subtree_end = table.group_count();
        let mut parent_depth = None;

        for (depth, frame) in self.group_stack.iter().enumerate() {
            let frame_index = depth + 1;
            let Some(group_index) = table.active_group_index(frame.group_anchor) else {
                return Err(SlotInvariantError::AnchorMismatch {
                    anchor: frame.group_anchor,
                    expected: parent_group_index.unwrap_or(0),
                    actual: table.group_anchor_state(frame.group_anchor),
                });
            };
            let min_group_index = parent_group_index.map_or(0, |index| index + 1);
            let max_group_index = parent_subtree_end.saturating_sub(1);
            validate_cursor(
                frame_index,
                frame.group_anchor,
                "group_index",
                group_index,
                min_group_index,
                max_group_index,
            )?;

            let payload_len = table.group_payload_len_at(group_index);
            let node_len = table.group_node_len_at(group_index);
            let payload_limit = frame.old_payload_len.max(payload_len);
            let node_limit = frame.old_node_len.max(node_len);
            if let Some(parent_depth) = parent_depth {
                let expected_depth = parent_depth + 1;
                validate_cursor(
                    frame_index,
                    frame.group_anchor,
                    "group_depth",
                    table.group_depth_at_index(group_index),
                    expected_depth,
                    expected_depth,
                )?;
            }

            let subtree_end = table.group_subtree_end_at_index(group_index);
            validate_cursor(
                frame_index,
                frame.group_anchor,
                "next_child_index",
                frame.next_child_index,
                group_index + 1,
                subtree_end,
            )?;
            validate_child_boundary(
                table,
                frame_index,
                frame.group_anchor,
                frame.group_anchor,
                frame.next_child_index,
            )?;
            validate_cursor(
                frame_index,
                frame.group_anchor,
                "payload_cursor",
                frame.payload_cursor,
                0,
                payload_limit,
            )?;
            validate_cursor(
                frame_index,
                frame.group_anchor,
                "node_cursor",
                frame.node_cursor,
                0,
                node_limit,
            )?;

            parent_group_index = Some(group_index);
            parent_subtree_end = subtree_end;
            parent_depth = Some(table.group_depth_at_index(group_index));
        }

        Ok(())
    }

    pub(in crate::slot) fn debug_assert_valid_after(
        &mut self,
        table: &mut SlotTable,
        operation: &'static str,
    ) {
        if let Err(err) = self.validate(table) {
            panic!("slot writer invariant violation after {operation}: {err:?}");
        }
    }
}
