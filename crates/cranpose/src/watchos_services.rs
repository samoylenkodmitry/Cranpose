#![expect(unsafe_code)]

//! The platform services of the watchOS host: the application's folders,
//! taps, the microphone, and with the `wearable` feature the link to the
//! iPhone app.

use std::sync::{Arc, Once};

use cranpose_services::{
    HapticFeedback, HapticPattern, Haptics, HostController, PlatformDirectories,
    set_host_controller, set_platform_haptics,
};
use objc2_watch_kit::{WKHapticType, WKInterfaceDevice};

use crate::apple_mobile::{buzz_starts, on_main_after, sandbox_directories};

/// Installs the services once per process.
pub(crate) fn register() {
    static REGISTERED: Once = Once::new();
    REGISTERED.call_once(|| {
        crate::apple_mobile::init_logging();
        set_host_controller(Arc::new(WatchHost));
        set_platform_haptics(Arc::new(WatchHaptics));
        crate::apple_microphone::register();
        #[cfg(feature = "wearable")]
        crate::apple_wearable::register();
    });
}

/// A watch app cannot keep the screen on, end itself or step back to the
/// watch face; the system decides all three.
struct WatchHost;

impl HostController for WatchHost {
    fn set_keep_screen_on(&self, _enabled: bool) {}

    fn platform_directories(&self) -> Option<PlatformDirectories> {
        Some(sandbox_directories())
    }

    fn exit(&self) {}

    fn background(&self) {}
}

/// WatchKit's taps, which play on the main thread.
struct WatchHaptics;

impl Haptics for WatchHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        play(match feedback {
            HapticFeedback::ImpactLight | HapticFeedback::Selection => WKHapticType::Click,
            HapticFeedback::ImpactMedium => WKHapticType::DirectionUp,
            HapticFeedback::ImpactHeavy => WKHapticType::Start,
            HapticFeedback::Success => WKHapticType::Success,
            HapticFeedback::Warning => WKHapticType::Retry,
            HapticFeedback::Error => WKHapticType::Failure,
        });
    }

    /// One tap where each buzz of the pattern starts, as strong as it.
    fn play_pattern(&self, pattern: &HapticPattern) {
        for (start, amplitude) in buzz_starts(pattern) {
            let tap = if amplitude >= 200 {
                WKHapticType::Start
            } else if amplitude >= 110 {
                WKHapticType::DirectionUp
            } else {
                WKHapticType::Click
            };
            on_main_after(start, move |_| play_now(tap));
        }
    }
}

fn play(tap: WKHapticType) {
    on_main_after(std::time::Duration::ZERO, move |_| play_now(tap));
}

fn play_now(tap: WKHapticType) {
    // SAFETY: the current device is valid for the life of the process, and
    // the caller is on the main thread.
    unsafe { WKInterfaceDevice::currentDevice().playHaptic(tap) };
}
