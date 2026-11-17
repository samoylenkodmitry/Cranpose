//! The page's rotation from `deviceorientation` events. A browser reports at
//! its own rate, so a requested period does not apply.
//!
//! Safari on iOS sends the events only after `DeviceOrientationEvent
//! .requestPermission()` resolves to "granted", which the browser allows
//! from a tap handler only. The application calls
//! `request_device_motion_permission()` there.

use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use cranpose_services::{
    DeviceMotionError, DeviceMotionPermission, DeviceMotionSensor, MotionClock, MotionConfig,
    MotionReference, Quaternion, publish_device_motion, publish_device_motion_permission,
    set_platform_device_motion,
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};

const NOT_ASKED: u8 = 0;
const GRANTED: u8 = 1;
const DENIED: u8 = 2;

static PERMISSION: AtomicU8 = AtomicU8::new(GRANTED);

// The event name and handler of the page's listener.
pub(crate) type Listener = (
    &'static str,
    Closure<dyn FnMut(web_sys::DeviceOrientationEvent)>,
);

pub(crate) fn register() {
    if constructor().is_none() {
        return;
    }
    if request_function().is_some() {
        PERMISSION.store(NOT_ASKED, Ordering::Release);
    }
    set_platform_device_motion(Arc::new(WebDeviceMotion));
}

fn constructor() -> Option<JsValue> {
    let window = web_sys::window()?;
    js_sys::Reflect::get(&window, &JsValue::from_str("DeviceOrientationEvent"))
        .ok()
        .filter(JsValue::is_function)
}

// `DeviceOrientationEvent.requestPermission`, which only Safari has.
fn request_function() -> Option<js_sys::Function> {
    let constructor = constructor()?;
    js_sys::Reflect::get(&constructor, &JsValue::from_str("requestPermission"))
        .ok()?
        .dyn_into()
        .ok()
}

// Chrome has an event of its own for the angles that count from north.
fn has_absolute_event() -> bool {
    web_sys::window().is_some_and(|window| {
        js_sys::Reflect::has(&window, &JsValue::from_str("ondeviceorientationabsolute"))
            .unwrap_or(false)
    })
}

struct WebDeviceMotion;

impl DeviceMotionSensor for WebDeviceMotion {
    fn clock(&self) -> MotionClock {
        MotionClock::PerformanceNow
    }

    fn now_ns(&self) -> u64 {
        web_sys::window()
            .and_then(|window| window.performance())
            .map_or(0, |performance| (performance.now() * 1.0e6) as u64)
    }

    fn available(&self) -> bool {
        constructor().is_some()
    }

    fn permission(&self) -> DeviceMotionPermission {
        match PERMISSION.load(Ordering::Acquire) {
            GRANTED => DeviceMotionPermission::Granted,
            DENIED => DeviceMotionPermission::Denied,
            _ => DeviceMotionPermission::NotAsked,
        }
    }

    fn request_permission(&self) {
        let Some(request) = request_function() else {
            return;
        };
        let Some(constructor) = constructor() else {
            return;
        };
        let promise = match request.call0(&constructor) {
            Ok(promise) => promise,
            Err(error) => {
                log::warn!("cranpose: the motion permission could not be asked: {error:?}");
                return;
            }
        };
        let Ok(promise) = promise.dyn_into::<js_sys::Promise>() else {
            return;
        };
        spawn_local(async move {
            let answer = JsFuture::from(promise).await;
            let granted = answer
                .ok()
                .and_then(|answer| answer.as_string())
                .is_some_and(|answer| answer == "granted");
            PERMISSION.store(if granted { GRANTED } else { DENIED }, Ordering::Release);
            publish_device_motion_permission(granted);
        });
    }

    fn start(&self, config: MotionConfig) -> Result<(), DeviceMotionError> {
        let window = web_sys::window().ok_or(DeviceMotionError::Unavailable)?;
        self.stop();
        let event = if config.reference == MotionReference::MagneticNorth && has_absolute_event() {
            "deviceorientationabsolute"
        } else {
            "deviceorientation"
        };
        let handler = Closure::wrap(Box::new(move |event: web_sys::DeviceOrientationEvent| {
            // A browser without the sensor sends one event whose angles are null.
            let (Some(alpha), Some(beta), Some(gamma)) =
                (event.alpha(), event.beta(), event.gamma())
            else {
                return;
            };
            let reference = if event.absolute() {
                MotionReference::MagneticNorth
            } else {
                MotionReference::ArbitraryHeading
            };
            publish_device_motion(
                Quaternion::from_device_orientation(alpha, beta, gamma),
                // `Event.timeStamp` counts milliseconds on `performance.now()`.
                (web_sys::Event::time_stamp(&event) * 1.0e6) as u64,
                reference,
            );
        })
            as Box<dyn FnMut(web_sys::DeviceOrientationEvent)>);
        window
            .add_event_listener_with_callback(event, handler.as_ref().unchecked_ref())
            .map_err(|error| DeviceMotionError::Failed(format!("{error:?}")))?;
        crate::thread_state::with(|state| {
            *state.web.device_motion.borrow_mut() = Some((event, handler));
        });
        Ok(())
    }

    fn stop(&self) {
        let listener =
            crate::thread_state::with(|state| state.web.device_motion.borrow_mut().take());
        if let (Some((event, handler)), Some(window)) = (listener, web_sys::window()) {
            let _ =
                window.remove_event_listener_with_callback(event, handler.as_ref().unchecked_ref());
        }
    }
}
