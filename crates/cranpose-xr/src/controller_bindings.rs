//! The inputs of each interaction profile that the backend binds, from the OpenXR specification's
//! profile sections.
//!
//! A runtime rejects all bindings of a profile when one path does not belong to it, so each list
//! holds only the paths that the specification gives to that profile for that hand.

use Input::*;

use crate::{Hand, InteractionProfile};

// What an action of a hand reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Input {
    AimPose,
    GripPose,
    Trigger,
    TriggerTouch,
    Squeeze,
    StickX,
    StickY,
    StickClick,
    StickTouch,
    PrimaryClick,
    PrimaryTouch,
    SecondaryClick,
    SecondaryTouch,
    Menu,
    ThumbrestTouch,
    Haptic,
}

impl Input {
    // The inputs that the pointer needs to point and to scroll: where it aims, whether it presses
    // and how far a stick leans. A profile whose full list the runtime rejects still binds these.
    pub(crate) const fn points(self) -> bool {
        matches!(
            self,
            Self::AimPose | Self::Trigger | Self::StickX | Self::StickY
        )
    }
}

// An input and its path below the hand's user path, such as `input/trigger/value`.
pub(crate) type Binding = (Input, &'static str);

const POSES: &[Binding] = &[(AimPose, "input/aim/pose"), (GripPose, "input/grip/pose")];
const HAPTIC: &[Binding] = &[(Haptic, "output/haptic")];
const TRIGGER: &[Binding] = &[(Trigger, "input/trigger/value")];
const TRIGGER_TOUCHED: &[Binding] = &[
    (Trigger, "input/trigger/value"),
    (TriggerTouch, "input/trigger/touch"),
];
const SQUEEZE_VALUE: &[Binding] = &[(Squeeze, "input/squeeze/value")];
const SQUEEZE_CLICK: &[Binding] = &[(Squeeze, "input/squeeze/click")];
const STICK: &[Binding] = &[
    (StickX, "input/thumbstick/x"),
    (StickY, "input/thumbstick/y"),
    (StickClick, "input/thumbstick/click"),
];
const STICK_TOUCHED: &[Binding] = &[
    (StickX, "input/thumbstick/x"),
    (StickY, "input/thumbstick/y"),
    (StickClick, "input/thumbstick/click"),
    (StickTouch, "input/thumbstick/touch"),
];
// A trackpad stands in for the thumbstick.
const TRACKPAD: &[Binding] = &[
    (StickX, "input/trackpad/x"),
    (StickY, "input/trackpad/y"),
    (StickClick, "input/trackpad/click"),
    (StickTouch, "input/trackpad/touch"),
];
const THUMBREST: &[Binding] = &[(ThumbrestTouch, "input/thumbrest/touch")];
const MENU: &[Binding] = &[(Menu, "input/menu/click")];
const XY: &[Binding] = &[
    (PrimaryClick, "input/x/click"),
    (SecondaryClick, "input/y/click"),
];
const XY_TOUCHED: &[Binding] = &[
    (PrimaryClick, "input/x/click"),
    (PrimaryTouch, "input/x/touch"),
    (SecondaryClick, "input/y/click"),
    (SecondaryTouch, "input/y/touch"),
];
const AB: &[Binding] = &[
    (PrimaryClick, "input/a/click"),
    (SecondaryClick, "input/b/click"),
];
const AB_TOUCHED: &[Binding] = &[
    (PrimaryClick, "input/a/click"),
    (PrimaryTouch, "input/a/touch"),
    (SecondaryClick, "input/b/click"),
    (SecondaryTouch, "input/b/touch"),
];
const PRIMARY_SECONDARY: &[Binding] = &[
    (PrimaryClick, "input/primary/click"),
    (SecondaryClick, "input/secondary/click"),
];
// Hands that cameras track: the aim's pinch is the trigger and the grasp is the squeeze.
const HAND_AIM_ACTIVATE: &[Binding] = &[(Trigger, "input/aim_activate_ext/value")];
const HAND_GRASP: &[Binding] = &[(Squeeze, "input/grasp_ext/value")];
const HAND_SELECT: &[Binding] = &[(Trigger, "input/select/value")];
const SIMPLE_SELECT: &[Binding] = &[(Trigger, "input/select/click")];

type Lists = &'static [&'static [Binding]];

// The Touch controllers: face buttons with touch on each hand, a menu button on the left.
const TOUCH_LEFT: Lists = &[
    POSES,
    HAPTIC,
    TRIGGER_TOUCHED,
    SQUEEZE_VALUE,
    STICK_TOUCHED,
    THUMBREST,
    XY_TOUCHED,
    MENU,
];
const TOUCH_RIGHT: Lists = &[
    POSES,
    HAPTIC,
    TRIGGER_TOUCHED,
    SQUEEZE_VALUE,
    STICK_TOUCHED,
    THUMBREST,
    AB_TOUCHED,
];
const INDEX: Lists = &[
    POSES,
    HAPTIC,
    TRIGGER_TOUCHED,
    SQUEEZE_VALUE,
    STICK_TOUCHED,
    AB_TOUCHED,
];
const VIVE: Lists = &[POSES, HAPTIC, TRIGGER, SQUEEZE_CLICK, TRACKPAD, MENU];
const COSMOS_LEFT: Lists = &[
    POSES,
    HAPTIC,
    TRIGGER,
    SQUEEZE_CLICK,
    STICK_TOUCHED,
    XY,
    MENU,
];
const COSMOS_RIGHT: Lists = &[POSES, HAPTIC, TRIGGER, SQUEEZE_CLICK, STICK_TOUCHED, AB];
const FOCUS3_LEFT: Lists = &[
    POSES,
    HAPTIC,
    TRIGGER_TOUCHED,
    SQUEEZE_VALUE,
    STICK_TOUCHED,
    THUMBREST,
    XY,
    MENU,
];
const FOCUS3_RIGHT: Lists = &[
    POSES,
    HAPTIC,
    TRIGGER_TOUCHED,
    SQUEEZE_VALUE,
    STICK_TOUCHED,
    THUMBREST,
    AB,
];
// The Microsoft motion controllers and the Samsung Odyssey's.
const MOTION: Lists = &[POSES, HAPTIC, TRIGGER, SQUEEZE_CLICK, STICK, MENU];
const HP_LEFT: Lists = &[POSES, HAPTIC, TRIGGER, SQUEEZE_VALUE, STICK, MENU, XY];
const HP_RIGHT: Lists = &[POSES, HAPTIC, TRIGGER, SQUEEZE_VALUE, STICK, MENU, AB];
// PICO 4 has the menu button on the left only. PICO Neo 3 has it on both hands.
const PICO4_LEFT: Lists = &[
    POSES,
    HAPTIC,
    TRIGGER_TOUCHED,
    SQUEEZE_VALUE,
    STICK_TOUCHED,
    XY_TOUCHED,
    MENU,
];
const PICO4_RIGHT: Lists = &[
    POSES,
    HAPTIC,
    TRIGGER_TOUCHED,
    SQUEEZE_VALUE,
    STICK_TOUCHED,
    AB_TOUCHED,
];
const NEO3_RIGHT: Lists = &[
    POSES,
    HAPTIC,
    TRIGGER_TOUCHED,
    SQUEEZE_VALUE,
    STICK_TOUCHED,
    AB_TOUCHED,
    MENU,
];
const MAGIC_LEAP: Lists = &[POSES, HAPTIC, TRIGGER, TRACKPAD, MENU];
const GENERIC: Lists = &[
    POSES,
    HAPTIC,
    TRIGGER,
    SQUEEZE_VALUE,
    STICK,
    PRIMARY_SECONDARY,
];
const EXT_HAND: Lists = &[POSES, HAND_AIM_ACTIVATE, HAND_GRASP];
const MICROSOFT_HAND: Lists = &[POSES, HAND_SELECT, SQUEEZE_VALUE];
const SIMPLE: Lists = &[POSES, HAPTIC, SIMPLE_SELECT, MENU];

// Each profile with the lists of inputs that it gives the left hand and the right hand.
const PROFILES: [(InteractionProfile, Lists, Lists); 18] = {
    use InteractionProfile as P;
    [
        (P::OculusTouch, TOUCH_LEFT, TOUCH_RIGHT),
        (P::MetaTouchPlus, TOUCH_LEFT, TOUCH_RIGHT),
        (P::MetaTouchPro, TOUCH_LEFT, TOUCH_RIGHT),
        (P::ValveIndex, INDEX, INDEX),
        (P::HtcVive, VIVE, VIVE),
        (P::HtcViveCosmos, COSMOS_LEFT, COSMOS_RIGHT),
        (P::HtcViveFocus3, FOCUS3_LEFT, FOCUS3_RIGHT),
        (P::MicrosoftMotion, MOTION, MOTION),
        (P::HpMixedReality, HP_LEFT, HP_RIGHT),
        (P::SamsungOdyssey, MOTION, MOTION),
        (P::Pico4, PICO4_LEFT, PICO4_RIGHT),
        (P::Pico4S, PICO4_LEFT, PICO4_RIGHT),
        (P::PicoNeo3, PICO4_LEFT, NEO3_RIGHT),
        (P::MagicLeap2, MAGIC_LEAP, MAGIC_LEAP),
        (P::KhrGeneric, GENERIC, GENERIC),
        (P::ExtHand, EXT_HAND, EXT_HAND),
        (P::MicrosoftHand, MICROSOFT_HAND, MICROSOFT_HAND),
        (P::KhrSimple, SIMPLE, SIMPLE),
    ]
};

// The lists of inputs that `profile` gives `hand`. A profile that has no row gives none.
pub(crate) fn bindings(profile: InteractionProfile, hand: Hand) -> Lists {
    PROFILES
        .iter()
        .find(|(known, _, _)| *known == profile)
        .map_or(&[], |&(_, left, right)| match hand {
            Hand::Left => left,
            Hand::Right => right,
        })
}

#[cfg(test)]
#[path = "tests/controller_bindings_tests.rs"]
mod tests;
