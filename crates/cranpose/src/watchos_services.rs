#![expect(unsafe_code)]

//! The platform services of the watchOS host: the application's folders,
//! taps, the microphone, with the `heart-rate` feature the wearer's pulse,
//! and with the `wearable` feature the link to the iPhone app.

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
        #[cfg(feature = "heart-rate")]
        crate::watchos_heart_rate::register();
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

    /// One click, whatever the length and strength.
    ///
    /// A vibration is silent on every other platform. Unless the watch is
    /// silenced, WatchKit plays a tone with every tap it offers apps but the
    /// click: watchOS ships a sound for each of the others.
    fn vibrate(&self, _duration_ms: u32, _amplitude: u8) {
        play(WKHapticType::Click);
    }

    /// One click where each buzz of the pattern starts, silent for the same
    /// reason as [`vibrate`](Haptics::vibrate).
    fn play_pattern(&self, pattern: &HapticPattern) {
        for (start, _) in buzz_starts(pattern) {
            on_main_after(start, |_| play_now(WKHapticType::Click));
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
