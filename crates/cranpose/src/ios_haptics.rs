//! iOS haptics via `UIFeedbackGenerator`.
//!
//! Registered as the platform haptics (see
//! [`cranpose_services::set_platform_haptics`]) by the iOS backend. Feedback
//! generators are main-thread objects; haptics are triggered from UI event
//! handlers, so they run on the main thread.

use cranpose_services::{set_platform_haptics, HapticFeedback, Haptics};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_ui_kit::{
    UIImpactFeedbackGenerator, UIImpactFeedbackStyle, UINotificationFeedbackGenerator,
    UINotificationFeedbackType, UISelectionFeedbackGenerator,
};
use std::rc::Rc;

/// Installs the iOS haptics as the platform haptics.
pub(crate) fn register() {
    set_platform_haptics(Rc::new(IosHaptics));
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
