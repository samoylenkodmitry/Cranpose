use super::*;

#[test]
fn schedule_and_process_repasses() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_pointer_repasses();

    let node1: NodeId = 1;
    let node2: NodeId = 2;

    schedule_pointer_repass(node1);
    schedule_pointer_repass(node2);

    assert!(has_pending_pointer_repasses());

    let mut processed = Vec::new();
    process_pointer_repasses(|node_id| {
        processed.push(node_id);
    });

    assert_eq!(processed.len(), 2);
    assert!(processed.contains(&node1));
    assert!(processed.contains(&node2));
    assert!(!has_pending_pointer_repasses());
}

#[test]
fn duplicate_schedules_deduplicated() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_pointer_repasses();

    let node: NodeId = 42;
    schedule_pointer_repass(node);
    schedule_pointer_repass(node);
    schedule_pointer_repass(node);

    let mut count = 0;
    process_pointer_repasses(|_| {
        count += 1;
    });

    assert_eq!(count, 1);
}

#[test]
fn process_repasses_recovers_after_processor_panic() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_pointer_repasses();

    schedule_pointer_repass(1);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        process_pointer_repasses(|_| panic!("pointer repass processor panic"));
    }));
    assert!(result.is_err());

    schedule_pointer_repass(2);
    let mut processed = Vec::new();
    process_pointer_repasses(|node_id| processed.push(node_id));

    assert!(
        processed.contains(&2),
        "pointer repass processing must not stay stuck after a processor panic"
    );
    assert!(!has_pending_pointer_repasses());
}

#[test]
fn process_repasses_allows_processor_to_schedule_more_work() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_pointer_repasses();

    schedule_pointer_repass(1);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        process_pointer_repasses(|_| schedule_pointer_repass(2));
    }));
    assert!(
        result.is_ok(),
        "pointer repass processors must be able to enqueue follow-up repasses"
    );
    assert!(has_pending_pointer_repasses());

    let mut processed = Vec::new();
    process_pointer_repasses(|node_id| processed.push(node_id));

    assert_eq!(processed, vec![2]);
    assert!(!has_pending_pointer_repasses());
}

#[test]
fn pointer_repasses_are_scoped_by_app_context() {
    let _app_context = crate::render_state::app_context_test_scope();
    let first = crate::render_state::AppContext::new_with_density(1.0);
    let second = crate::render_state::AppContext::new_with_density(1.0);

    first.enter(|| {
        clear_pointer_repasses();
        schedule_pointer_repass(7);
        assert!(has_pending_pointer_repasses());
    });

    second.enter(|| {
        clear_pointer_repasses();
        assert!(!has_pending_pointer_repasses());
        schedule_pointer_repass(9);
    });

    first.enter(|| {
        let mut processed = Vec::new();
        process_pointer_repasses(|node_id| processed.push(node_id));
        assert_eq!(processed, vec![7]);
    });

    second.enter(|| {
        let mut processed = Vec::new();
        process_pointer_repasses(|node_id| processed.push(node_id));
        assert_eq!(processed, vec![9]);
    });
}
