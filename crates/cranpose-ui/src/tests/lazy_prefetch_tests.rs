use super::*;
use crate::render_state::app_context_test_scope;

fn drained_requests() -> Vec<NodeId> {
    let mut drained = Vec::new();
    drain_lazy_prefetch_requests(|node| drained.push(node));
    drained
}

#[test]
fn a_request_is_drained_once_and_names_each_list_once() {
    let _scope = app_context_test_scope();
    assert!(!has_lazy_prefetch_requests());
    request_lazy_prefetch(3);
    request_lazy_prefetch(3);
    request_lazy_prefetch(5);
    assert!(has_lazy_prefetch_requests());
    assert_eq!(drained_requests(), vec![3, 5]);
    assert!(drained_requests().is_empty());
    assert!(!has_lazy_prefetch_requests());
}

#[test]
fn the_item_cost_follows_recent_items() {
    let _scope = app_context_test_scope();
    assert_eq!(lazy_prefetch_item_cost(), Duration::ZERO);
    record_lazy_item_cost(Duration::from_millis(4));
    assert_eq!(lazy_prefetch_item_cost(), Duration::from_millis(4));
    record_lazy_item_cost(Duration::ZERO);
    assert_eq!(
        lazy_prefetch_item_cost(),
        Duration::from_millis(3),
        "a new item weighs a quarter"
    );
}

#[test]
fn a_prefetch_pass_is_one_only_while_it_runs() {
    let _scope = app_context_test_scope();
    assert!(!in_lazy_prefetch_pass());
    assert!(with_lazy_prefetch_pass(in_lazy_prefetch_pass));
    assert!(!in_lazy_prefetch_pass());
}
