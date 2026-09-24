use std::sync::PoisonError;

use super::*;

fn recording_observer() -> (Observer, Arc<Mutex<Vec<MemoryPressure>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let observer: Observer = Arc::new(move |pressure| {
        recorder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(pressure);
    });
    (observer, seen)
}

#[test]
fn trim_levels_map_to_the_three_kinds() {
    assert_eq!(
        MemoryPressure::from_android_trim_level(20),
        MemoryPressure::UiHidden
    );
    for level in [5, 10, 40] {
        assert_eq!(
            MemoryPressure::from_android_trim_level(level),
            MemoryPressure::Low
        );
    }
    for level in [15, 60, 80] {
        assert_eq!(
            MemoryPressure::from_android_trim_level(level),
            MemoryPressure::Critical
        );
    }
}

#[test]
fn publish_reaches_every_observer() {
    let mut registry = Registry::new();
    let (first, first_seen) = recording_observer();
    let (second, second_seen) = recording_observer();
    registry.observe(1, first);
    registry.observe(2, second);

    for observer in registry.publish() {
        observer(MemoryPressure::Critical);
    }

    for seen in [first_seen, second_seen] {
        assert_eq!(
            seen.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_slice(),
            [MemoryPressure::Critical]
        );
    }
}

#[test]
fn a_removed_observer_stops_seeing_reports() {
    let mut registry = Registry::new();
    let (observer, seen) = recording_observer();
    registry.observe(7, observer);

    for observer in registry.publish() {
        observer(MemoryPressure::Low);
    }
    registry.remove_observer(7);
    for observer in registry.publish() {
        observer(MemoryPressure::Low);
    }

    assert_eq!(
        seen.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_slice(),
        [MemoryPressure::Low]
    );
}

#[test]
fn a_report_with_no_observers_goes_nowhere() {
    let registry = Registry::new();
    assert!(
        registry.publish().is_empty(),
        "pressure describes a moment; nothing is kept for late observers"
    );
}
