//! Haptic feedback — the framework analogue of Jetpack's `LocalHapticFeedback`.
//!
//! The compiled-in default is a no-op; platform backends install a real
//! implementation through [`set_platform_haptics`] (iOS `UIFeedbackGenerator`,
//! Android `Vibrator`). Desktop and the web have no haptics and drop it.

use cranpose_core::compositionLocalOfWithPolicy;
use cranpose_core::CompositionLocal;
use cranpose_core::CompositionLocalProvider;
use cranpose_macros::composable;
use std::cell::RefCell;
use std::rc::Rc;

/// A haptic feedback event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HapticFeedback {
    /// A light physical impact (e.g. a small control toggling).
    ImpactLight,
    /// A medium physical impact (e.g. a button press).
    ImpactMedium,
    /// A heavy physical impact (e.g. a large snap).
    ImpactHeavy,
    /// A selection change (e.g. scrubbing through a picker).
    Selection,
    /// A task completed successfully.
    Success,
    /// A warning.
    Warning,
    /// An error / rejected action.
    Error,
}

/// Performs haptic feedback. Installed by the platform backend; the default is
/// a no-op.
pub trait Haptics {
    fn perform(&self, feedback: HapticFeedback);
}

pub type HapticsRef = Rc<dyn Haptics>;

struct NoopHaptics;

impl Haptics for NoopHaptics {
    fn perform(&self, _feedback: HapticFeedback) {}
}

thread_local! {
    static PLATFORM_HAPTICS: RefCell<Option<HapticsRef>> = const { RefCell::new(None) };
}

/// Installs a platform haptics implementation, replacing any previous one.
pub fn set_platform_haptics(haptics: HapticsRef) {
    PLATFORM_HAPTICS.with(|cell| *cell.borrow_mut() = Some(haptics));
}

/// Removes any registered platform haptics (tests and teardown).
pub fn clear_platform_haptics() {
    PLATFORM_HAPTICS.with(|cell| *cell.borrow_mut() = None);
}

pub fn default_haptics() -> HapticsRef {
    PLATFORM_HAPTICS
        .with(|cell| cell.borrow().clone())
        .unwrap_or_else(|| Rc::new(NoopHaptics))
}

pub fn local_haptics() -> CompositionLocal<HapticsRef> {
    thread_local! {
        static LOCAL_HAPTICS: RefCell<Option<CompositionLocal<HapticsRef>>> = const { RefCell::new(None) };
    }

    LOCAL_HAPTICS.with(|cell| {
        let mut local = cell.borrow_mut();
        local
            .get_or_insert_with(|| compositionLocalOfWithPolicy(default_haptics, Rc::ptr_eq))
            .clone()
    })
}

#[allow(non_snake_case)]
#[composable]
pub fn ProvideHaptics(content: impl FnOnce()) {
    let haptics = cranpose_core::remember(default_haptics).with(|state| state.clone());
    let local = local_haptics();
    CompositionLocalProvider(vec![local.provides(haptics)], move || {
        content();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn registered_haptics_receives_events() {
        clear_platform_haptics();
        default_haptics().perform(HapticFeedback::Selection); // no-op, no panic

        struct Rec(Rc<Cell<u32>>);
        impl Haptics for Rec {
            fn perform(&self, _f: HapticFeedback) {
                self.0.set(self.0.get() + 1);
            }
        }
        let count = Rc::new(Cell::new(0));
        set_platform_haptics(Rc::new(Rec(count.clone())));
        default_haptics().perform(HapticFeedback::ImpactMedium);
        assert_eq!(count.get(), 1);
        clear_platform_haptics();
    }
}
