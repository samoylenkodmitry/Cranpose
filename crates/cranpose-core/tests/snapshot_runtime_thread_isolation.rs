use cranpose_core::snapshot_v2::take_mutable_snapshot;
use std::sync::mpsc;

#[test]
fn mutable_snapshots_ignore_open_snapshots_from_other_threads() {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();

    let worker = std::thread::spawn(move || {
        let snapshot = take_mutable_snapshot(None, None);
        let snapshot_id = snapshot.snapshot_id();
        ready_tx.send(snapshot_id).expect("send worker snapshot id");
        release_rx.recv().expect("wait for release");
        snapshot.dispose();
    });

    let worker_snapshot_id = ready_rx.recv().expect("receive worker snapshot id");

    let snapshot = take_mutable_snapshot(None, None);
    assert!(
        !snapshot.invalid().get(worker_snapshot_id),
        "snapshot invalid set leaked another thread's open snapshot id {worker_snapshot_id}",
    );
    snapshot.dispose();

    release_tx.send(()).expect("release worker snapshot");
    worker.join().expect("join worker thread");
}
