//! What an app chooses about the pointer: its buttons and its beams.

use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use cranpose_core::CompositionLocal;

use crate::{PointerMapping, Vibration, locals::with_locals};

// The tick that a controller plays when its press takes and when a click ends: light and short,
// so it marks the moment and does not shake the ray.
const PRESS_TICK: Vibration = Vibration::constant(0.3, Duration::from_millis(20));

#[derive(Clone, Copy, Debug)]
struct Choice {
    mapping: PointerMapping,
    beams_visible: bool,
    press_vibration: Option<Vibration>,
}

/// The pointer's settings: which controller input presses which pointer button, whether the
/// beams of the pointers show, and the vibration that a press plays. An app reads the handle from
/// [`local_pointer_settings`], and changes it from the composition, from an event handler or from
/// any other thread. A change counts from the next frame.
///
/// A beam shows only while the app has a panel, a curved panel or a volume that the pointer can
/// reach, and not at all while the app hides the beams. An app that draws its own rays, or whose
/// content is hidden, hides the beams with [`set_beams_visible`](Self::set_beams_visible). The
/// pointer works with its beams hidden. Jetpack XR has no such API.
#[derive(Clone, Debug)]
pub struct PointerSettings {
    choice: Arc<Mutex<Choice>>,
}

impl PartialEq for PointerSettings {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.choice, &other.choice)
    }
}

impl PointerSettings {
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The default settings: the default mapping, with the beams shown.
    pub(crate) fn new() -> Self {
        Self {
            choice: Arc::new(Mutex::new(Choice {
                mapping: PointerMapping::new(),
                beams_visible: true,
                press_vibration: Some(PRESS_TICK),
            })),
        }
    }

    /// The mapping that the pointer uses.
    pub fn mapping(&self) -> PointerMapping {
        self.read().mapping
    }

    /// Makes the pointer use `mapping`. A button that is down keeps the pointer button that it
    /// pressed until the button goes up.
    pub fn set_mapping(&self, mapping: PointerMapping) {
        self.write(|choice| choice.mapping = mapping);
    }

    /// Whether the app lets the beams show. It is `true` until the app hides them.
    pub fn beams_visible(&self) -> bool {
        self.read().beams_visible
    }

    /// Shows or hides the beams of both pointers. A hidden beam submits no layer.
    pub fn set_beams_visible(&self, visible: bool) {
        self.write(|choice| choice.beams_visible = visible);
    }

    /// The vibration that a controller plays when a trigger press on a panel or a handle takes,
    /// when a press ends as a click, and when a handle's panel starts and stops moving. `None`
    /// plays nothing. It is a light tick of 20 ms until the app changes it. A press that turns
    /// into a drag ends with no tick, so the hand feels whether the press clicked.
    pub fn press_vibration(&self) -> Option<Vibration> {
        self.read().press_vibration
    }

    /// Sets the vibration of [`press_vibration`](Self::press_vibration), or turns it off with
    /// `None`.
    pub fn set_press_vibration(&self, vibration: Option<Vibration>) {
        self.write(|choice| choice.press_vibration = vibration);
    }

    fn read(&self) -> Choice {
        // The data is plain, so a panic of another thread leaves it whole
        *self.choice.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self, change: impl FnOnce(&mut Choice)) {
        change(&mut self.choice.lock().unwrap_or_else(PoisonError::into_inner));
    }
}

/// The composition local that holds the [`PointerSettings`], or `None` outside a headset.
///
/// ```ignore
/// if let Some(settings) = local_pointer_settings().current() {
///     // A right click on the grip, and no scrolling
///     settings.set_mapping(
///         PointerMapping::new()
///             .with_button(ControllerInput::Squeeze, Some(PointerButton::Secondary))
///             .with_scroll_speed(0.0),
///     );
/// }
/// ```
pub fn local_pointer_settings() -> CompositionLocal<Option<PointerSettings>> {
    with_locals(|locals| locals.pointer_settings.clone())
}

#[cfg(test)]
#[path = "tests/pointer_settings_tests.rs"]
mod tests;
