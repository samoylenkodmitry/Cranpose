//! What an app chooses about the pointer: its buttons, its beams and the direct touch of a hand.

use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use cranpose_core::CompositionLocal;

use crate::{PointerMapping, Vibration, locals::with_locals};

// The tick that a controller plays when its press takes and when a click ends: light and short,
// so it marks the moment and does not shake the ray.
const PRESS_TICK: Vibration = Vibration::constant(0.3, Duration::from_millis(20));

// What the pointer reads of the app's settings in a frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PointerChoice {
    pub(crate) mapping: PointerMapping,
    pub(crate) beams_visible: bool,
    pub(crate) press_vibration: Option<Vibration>,
    pub(crate) direct_touch: bool,
}

impl Default for PointerChoice {
    // The default mapping, with the beams shown, the press's tick and the direct touch on.
    fn default() -> Self {
        Self {
            mapping: PointerMapping::new(),
            beams_visible: true,
            press_vibration: Some(PRESS_TICK),
            direct_touch: true,
        }
    }
}

/// The pointer's settings: which controller input presses which pointer button, whether the
/// beams of the pointers show, the vibration that a press plays and whether a tracked hand presses
/// the panels by touch. An app reads the handle from [`local_pointer_settings`], and changes it from
/// the composition, from an event handler or from any other thread. A change counts from the next
/// frame.
///
/// A beam shows only while the app has a panel, a curved panel or a volume that the pointer can
/// reach, and not at all while the app hides the beams. An app that draws its own rays, or whose
/// content is hidden, hides the beams with [`set_beams_visible`](Self::set_beams_visible). The
/// pointer works with its beams hidden. Jetpack XR has no such API.
#[derive(Clone, Debug)]
pub struct PointerSettings {
    choice: Arc<Mutex<PointerChoice>>,
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
    // The default settings: the default mapping, with the beams shown and the direct touch on.
    pub(crate) fn new() -> Self {
        Self {
            choice: Arc::new(Mutex::new(PointerChoice::default())),
        }
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // What the pointer reads of the settings in this frame.
    pub(crate) fn choice(&self) -> PointerChoice {
        self.read()
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

    /// Whether a tracked hand presses the panels by direct touch. It is `true` until the app turns
    /// it off.
    ///
    /// With the direct touch on, the tip of a tracked hand's index finger hovers over the panel
    /// whose face it comes near, as a ray does that rests there, and presses the face where it
    /// touches it. The press needs no pinch. It holds its point while the finger shakes, drags once
    /// the finger slides along the face, and ends when the finger comes back out of the face. Only a
    /// finger that comes to the face from the front presses: a finger that pushes through a panel
    /// presses nothing behind it. While the finger is near a panel, that hand's beam hides and its
    /// pinch presses nothing. A curved panel takes the touch on its curve. A volume takes no touch:
    /// its meshes take the ray. A controller presses only with its ray.
    ///
    /// Jetpack XR has no such setting: Android XR gives every panel its direct touch.
    pub fn direct_touch(&self) -> bool {
        self.read().direct_touch
    }

    /// Turns the direct touch of [`direct_touch`](Self::direct_touch) on or off. A finger that
    /// presses a panel when the touch turns off releases its press.
    pub fn set_direct_touch(&self, on: bool) {
        self.write(|choice| choice.direct_touch = on);
    }

    fn read(&self) -> PointerChoice {
        // The data is plain, so a panic of another thread leaves it whole
        *self.choice.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self, change: impl FnOnce(&mut PointerChoice)) {
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
