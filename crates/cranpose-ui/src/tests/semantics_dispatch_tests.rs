use super::*;

#[test]
fn a_scheduled_node_is_handed_to_the_processor_once() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_semantics_invalidations();

    schedule_semantics_invalidation(7);
    schedule_semantics_invalidation(7);
    schedule_semantics_invalidation(9);
    assert!(has_pending_semantics_invalidations());

    let mut seen = Vec::new();
    process_semantics_invalidations(|node_id| seen.push(node_id));
    seen.sort_unstable();
    assert_eq!(seen, vec![7, 9]);
    assert!(!has_pending_semantics_invalidations());
}

#[test]
fn nothing_is_pending_until_something_asks() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_semantics_invalidations();
    assert!(!has_pending_semantics_invalidations());

    let mut seen = 0;
    process_semantics_invalidations(|_| seen += 1);
    assert_eq!(seen, 0);
}
