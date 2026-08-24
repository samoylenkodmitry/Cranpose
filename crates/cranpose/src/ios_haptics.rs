//! iOS haptics via `UIFeedbackGenerator`.
//!
//! Registered as the platform haptics (see
//! [`cranpose_services::set_platform_haptics`]) by the iOS backend. Feedback
//! generators are main-thread objects; haptics are triggered from UI event
//! handlers, so they run on the main thread.

use std::sync::Arc;

use cranpose_services::{
    set_platform_haptics, HapticEffect, HapticFeedback, HapticPattern, Haptics,
};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_ui_kit::{
    UIImpactFeedbackGenerator, UIImpactFeedbackStyle, UINotificationFeedbackGenerator,
    UINotificationFeedbackType, UISelectionFeedbackGenerator,
};

/// Installs the iOS haptics as the platform haptics.
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

    /// `UIFeedbackGenerator` has no duration control, but it does take an
    /// intensity, so the amplitude is honoured and the duration is not.
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

    /// UIKit exposes no arbitrary waveform, so each step of the pattern that
    /// asks for vibration becomes one impact at its own intensity. Long
    /// patterns are truncated rather than run as a timed sequence: scheduling
    /// them would need a run-loop timer the framework does not own here.
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

    /// `UIFeedbackGenerator` events are instantaneous, so there is nothing
    /// running to cancel.
    fn cancel(&self) {}

    fn has_amplitude_control(&self) -> bool {
        true
    }
}

fn impact(mtm: MainThreadMarker, style: UIImpactFeedbackStyle) {
    // `initWithStyle:` is soft-deprecated in favor of the view-relative
    // initializer; the style-only generator is exactly what a non-spatial
    // haptic wants.
    #[allow(deprecated)]
    let generator =
        UIImpactFeedbackGenerator::initWithStyle(UIImpactFeedbackGenerator::alloc(mtm), style);
    generator.impactOccurred();
}

fn notify(mtm: MainThreadMarker, feedback: UINotificationFeedbackType) {
    let generator = UINotificationFeedbackGenerator::new(mtm);
    generator.notificationOccurred(feedback);
}
