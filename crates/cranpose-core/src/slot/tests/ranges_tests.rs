use super::*;

#[test]
fn group_range_tracks_empty_and_non_empty_spans() {
    let empty = GroupRange::new(2, 2);
    assert_eq!(empty.as_range(), 2..2);

    let non_empty = GroupRange::from_start_len(2, 3);
    assert_eq!(non_empty.as_range(), 2..5);
}

#[test]
fn subtree_range_keeps_root_and_span_together() {
    let range = SubtreeRange::from_root_len(4, 3);
    assert_eq!(range.root_index(), 4);
    assert_eq!(range.len(), 3);
    assert_eq!(range.as_range(), 4..7);
}

#[test]
fn group_payload_range_converts_group_offsets_to_table_range() {
    let group_payloads = PayloadRange::new(10, 15);
    let range = GroupPayloadRange::from_range(GroupItemRange::new(2, group_payloads, 1, 4), 1);
    assert_eq!(range.group_index(), 2);
    assert_eq!(range.start_offset(), 1);
    assert_eq!(range.into_inner().as_range(), 11..14);
}

#[test]
fn group_node_range_converts_group_offsets_to_table_range() {
    let group_nodes = NodeRange::new(20, 26);
    let range = GroupNodeRange::new(5, group_nodes, 2, 6);
    assert_eq!(range.group_index(), 5);
    assert_eq!(range.as_range(), 22..26);
}

#[test]
fn direct_child_range_accepts_only_indexes_inside_parent_span() {
    let range = DirectChildRange::new(3, 8);
    assert_eq!(range.start(), 3);
    assert!(!range.contains_index(2));
    assert!(range.contains_index(3));
    assert!(range.contains_index(7));
    assert!(!range.contains_index(8));
    assert_eq!(range.end(), 8);
}
