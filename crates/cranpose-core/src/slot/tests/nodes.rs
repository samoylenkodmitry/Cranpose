use super::*;

#[test]
fn record_node_result_is_false_when_replacing_existing_node_slot() {
    const GROUP_KEY: Key = 366;

    let mut harness = SlotHarness::new();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        let recorded = session.record_node(11, 1);
        assert!(!recorded.reused);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    harness.begin_pass(SlotPassMode::Compose);
    harness.session(|session| {
        begin_unkeyed(session, GROUP_KEY, None);
        assert_eq!(session.current_node_record(), Some((11, 1)));
        let recorded = session.record_node(12, 1);
        assert!(
            !recorded.reused,
            "replacing the node at the current cursor must not report a reused node"
        );
        assert_eq!(recorded.id, 12);
        let result = session.finish_group_body();
        assert!(result.detached_children.is_empty());
        session.end_group();
    });
    harness.finish_pass();

    assert_eq!(harness.table.group_node_record_at(0, 0).id, 12);
}
