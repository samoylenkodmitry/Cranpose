//! Memory pressure the platform reports for this process.
//!
//! Android delivers it through `onTrimMemory`; other hosts publish their own
//! signal. There is no backlog: pressure describes a moment, so an observer
//! that registers later waits for the next report. Applications collect the
//! stream and give back what they can rebuild — caches, warm model sessions,
//! pools.

use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use cranpose_core::{EventStream, rememberEventStream};

/// How hard the platform asks for memory back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryPressure {
    /// The UI left the screen. Anything held only to draw the next frame fast
    /// can go.
    UiHidden,
    /// The process should give back what it can rebuild.
    Low,
    /// The system reclaims by force next. Free everything that can go.
    Critical,
}

impl MemoryPressure {
    /// Maps an Android `ComponentCallbacks2` trim level.
    pub fn from_android_trim_level(level: i32) -> Self {
        match level {
            20 => Self::UiHidden,
            level if level >= 60 || level == 15 => Self::Critical,
            _ => Self::Low,
        }
    }
}

type Observer = Arc<dyn Fn(MemoryPressure) + Send + Sync>;

struct Registry {
    observers: Vec<(u64, Observer)>,
}

impl Registry {
    fn new() -> Self {
        Self {
            observers: Vec::new(),
        }
    }

    fn observe(&mut self, id: u64, observer: Observer) {
        self.observers.push((id, observer));
    }

    /// The observers a report should reach. Handed back rather than run here
    /// so the caller can leave the lock before running application code.
    fn publish(&self) -> Vec<Observer> {
        self.observers
            .iter()
            .map(|(_, observer)| Arc::clone(observer))
            .collect()
    }

    fn remove_observer(&mut self, id: u64) {
        self.observers.retain(|(existing, _)| *existing != id);
    }
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::new()))
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Keeps an observer registered until it is dropped.
pub struct MemoryPressureObserver {
    id: u64,
}

impl Drop for MemoryPressureObserver {
    fn drop(&mut self) {
        if let Ok(mut registry) = registry().lock() {
            registry.remove_observer(self.id);
        }
    }
}

/// Registers `observer` for pressure reports.
///
/// Applications collect the stream from [`rememberMemoryPressure`] instead of
/// calling this.
pub fn observe_memory_pressure(
    observer: impl Fn(MemoryPressure) + Send + Sync + 'static,
) -> MemoryPressureObserver {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut registry) = registry().lock() {
        registry.observe(id, Arc::new(observer));
    }
    MemoryPressureObserver { id }
}

/// Publishes a pressure report. Callable from any thread; the framework moves
/// each report onto the UI thread before a composition sees it.
pub fn publish_memory_pressure(pressure: MemoryPressure) {
    let observers = {
        let Ok(registry) = registry().lock() else {
            return;
        };
        registry.publish()
    };
    for observer in observers {
        observer(pressure);
    }
}

/// Collects pressure reports for as long as this call stays in the
/// composition.
///
/// ```rust,no_run
/// use cranpose_macros::composable;
/// use cranpose_services::rememberMemoryPressure;
///
/// #[composable]
/// fn Caches() {
///     let pressure = rememberMemoryPressure();
///     cranpose_core::CollectEvents(pressure, (), |report| {
///         log::info!("memory pressure: {report:?}");
///     });
/// }
/// ```
#[allow(non_snake_case)]
pub fn rememberMemoryPressure() -> EventStream<MemoryPressure> {
    rememberEventStream((), |sender| {
        observe_memory_pressure(move |pressure| sender.send(pressure))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording_observer() -> (Observer, Arc<Mutex<Vec<MemoryPressure>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        let observer: Observer = Arc::new(move |pressure| {
            recorder
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(pressure)
        });
        (observer, seen)
    }

    /// These exercise `Registry` directly rather than the process-global one,
    /// for the same reason the incoming-share tests do: the global is shared
    /// by every test in this binary and the harness runs them on parallel
    /// threads.
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
                seen.lock().unwrap_or_else(|e| e.into_inner()).as_slice(),
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
            seen.lock().unwrap_or_else(|e| e.into_inner()).as_slice(),
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
}
