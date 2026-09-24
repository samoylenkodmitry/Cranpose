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
#[expect(non_snake_case)]
#[track_caller]
pub fn rememberMemoryPressure() -> EventStream<MemoryPressure> {
    rememberEventStream((), |sender| {
        observe_memory_pressure(move |pressure| sender.send(pressure))
    })
}

#[cfg(test)]
#[path = "tests/memory_pressure_tests.rs"]
mod tests;
