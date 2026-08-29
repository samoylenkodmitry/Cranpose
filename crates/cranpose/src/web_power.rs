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

pub(crate) fn register() {
    set_platform_power_monitor(Arc::new(WebPowerMonitor));
}

pub(crate) fn start_battery_probe(request_frame: Rc<dyn Fn()>) {
    let Some(navigator) = web_sys::window().map(|window| window.navigator()) else {
        return;
    };
    let Some(get_battery) = get_battery_function(&navigator) else {
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

fn get_battery_function(navigator: &web_sys::Navigator) -> Option<js_sys::Function> {
    let value = js_sys::Reflect::get(navigator.as_ref(), &JsValue::from_str("getBattery")).ok()?;
    value.dyn_into::<js_sys::Function>().ok()
}

fn update_from_manager(manager: &web_sys::BatteryManager, request_frame: &Rc<dyn Fn()>) {
    let percent = (manager.level() * 100.0).round().clamp(0.0, 100.0) as u8;
    BATTERY_PERCENT.store(percent, Ordering::Release);
    BATTERY_CHARGING.store(manager.charging(), Ordering::Release);
    BATTERY_RESOLVED.store(true, Ordering::Release);
    publish_power_state(power_state());
    request_frame();
}

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
