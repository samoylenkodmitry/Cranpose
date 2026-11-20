//! Which pointer button each button of a controller presses, and how fast the thumbstick scrolls.

use cranpose::PointerButton;

use crate::ControllerInput;

/// How a controller's inputs press the pointer's buttons, and how fast its thumbstick scrolls the
/// panel that the pointer points at. An app changes it through
/// [`PointerSettings::set_mapping`](crate::PointerSettings::set_mapping).
///
/// The pointer's buttons are Compose's [`PointerButton`]s: the panels see a press of the
/// secondary button as a `PointerEventKind::ButtonDown(PointerButton::Secondary)`. The default
/// follows the mouse that a desktop has:
///
/// | Input | Pointer button |
/// | --- | --- |
/// | trigger | primary (left click) |
/// | first face button (A on the right hand, X on the left) | secondary (right click) |
/// | thumbstick click | middle |
/// | thumbstick | scrolls, 800 dp each second at full lean |
///
/// The grip, the second face button and the menu button press nothing by default. The grip is
/// where an app grabs, and the menu button is the app's own. A tracked hand has one button, the
/// pinch of its index finger, which presses the button of the trigger. Jetpack XR has no such
/// type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerMapping {
    buttons: [Option<PointerButton>; ControllerInput::ALL.len()],
    scroll_speed: f32,
}

impl Default for PointerMapping {
    fn default() -> Self {
        Self::new()
    }
}

impl PointerMapping {
    /// The default mapping, as the type's table lists it.
    pub const fn new() -> Self {
        Self {
            buttons: [
                Some(PointerButton::Primary),
                None,
                Some(PointerButton::Secondary),
                None,
                Some(PointerButton::Middle),
                None,
            ],
            scroll_speed: 800.0,
        }
    }

    /// The mapping where `input` presses `button`, or presses nothing for `None`. Two inputs may
    /// press the same button. The trigger also grabs the handles of panels, whatever it
    /// presses.
    #[must_use]
    pub const fn with_button(
        mut self,
        input: ControllerInput,
        button: Option<PointerButton>,
    ) -> Self {
        self.buttons[input.index()] = button;
        self
    }

    /// The button that `input` presses, or `None` when it presses nothing.
    pub const fn button(&self, input: ControllerInput) -> Option<PointerButton> {
        self.buttons[input.index()]
    }

    /// The mapping where a thumbstick leaned all the way scrolls `dp_per_second` dp each second.
    /// Leaning forward scrolls up and leaning to the right scrolls to the right, as a wheel
    /// does. A negative speed turns both directions around, and 0 turns scrolling off. A value
    /// that is not finite counts as 0.
    #[must_use]
    pub const fn with_scroll_speed(mut self, dp_per_second: f32) -> Self {
        self.scroll_speed = if dp_per_second.is_finite() {
            dp_per_second
        } else {
            0.0
        };
        self
    }

    /// How many dp each second a thumbstick leaned all the way scrolls.
    pub const fn scroll_speed(&self) -> f32 {
        self.scroll_speed
    }
}
