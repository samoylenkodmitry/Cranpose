use std::{sync::Arc, time::Duration};

use cranpose_services::{
    HapticEffect, HapticFeedback, HapticPattern, Haptics, set_platform_haptics,
};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_ui_kit::{
    UIImpactFeedbackGenerator, UIImpactFeedbackStyle, UINotificationFeedbackGenerator,
    UINotificationFeedbackType, UISelectionFeedbackGenerator,
};

use crate::apple_mobile::{buzz_starts, on_main_after};

pub(crate) fn register() {
    set_platform_haptics(Arc::new(IosHaptics));
}

/// UIKit's feedback generators, which run on the main thread: a call from
/// another thread plays there, a moment later.
struct IosHaptics;

impl Haptics for IosHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        on_main_after(Duration::ZERO, move |mtm| match feedback {
            HapticFeedback::ImpactLight => impact(mtm, UIImpactFeedbackStyle::Light, 1.0),
            HapticFeedback::ImpactMedium => impact(mtm, UIImpactFeedbackStyle::Medium, 1.0),
            HapticFeedback::ImpactHeavy => impact(mtm, UIImpactFeedbackStyle::Heavy, 1.0),
            HapticFeedback::Selection => {
                let generator = UISelectionFeedbackGenerator::new(mtm);
                generator.selectionChanged();
            }
            HapticFeedback::Success => notify(mtm, UINotificationFeedbackType::Success),
            HapticFeedback::Warning => notify(mtm, UINotificationFeedbackType::Warning),
            HapticFeedback::Error => notify(mtm, UINotificationFeedbackType::Error),
        });
    }

    fn vibrate(&self, _duration_ms: u32, amplitude: u8) {
        let intensity = f64::from(amplitude.max(1)) / 255.0;
        on_main_after(Duration::ZERO, move |mtm| {
            impact(mtm, UIImpactFeedbackStyle::Medium, intensity);
        });
    }

    /// One impact where each buzz of the pattern starts, as strong as it.
    fn play_pattern(&self, pattern: &HapticPattern) {
        for (start, amplitude) in buzz_starts(pattern) {
            let style = style(amplitude);
            on_main_after(start, move |mtm| impact(mtm, style, 1.0));
        }
    }

    fn perform_effect(&self, effect: HapticEffect) {
        self.perform(effect.closest_feedback());
    }

    fn cancel(&self) {}

    fn has_amplitude_control(&self) -> bool {
        true
    }
}

fn style(amplitude: u8) -> UIImpactFeedbackStyle {
    if amplitude >= 200 {
        UIImpactFeedbackStyle::Heavy
    } else if amplitude >= 110 {
        UIImpactFeedbackStyle::Medium
    } else {
        UIImpactFeedbackStyle::Light
    }
}

fn impact(mtm: MainThreadMarker, style: UIImpactFeedbackStyle, intensity: f64) {
    #[expect(deprecated)]
    let generator =
        UIImpactFeedbackGenerator::initWithStyle(UIImpactFeedbackGenerator::alloc(mtm), style);
    generator.impactOccurredWithIntensity(intensity);
}

fn notify(mtm: MainThreadMarker, feedback: UINotificationFeedbackType) {
    let generator = UINotificationFeedbackGenerator::new(mtm);
    generator.notificationOccurred(feedback);
}
