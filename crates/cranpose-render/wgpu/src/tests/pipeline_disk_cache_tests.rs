use std::sync::{Arc, atomic::AtomicBool};

use web_time::{Duration, Instant};

use super::{PersistSignal, Wake};

#[test]
fn a_burst_of_pipelines_is_written_once_it_pauses() {
    let signal = PersistSignal::new();
    let stopped = AtomicBool::new(false);
    for _ in 0..3 {
        signal.note_change();
    }
    assert_eq!(
        signal.wait_for_quiet(0, Duration::from_millis(20), &stopped),
        Wake::Quiet(3)
    );
}

#[test]
fn a_build_inside_the_quiet_period_restarts_it() {
    let signal = Arc::new(PersistSignal::new());
    let stopped = AtomicBool::new(false);
    signal.note_change();
    let late = Arc::clone(&signal);
    let builder = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        late.note_change();
    });
    let started = Instant::now();
    let wake = signal.wait_for_quiet(0, Duration::from_millis(400), &stopped);
    assert!(builder.join().is_ok());
    assert_eq!(wake, Wake::Quiet(2), "the late build joins the burst");
    assert!(started.elapsed() >= Duration::from_millis(450));
}

#[test]
fn stopping_wakes_a_watcher_with_nothing_to_write() {
    let signal = Arc::new(PersistSignal::new());
    let stopped = Arc::new(AtomicBool::new(false));
    let stopper = {
        let signal = Arc::clone(&signal);
        let stopped = Arc::clone(&stopped);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            signal.stop(&stopped);
        })
    };
    let started = Instant::now();
    let wake = signal.wait_for_quiet(0, Duration::from_secs(60), &stopped);
    assert!(stopper.join().is_ok());
    assert_eq!(wake, Wake::Stopped(0));
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[test]
fn stopping_mid_burst_reports_the_unwritten_builds() {
    let signal = PersistSignal::new();
    let stopped = AtomicBool::new(false);
    signal.note_change();
    signal.note_change();
    signal.stop(&stopped);
    assert_eq!(
        signal.wait_for_quiet(0, Duration::from_secs(60), &stopped),
        Wake::Stopped(2)
    );
}

#[test]
fn builds_already_written_do_not_wake_the_watcher() {
    let signal = PersistSignal::new();
    let stopped = AtomicBool::new(false);
    signal.note_change();
    signal.stop(&stopped);
    assert_eq!(
        signal.wait_for_quiet(1, Duration::from_secs(60), &stopped),
        Wake::Stopped(1)
    );
}
