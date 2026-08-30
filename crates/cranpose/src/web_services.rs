use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use cranpose_services::{
    DeviceInfo, HapticEffect, HapticFeedback, HapticPattern, Haptics, NetworkMonitor,
    NetworkStatus, Notifier, NotifyRequest, ShareContent, ShareError, ShareSheet,
    set_platform_device_info, set_platform_haptics, set_platform_network_monitor,
    set_platform_notifier, set_platform_share_sheet,
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};

pub(crate) fn register() {
    set_platform_share_sheet(Rc::new(WebShareSheet));
    set_platform_notifier(Arc::new(WebNotifier));
    set_platform_haptics(Arc::new(WebHaptics));
    set_platform_device_info(Rc::new(WebDeviceInfo));
    crate::web_media::install();
    crate::web_power::register();
    register_network_monitor();
}

fn navigator() -> Option<web_sys::Navigator> {
    web_sys::window().map(|window| window.navigator())
}

struct WebShareSheet;

impl WebShareSheet {
    fn build_share_data(content: &ShareContent) -> Result<web_sys::ShareData, ShareError> {
        let data = web_sys::ShareData::new();
        if let Some(text) = &content.text {
            data.set_text(text);
        }
        data.set_title(&content.file_name);

        let bytes = js_sys::Uint8Array::from(content.bytes.as_slice());
        let parts = js_sys::Array::new();
        parts.push(&bytes.buffer());
        let options = web_sys::FilePropertyBag::new();
        options.set_type(&content.mime_type);
        let file = web_sys::File::new_with_u8_array_sequence_and_options(
            &parts,
            &content.file_name,
            &options,
        )
        .map_err(|error| ShareError::Failed(format!("failed to build File: {error:?}")))?;
        let files = js_sys::Array::new();
        files.push(&file);
        data.set_files(&files);
        Ok(data)
    }
}

impl ShareSheet for WebShareSheet {
    fn share(&self, content: ShareContent) -> Result<(), ShareError> {
        let Some(navigator) = navigator() else {
            return Err(ShareError::Unsupported);
        };
        if !self.is_supported() {
            return Err(ShareError::Unsupported);
        }
        let data = Self::build_share_data(&content)?;
        if !navigator.can_share_with_data(&data) {
            return Err(ShareError::Failed(
                "browser cannot share this file payload".to_string(),
            ));
        }
        let _ = navigator.share_with_data(&data);
        Ok(())
    }

    fn is_supported(&self) -> bool {
        navigator().is_some_and(|navigator| {
            js_sys::Reflect::get(navigator.as_ref(), &JsValue::from_str("share"))
                .map(|value| value.is_function())
                .unwrap_or(false)
        })
    }
}

struct WebNotifier;

type ActiveNotification = (web_sys::Notification, Closure<dyn FnMut()>);

thread_local! {
    static ACTIVE_NOTIFICATIONS: RefCell<HashMap<String, ActiveNotification>> = RefCell::new(HashMap::new());
}

impl WebNotifier {
    fn granted() -> bool {
        web_sys::Notification::permission() == web_sys::NotificationPermission::Granted
    }
}

impl Notifier for WebNotifier {
    fn request_permission(&self) {
        let _ = web_sys::Notification::request_permission();
    }

    fn notify(&self, request: NotifyRequest) {
        if !Self::granted() {
            log::debug!("web notification dropped (permission not granted)");
            return;
        }
        let options = web_sys::NotificationOptions::new();
        options.set_body(&request.body);
        options.set_tag(&request.id);
        options.set_require_interaction(request.ongoing);
        let Ok(notification) = web_sys::Notification::new_with_options(&request.title, &options)
        else {
            log::warn!("failed to post web notification");
            return;
        };

        let deeplink = request.deeplink.clone();
        let on_click = Closure::wrap(Box::new(move || {
            if let Some(link) = &deeplink {
                cranpose_services::push_notification_deeplink(link.clone());
            }
            if let Some(window) = web_sys::window() {
                let _ = window.focus();
            }
        }) as Box<dyn FnMut()>);
        notification.set_onclick(Some(on_click.as_ref().unchecked_ref()));

        let previous = ACTIVE_NOTIFICATIONS.with(|active| {
            active
                .borrow_mut()
                .insert(request.id.clone(), (notification, on_click))
        });
        if let Some((previous, _closure)) = previous {
            previous.close();
        }
    }

    fn cancel(&self, id: &str) {
        if let Some((notification, _closure)) =
            ACTIVE_NOTIFICATIONS.with(|active| active.borrow_mut().remove(id))
        {
            notification.close();
        }
    }
}

struct WebHaptics;

impl Haptics for WebHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        let duration_ms = match feedback {
            HapticFeedback::ImpactLight | HapticFeedback::Selection => 8,
            HapticFeedback::ImpactMedium => 15,
            HapticFeedback::ImpactHeavy => 25,
            HapticFeedback::Success => 12,
            HapticFeedback::Warning => 20,
            HapticFeedback::Error => 35,
        };
        if let Some(navigator) = navigator() {
            let _ = navigator.vibrate_with_duration(duration_ms);
        }
    }

    fn vibrate(&self, duration_ms: u32, _amplitude: u8) {
        if duration_ms == 0 {
            return;
        }
        if let Some(navigator) = navigator() {
            let _ = navigator.vibrate_with_duration(duration_ms);
        }
    }

    fn play_pattern(&self, pattern: &HapticPattern) {
        let Some(navigator) = navigator() else {
            return;
        };
        let timings = js_sys::Array::new();
        for step in pattern.timings_ms() {
            timings.push(&JsValue::from_f64(f64::from(*step)));
        }
        let _ = navigator.vibrate_with_pattern(timings.as_ref());
    }

    fn perform_effect(&self, effect: HapticEffect) {
        self.perform(effect.closest_feedback());
    }

    fn cancel(&self) {
        if let Some(navigator) = navigator() {
            let _ = navigator.vibrate_with_duration(0);
        }
    }

    fn has_amplitude_control(&self) -> bool {
        false
    }
}

struct WebDeviceInfo;

impl DeviceInfo for WebDeviceInfo {
    fn total_memory_bytes(&self) -> Option<u64> {
        let navigator = navigator()?;
        let value = js_sys::Reflect::get(navigator.as_ref(), &JsValue::from_str("deviceMemory"))
            .ok()?
            .as_f64()?;
        if value <= 0.0 {
            return None;
        }
        Some((value * 1024.0 * 1024.0 * 1024.0) as u64)
    }
}

struct WebNetworkMonitor;

impl NetworkMonitor for WebNetworkMonitor {
    fn status(&self) -> NetworkStatus {
        NetworkStatus {
            online: NETWORK_ONLINE.load(Ordering::Acquire),
            metered: false,
        }
    }

    fn is_alive(&self) -> bool {
        web_sys::window().is_some()
    }

    fn reconnect(&self) {}
}

fn register_network_monitor() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let online = Arc::new(AtomicBool::new(window.navigator().on_line()));

    for (event, value) in [("online", true), ("offline", false)] {
        let online = Arc::clone(&online);
        let closure = Closure::wrap(Box::new(move || {
            online.store(value, Ordering::Release);
        }) as Box<dyn FnMut()>);
        let _ = window.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref());
        closure.forget();
    }

    NETWORK_ONLINE.store(online.load(Ordering::Acquire), Ordering::Release);
    set_platform_network_monitor(Arc::new(WebNetworkMonitor));
}

static NETWORK_ONLINE: AtomicBool = AtomicBool::new(true);
