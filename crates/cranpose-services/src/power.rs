//! Device power and thermal state: observable, capability-aware, and explicit
//! about what this platform cannot answer.
//!
//! A snapshot query cannot tell "the battery is at 100%" from "this platform
//! has no battery API", and an application that guesses gets the decision
//! wrong on the platform it was not written for. Every reading here is either a
//! value or [`PowerReading::Unsupported`], and the whole state is observable so
//! a screen reacts to thermal pressure instead of sampling it.

use crate::registry::ServiceRegistry;
use cranpose_core::{rememberEventStream, State};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// The operating system's thermal-pressure level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThermalState {
    /// No thermal pressure.
    #[default]
    Normal,
    /// Light pressure; ordinary work may continue.
    Light,
    /// Sustained work should be reduced.
    Moderate,
    /// Expensive work should pause.
    Severe,
    /// The device is close to forced shutdown.
    Critical,
    /// Emergency pressure reported by the platform.
    Emergency,
    /// The platform is shutting down because of heat.
    Shutdown,
}

impl ThermalState {
    /// Whether sustained, expensive work should stop at this level.
    pub fn should_pause_work(self) -> bool {
        self >= ThermalState::Severe
    }
}

/// Current battery level and charging state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatteryStatus {
    /// Charge from zero through one hundred percent.
    pub percent: u8,
    /// Whether external power is charging or has fully charged the battery.
    pub charging: bool,
}

/// One power reading, or the reason there is no value.
///
/// The distinction matters: a desktop with no battery is not a device at 100%,
/// and an application that treats it as one refuses to run its own work on a
/// machine that is plugged into the wall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerReading<T> {
    /// The platform reported this value.
    Known(T),
    /// This platform has no API for it.
    Unsupported,
    /// The platform has the API but has not reported a value yet.
    Unknown,
}

impl<T> PowerReading<T> {
    /// The value, if the platform reported one.
    pub fn known(self) -> Option<T> {
        match self {
            PowerReading::Known(value) => Some(value),
            _ => None,
        }
    }

    /// Whether this platform can answer at all.
    pub fn is_supported(&self) -> bool {
        !matches!(self, PowerReading::Unsupported)
    }

    /// The value, or `fallback` when there is none.
    pub fn unwrap_or(self, fallback: T) -> T {
        self.known().unwrap_or(fallback)
    }
}

/// What a power backend can answer on this platform.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PowerCapabilities {
    /// Whether [`PowerMonitor::thermal_state`] reports real readings.
    pub thermal: bool,
    /// Whether [`PowerMonitor::battery_status`] reports real readings.
    pub battery: bool,
    /// Whether the OS has a background-execution restriction to report.
    pub background_restriction: bool,
}

/// Everything the platform currently says about power.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerState {
    /// Current thermal pressure.
    pub thermal: PowerReading<ThermalState>,
    /// Current battery state.
    pub battery: PowerReading<BatteryStatus>,
    /// Whether the OS permits unrestricted background execution.
    pub unrestricted_background_work: PowerReading<bool>,
}

impl PowerState {
    /// The state a platform with no power APIs reports.
    pub const fn unsupported() -> Self {
        Self {
            thermal: PowerReading::Unsupported,
            battery: PowerReading::Unsupported,
            unrestricted_background_work: PowerReading::Unsupported,
        }
    }

    /// Whether sustained work should stop. A platform that cannot measure heat
    /// never says stop, because guessing would starve every desktop build.
    pub fn should_pause_work(&self) -> bool {
        matches!(self.thermal, PowerReading::Known(level) if level.should_pause_work())
    }
}

/// Platform power policy.
pub trait PowerMonitor: Send + Sync {
    /// What this backend can answer.
    fn capabilities(&self) -> PowerCapabilities {
        PowerCapabilities::default()
    }

    /// Current thermal pressure.
    fn thermal_state(&self) -> PowerReading<ThermalState> {
        PowerReading::Unsupported
    }

    /// Current battery state.
    fn battery_status(&self) -> PowerReading<BatteryStatus> {
        PowerReading::Unsupported
    }

    /// Whether the OS permits unrestricted background execution for this app.
    fn unrestricted_background_work(&self) -> PowerReading<bool> {
        PowerReading::Unsupported
    }

    /// Opens the platform UI where the user can allow background execution.
    fn request_unrestricted_background_work(&self) {}
}

/// Shared power-monitor service.
pub type PowerMonitorRef = Arc<dyn PowerMonitor>;

struct DefaultPowerMonitor;
impl PowerMonitor for DefaultPowerMonitor {}

static PLATFORM_POWER_MONITOR: ServiceRegistry<dyn PowerMonitor> = ServiceRegistry::new();

/// Installs the platform power monitor.
pub fn set_platform_power_monitor(monitor: PowerMonitorRef) {
    PLATFORM_POWER_MONITOR.set(monitor);
    publish_power_state(power_state());
}

/// Removes the platform power monitor.
pub fn clear_platform_power_monitor() {
    PLATFORM_POWER_MONITOR.clear();
    if let Ok(mut observers) = power_observers().lock() {
        observers.clear();
    }
}

/// Returns the platform monitor, or one that answers "unsupported".
pub fn power_monitor() -> PowerMonitorRef {
    PLATFORM_POWER_MONITOR
        .get()
        .unwrap_or_else(|| Arc::new(DefaultPowerMonitor))
}

/// The current power state, read from the installed backend.
pub fn power_state() -> PowerState {
    let monitor = power_monitor();
    PowerState {
        thermal: monitor.thermal_state(),
        battery: monitor.battery_status(),
        unrestricted_background_work: monitor.unrestricted_background_work(),
    }
}

/// What the installed backend can answer on this platform.
pub fn power_capabilities() -> PowerCapabilities {
    power_monitor().capabilities()
}

type PowerObserver = Arc<dyn Fn(PowerState) + Send + Sync>;

fn power_observers() -> &'static Mutex<Vec<(u64, PowerObserver)>> {
    static SLOT: OnceLock<Mutex<Vec<(u64, PowerObserver)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(Vec::new()))
}

static NEXT_OBSERVER: AtomicU64 = AtomicU64::new(1);

/// Keeps a power observer registered until it is dropped.
pub struct PowerObserverRegistration {
    id: u64,
}

impl Drop for PowerObserverRegistration {
    fn drop(&mut self) {
        if let Ok(mut observers) = power_observers().lock() {
            observers.retain(|(id, _)| *id != self.id);
        }
    }
}

/// Registers `observer` for power-state changes. Applications collect
/// [`rememberPowerState`] instead of calling this.
pub fn observe_power_state(
    observer: impl Fn(PowerState) + Send + Sync + 'static,
) -> PowerObserverRegistration {
    let id = NEXT_OBSERVER.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut observers) = power_observers().lock() {
        observers.push((id, Arc::new(observer)));
    }
    PowerObserverRegistration { id }
}

/// Publishes a new power state. Platform backends call this whenever the OS
/// reports a thermal or battery change, from whatever thread delivered it.
pub fn publish_power_state(state: PowerState) {
    let observers = power_observers()
        .lock()
        .map(|observers| {
            observers
                .iter()
                .map(|(_, observer)| Arc::clone(observer))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for observer in observers {
        observer(state);
    }
}

/// The device's power state, observed for as long as this call stays in the
/// composition.
#[allow(non_snake_case)]
pub fn rememberPowerState() -> State<PowerState> {
    let updates = rememberEventStream((), |sender| {
        observe_power_state(move |state| sender.send(state))
    });
    cranpose_core::collectAsState(updates, (), power_state())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DesktopMonitor;

    impl PowerMonitor for DesktopMonitor {
        fn capabilities(&self) -> PowerCapabilities {
            PowerCapabilities {
                thermal: false,
                battery: false,
                background_restriction: false,
            }
        }
    }

    struct PhoneMonitor;

    impl PowerMonitor for PhoneMonitor {
        fn capabilities(&self) -> PowerCapabilities {
            PowerCapabilities {
                thermal: true,
                battery: true,
                background_restriction: true,
            }
        }
        fn thermal_state(&self) -> PowerReading<ThermalState> {
            PowerReading::Known(ThermalState::Severe)
        }
        fn battery_status(&self) -> PowerReading<BatteryStatus> {
            PowerReading::Known(BatteryStatus {
                percent: 12,
                charging: false,
            })
        }
        fn unrestricted_background_work(&self) -> PowerReading<bool> {
            PowerReading::Known(false)
        }
    }

    #[test]
    fn a_platform_without_power_apis_says_unsupported_rather_than_full() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_power_monitor();
        let state = power_state();
        assert_eq!(state, PowerState::unsupported());
        assert!(!state.thermal.is_supported());
        assert!(!state.should_pause_work());
        assert_eq!(power_capabilities(), PowerCapabilities::default());
    }

    #[test]
    fn a_backend_that_measures_nothing_still_reports_its_capabilities() {
        let _guard = crate::registry::test_service_guard();
        set_platform_power_monitor(Arc::new(DesktopMonitor));
        assert!(!power_capabilities().thermal);
        assert_eq!(power_state().battery, PowerReading::Unsupported);
        clear_platform_power_monitor();
    }

    #[test]
    fn severe_thermal_pressure_pauses_sustained_work() {
        let _guard = crate::registry::test_service_guard();
        set_platform_power_monitor(Arc::new(PhoneMonitor));
        let state = power_state();
        assert!(state.should_pause_work());
        assert_eq!(
            state.battery.known().map(|battery| battery.percent),
            Some(12)
        );
        assert!(!state.unrestricted_background_work.unwrap_or(true));
        clear_platform_power_monitor();
    }

    #[test]
    fn observers_see_published_changes_and_stop_when_dropped() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_power_monitor();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        let registration = observe_power_state(move |state| {
            recorder
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(state.thermal)
        });
        publish_power_state(PowerState {
            thermal: PowerReading::Known(ThermalState::Moderate),
            ..PowerState::unsupported()
        });
        assert_eq!(
            seen.lock().unwrap_or_else(|e| e.into_inner()).as_slice(),
            [PowerReading::Known(ThermalState::Moderate)]
        );
        drop(registration);
        publish_power_state(PowerState::unsupported());
        assert_eq!(seen.lock().unwrap_or_else(|e| e.into_inner()).len(), 1);
    }
}
