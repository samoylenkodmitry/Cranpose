use cranpose_core::NodeId;

use super::{LayoutSync, MAX_PENDING, SemanticsLayoutLog, record_layout_change};

fn synced(log: &SemanticsLayoutLog) -> (LayoutSync, Vec<NodeId>) {
    let mut changed = Vec::new();
    let sync = log.begin_sync(&mut changed);
    (sync, changed)
}

#[test]
fn nothing_is_recorded_before_a_tree_reads_the_log() {
    let log = SemanticsLayoutLog::default();
    log.record(NodeId(7));
    let (sync, changed) = synced(&log);
    assert!(!sync.continuous, "the first reader walks the whole tree");
    assert!(changed.is_empty());
}

#[test]
fn each_node_is_recorded_once_between_reads() {
    let log = SemanticsLayoutLog::default();
    synced(&log);
    log.record(NodeId(1));
    log.record(NodeId(1));
    log.record(NodeId(2));
    let (sync, changed) = synced(&log);
    assert!(sync.continuous);
    assert_eq!(changed, vec![NodeId(1), NodeId(2)]);

    log.record(NodeId(1));
    assert_eq!(
        synced(&log).1,
        vec![NodeId(1)],
        "a node changing again after a read is recorded again"
    );
}

#[test]
fn an_overflowing_log_stops_recording_and_starts_a_new_epoch() {
    let log = SemanticsLayoutLog::default();
    let (before, _) = synced(&log);
    for id in 0..=MAX_PENDING {
        log.record(NodeId(id));
    }
    log.record(NodeId(99_999));
    let (sync, changed) = synced(&log);
    assert!(!sync.continuous, "the reader walks the whole tree again");
    assert_ne!(sync.epoch, before.epoch);
    assert!(changed.is_empty());
    assert!(synced(&log).0.continuous, "the next read tracks again");
}

#[test]
fn changes_are_recorded_in_the_current_app_context_only_while_armed() {
    let _app_context = crate::render_state::app_context_test_scope();
    record_layout_change(NodeId(3));
    let drain = || {
        crate::render_state::with_current_semantics_layout_log(synced).map(|(_, changed)| changed)
    };
    assert_eq!(drain(), Some(Vec::new()), "an unarmed log records nothing");
    record_layout_change(NodeId(3));
    assert_eq!(drain(), Some(vec![NodeId(3)]));
}
