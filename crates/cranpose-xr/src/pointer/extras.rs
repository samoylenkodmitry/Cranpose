//! The buttons of a controller besides its trigger, as pointer buttons, and the scroll of its
//! thumbstick.

use cranpose::PointerButton;

use super::{Frame, Pointer};
use crate::{
    ControllerInput,
    aim::Buttons,
    gestures::Gestures,
    hit::Target,
    press_threshold::{Was, pressed},
    reach::Reach,
    surface_input::Gesture,
};

// A frame longer than this scrolls as if it lasted this long, in seconds, so a hitch does not
// throw a list far.
const LONGEST_STEP: f32 = 0.1;

// The inputs that press a button of the pointer besides the trigger, in the order of `Extras`.
const OTHERS: [ControllerInput; 5] = [
    ControllerInput::Squeeze,
    ControllerInput::Primary,
    ControllerInput::Secondary,
    ControllerInput::ThumbstickClick,
    ControllerInput::Menu,
];

// A button of the pointer that an input holds down: which one, as the mapping gave it when the
// input went down, and where its press went, or `None` where it went nowhere.
#[derive(Clone, Copy, Debug)]
struct Down {
    button: PointerButton,
    target: Option<Target>,
}

// The buttons that the inputs of `OTHERS` hold down.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Extras([Option<Down>; OTHERS.len()]);

// Whether `input` is down in this frame, given whether it was down in the frame before.
fn is_down(input: ControllerInput, buttons: &Buttons, was: Was) -> bool {
    match input {
        ControllerInput::Squeeze => pressed(buttons.squeeze, was),
        ControllerInput::Primary => buttons.primary,
        ControllerInput::Secondary => buttons.secondary,
        ControllerInput::ThumbstickClick => buttons.stick_click,
        ControllerInput::Menu => buttons.menu,
        ControllerInput::Trigger => false,
    }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the other buttons in their own file"
)]
impl Pointer {
    // Sends the presses and the releases of the other buttons. A button goes where the press of
    // the trigger is, or else where the ray points. A button that goes down while the ray points
    // at nothing, or while the hand carries a panel, goes nowhere, and neither does its release.
    // A controller that is lost has every button up.
    pub(super) fn press_others(&mut self, buttons: Buttons, frame: &Frame<'_>, out: &mut Gestures) {
        for (slot, input) in OTHERS.into_iter().enumerate() {
            let was = self.extras.0[slot];
            let now = is_down(input, &buttons, Was::from(was.is_some()));
            match (was, now) {
                (None, true) => {
                    let Some(button) = frame.mapping.button(input) else {
                        continue;
                    };
                    let target = match self.held {
                        Some(_) => None,
                        None => self
                            .press
                            .map(|press| press.target)
                            .or_else(|| self.over.map(|(target, _)| target)),
                    };
                    if let Some(target) = target {
                        self.emit(out, target, Gesture::Press(button));
                    }
                    self.extras.0[slot] = Some(Down { button, target });
                }
                (Some(down), false) => {
                    if let Some(target) = down.target {
                        self.emit(out, target, Gesture::Release(down.button));
                    }
                    self.extras.0[slot] = None;
                }
                _ => {}
            }
        }
    }

    // Scrolls what the pointer is over with the thumbstick, unless the hand carries a panel: the
    // stick pushes that panel then.
    pub(super) fn scroll(&mut self, reach: &Reach, frame: &Frame<'_>, out: &mut Gestures) {
        let speed = frame.mapping.scroll_speed();
        let [lean_x, lean_y] = reach.stick;
        if self.held.is_some() || speed.abs() < f32::EPSILON {
            return;
        }
        if lean_x.abs() < f32::EPSILON && lean_y.abs() < f32::EPSILON {
            return;
        }
        let Some((target, _)) = self.over else {
            return;
        };
        let step = speed * frame.dt.min(LONGEST_STEP);
        // A wheel scrolls up with a positive y, which is the stick leaning forward. The content
        // moves to the left when the stick leans to the right.
        self.emit(
            out,
            target,
            Gesture::Scroll {
                x: -lean_x * step,
                y: lean_y * step,
            },
        );
    }
}
