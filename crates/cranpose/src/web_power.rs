//! Battery status via the browser's Battery Status API
//! (`navigator.getBattery`).
//!
//! The API is optional in every sense [`PowerReading`] models: most browsers
//! never expose `navigator.getBattery` at all (Firefox and Safari never
//! shipped it; Chrome restricts what it reports for privacy but still exposes
//! the function), and where it exists, the manager it hands back arrives
//! asynchronously. So this backend tells apart three states rather than
//! collapsing them into one guess: no `getBattery` function on `navigator`
//! reports [`PowerReading::Unsupported`]; a function that exists but has not
//! resolved yet reports [`PowerReading::Unknown`]; and a resolved
//! [`web_sys::BatteryManager`] reports [`PowerReading::Known`], kept live by
//! its `levelchange`/`chargingchange` events for as long as the page runs.
//!
//! There is no thermal-pressure signal a web page can read, so thermal state
//! stays [`PowerReading::Unsupported`] on every browser — matching how the
//! desktop backend answers on Linux and Windows rather than inventing a
//! reading no platform API backs (see `desktop_power.rs`). Nor is there a
//! background-execution restriction a page can query or ask to lift, so
//! `unrestricted_background_work` and `request_unrestricted_background_work`
//! keep their [`PowerMonitor`] defaults.

use std::{
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use cranpose_services::{
    BatteryStatus, PowerCapabilities, PowerMonitor, PowerReading, power_state, publish_power_state,
    set_platform_power_monitor,
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};

/// Installs the browser power monitor. Battery readings stay
/// [`PowerReading::Unsupported`] until [`start_battery_probe`] finds the API,
/// and [`PowerReading::Unknown`] from there until its promise resolves.
pub(crate) fn register() {
    set_platform_power_monitor(Arc::new(WebPowerMonitor));
}

/// Starts the asynchronous `navigator.getBattery()` probe and, once it
/// resolves, keeps the reading live and wakes `request_frame` on every
/// change the browser reports.
///
/// Kept separate from [`register`] because it needs a frame requester that
/// does not exist yet at the point `web_services::register()` installs every
/// other browser-backed service; `web::run` calls it once the render loop is
/// set up.
pub(crate) fn start_battery_probe(request_frame: Rc<dyn Fn()>) {
    let Some(navigator) = web_sys::window().map(|window| window.navigator()) else {
        return;
    };
    let Some(get_battery) = get_battery_function(&navigator) else {
        // `BATTERY_SUPPORTED` stays false: `capabilities()` and
        // `battery_status()` already answer `Unsupported` without it.
        return;
    };
    let promise = match get_battery.call0(navigator.as_ref()) {
        Ok(value) => value,
        Err(error) => {
            log::debug!("navigator.getBattery() call failed: {error:?}");
            return;
        }
    };
    let Ok(promise) = promise.dyn_into::<js_sys::Promise>() else {
        return;
    };
    // The function exists and returned a promise: this browser can answer,
    // even before that promise resolves.
    BATTERY_SUPPORTED.store(true, Ordering::Release);
    publish_power_state(power_state());

    spawn_local(async move {
        let manager = match JsFuture::from(promise).await {
            Ok(value) => match value.dyn_into::<web_sys::BatteryManager>() {
                Ok(manager) => manager,
                Err(_) => return,
            },
            Err(error) => {
                log::debug!("navigator.getBattery() promise rejected: {error:?}");
                return;
            }
        };
        update_from_manager(&manager, &request_frame);
        // The manager outlives this task through the closures below; nothing
        // ever removes these listeners, matching every other DOM listener
        // this backend installs for the life of the page.
        for event_name in ["levelchange", "chargingchange"] {
            let manager_for_closure = manager.clone();
            let request_frame = request_frame.clone();
            let closure = Closure::wrap(Box::new(move || {
                update_from_manager(&manager_for_closure, &request_frame);
            }) as Box<dyn FnMut()>);
            let _ = manager
                .add_event_listener_with_callback(event_name, closure.as_ref().unchecked_ref());
            closure.forget();
        }
    });
}

/// `navigator.getBattery`, read reflectively: web-sys does not bind it (the
/// API was pulled from several standards tracks and is absent from the
/// `Navigator` IDL web-sys generates from), and probing the property instead
/// of assuming it exists is what lets every other browser answer
/// `Unsupported` truthfully instead of panicking on a missing function.
fn get_battery_function(navigator: &web_sys::Navigator) -> Option<js_sys::Function> {
    let value = js_sys::Reflect::get(navigator.as_ref(), &JsValue::from_str("getBattery")).ok()?;
    value.dyn_into::<js_sys::Function>().ok()
}

/// Reads the manager's current level and charging state into the shared
/// atomics, publishes the change, and wakes a frame so a composition
/// collecting `rememberPowerState` repaints with it.
fn update_from_manager(manager: &web_sys::BatteryManager, request_frame: &Rc<dyn Fn()>) {
    let percent = (manager.level() * 100.0).round().clamp(0.0, 100.0) as u8;
    BATTERY_PERCENT.store(percent, Ordering::Release);
    BATTERY_CHARGING.store(manager.charging(), Ordering::Release);
    BATTERY_RESOLVED.store(true, Ordering::Release);
    publish_power_state(power_state());
    request_frame();
}

// `PowerMonitorRef` is `Arc<dyn PowerMonitor>`, so the monitor itself must be
// `Send + Sync` even though the page is single-threaded; it carries no state
// of its own; the atomics below are what the DOM callbacks and `battery_status`
// share.
static BATTERY_SUPPORTED: AtomicBool = AtomicBool::new(false);
static BATTERY_RESOLVED: AtomicBool = AtomicBool::new(false);
static BATTERY_PERCENT: AtomicU8 = AtomicU8::new(0);
static BATTERY_CHARGING: AtomicBool = AtomicBool::new(false);

struct WebPowerMonitor;

impl PowerMonitor for WebPowerMonitor {
    fn capabilities(&self) -> PowerCapabilities {
        PowerCapabilities {
            thermal: false,
            battery: BATTERY_SUPPORTED.load(Ordering::Acquire),
            background_restriction: false,
        }
    }

    fn battery_status(&self) -> PowerReading<BatteryStatus> {
        if !BATTERY_SUPPORTED.load(Ordering::Acquire) {
            return PowerReading::Unsupported;
        }
        if !BATTERY_RESOLVED.load(Ordering::Acquire) {
            return PowerReading::Unknown;
        }
        PowerReading::Known(BatteryStatus {
            percent: BATTERY_PERCENT.load(Ordering::Acquire),
            charging: BATTERY_CHARGING.load(Ordering::Acquire),
        })
    }
}
