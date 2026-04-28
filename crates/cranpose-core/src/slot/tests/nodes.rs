use super::*;

#[test]
fn record_node_reports_explicit_replacement() {
    const GROUP_KEY: Key = 366;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let recorded = session.record_node_with_parent(11, 1, None);
        assert_eq!(
            recorded,
            NodeSlotUpdate::Inserted {
                id: 11,
                generation: 1,
            },
        );
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        assert_eq!(session.current_node_record(), Some((11, 1)));
        let recorded = session.record_node_with_parent(12, 1, None);
        assert_eq!(
            recorded,
            NodeSlotUpdate::Replaced {
                old_id: 11,
                old_generation: 1,
                new_id: 12,
                new_generation: 1,
            },
            "replacing the node at the current cursor must report an explicit replacement",
        );
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    assert_eq!(harness.table.group_node_record_at(0, 0).id, 12);
}
