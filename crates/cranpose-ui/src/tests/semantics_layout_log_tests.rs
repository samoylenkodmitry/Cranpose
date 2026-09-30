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
    let mut stamp = 0;
    log.record(7, &mut stamp);
    let (sync, changed) = synced(&log);
    assert!(!sync.continuous, "the first reader walks the whole tree");
    assert!(changed.is_empty());
}

#[test]
fn each_node_is_recorded_once_between_reads() {
    let log = SemanticsLayoutLog::default();
    synced(&log);
    let (mut first, mut second) = (0, 0);
    log.record(1, &mut first);
    log.record(1, &mut first);
    log.record(2, &mut second);
    let (sync, changed) = synced(&log);
    assert!(sync.continuous);
    assert_eq!(changed, vec![1, 2]);

    log.record(1, &mut first);
    assert_eq!(
        synced(&log).1,
        vec![1],
        "a node changing again after a read is recorded again"
    );
}

#[test]
fn an_overflowing_log_stops_recording_and_starts_a_new_epoch() {
    let log = SemanticsLayoutLog::default();
    let (before, _) = synced(&log);
    let mut stamps = vec![0; MAX_PENDING + 1];
    for (id, stamp) in stamps.iter_mut().enumerate() {
        log.record(id, stamp);
    }
    let mut late = 0;
    log.record(99_999, &mut late);
    let (sync, changed) = synced(&log);
    assert!(!sync.continuous, "the reader walks the whole tree again");
    assert_ne!(sync.epoch, before.epoch);
    assert!(changed.is_empty());
    assert!(synced(&log).0.continuous, "the next read tracks again");
}

#[test]
fn changes_are_recorded_in_the_current_app_context_only_while_armed() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut stamp = 0;
    record_layout_change(3, &mut stamp);
    let drain = || {
        crate::render_state::with_current_semantics_layout_log(synced).map(|(_, changed)| changed)
    };
    assert_eq!(drain(), Some(Vec::new()), "an unarmed log records nothing");
    record_layout_change(3, &mut stamp);
    assert_eq!(drain(), Some(vec![3]));
}
