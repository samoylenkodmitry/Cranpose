use std::sync::Arc;

use cranpose_services::{
    HapticEffect, HapticFeedback, HapticPattern, Haptics, set_platform_haptics,
};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_ui_kit::{
    UIImpactFeedbackGenerator, UIImpactFeedbackStyle, UINotificationFeedbackGenerator,
    UINotificationFeedbackType, UISelectionFeedbackGenerator,
};

pub(crate) fn register() {
    set_platform_haptics(Arc::new(IosHaptics));
}

struct IosHaptics;

impl Haptics for IosHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        match feedback {
            HapticFeedback::ImpactLight => impact(mtm, UIImpactFeedbackStyle::Light),
            HapticFeedback::ImpactMedium => impact(mtm, UIImpactFeedbackStyle::Medium),
            HapticFeedback::ImpactHeavy => impact(mtm, UIImpactFeedbackStyle::Heavy),
            HapticFeedback::Selection => {
                let generator = UISelectionFeedbackGenerator::new(mtm);
                generator.selectionChanged();
            }
            HapticFeedback::Success => notify(mtm, UINotificationFeedbackType::Success),
            HapticFeedback::Warning => notify(mtm, UINotificationFeedbackType::Warning),
            HapticFeedback::Error => notify(mtm, UINotificationFeedbackType::Error),
        }
    }

    fn vibrate(&self, _duration_ms: u32, amplitude: u8) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        #[allow(deprecated)]
        let generator = UIImpactFeedbackGenerator::initWithStyle(
            UIImpactFeedbackGenerator::alloc(mtm),
            UIImpactFeedbackStyle::Medium,
        );
        let intensity = f64::from(amplitude.max(1)) / 255.0;
        generator.impactOccurredWithIntensity(intensity);
    }

    fn play_pattern(&self, pattern: &HapticPattern) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(amplitude) = pattern
            .amplitudes()
            .iter()
            .copied()
            .find(|level| *level > 0)
            .or(Some(pattern.peak_amplitude()))
            .filter(|level| *level > 0)
        else {
            return;
        };
        let style = if amplitude >= 200 {
            UIImpactFeedbackStyle::Heavy
        } else if amplitude >= 110 {
            UIImpactFeedbackStyle::Medium
        } else {
            UIImpactFeedbackStyle::Light
        };
        impact(mtm, style);
    }

    fn perform_effect(&self, effect: HapticEffect) {
        self.perform(effect.closest_feedback());
    }

    fn cancel(&self) {}

    fn has_amplitude_control(&self) -> bool {
        true
    }
}

fn impact(mtm: MainThreadMarker, style: UIImpactFeedbackStyle) {
    #[allow(deprecated)]
    let generator =
        UIImpactFeedbackGenerator::initWithStyle(UIImpactFeedbackGenerator::alloc(mtm), style);
    generator.impactOccurred();
}

fn notify(mtm: MainThreadMarker, feedback: UINotificationFeedbackType) {
    let generator = UINotificationFeedbackGenerator::new(mtm);
    generator.notificationOccurred(feedback);
}
