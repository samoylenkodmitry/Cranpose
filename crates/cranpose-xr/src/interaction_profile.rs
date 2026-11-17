//! The kinds of controller that the backend binds.

/// The kind of controller or hand input that a runtime reports for a hand: an OpenXR interaction
/// profile. [`Controller::profile`](crate::Controller::profile) tells which one is active.
///
/// The variants are the profiles that the backend suggests bindings for. A runtime that maps a
/// controller to another of them reports that one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InteractionProfile {
    /// The Oculus Touch controllers of the Rift and the Quest.
    OculusTouch,
    /// The Meta Touch Plus controllers of the Quest 3.
    MetaTouchPlus,
    /// The Meta Touch Pro controllers.
    MetaTouchPro,
    /// The Valve Index controllers.
    ValveIndex,
    /// The HTC Vive wands.
    HtcVive,
    /// The HTC Vive Cosmos controllers.
    HtcViveCosmos,
    /// The HTC Vive Focus 3 controllers.
    HtcViveFocus3,
    /// The Microsoft Mixed Reality motion controllers.
    MicrosoftMotion,
    /// The HP Reverb G2 controllers.
    HpMixedReality,
    /// The Samsung Odyssey controllers.
    SamsungOdyssey,
    /// The PICO 4 controllers.
    Pico4,
    /// The PICO 4 Ultra controllers.
    Pico4S,
    /// The PICO Neo 3 controllers.
    PicoNeo3,
    /// The Magic Leap 2 controller.
    MagicLeap2,
    /// The Khronos generic controller, which a runtime maps to any controller it knows.
    KhrGeneric,
    /// Hands that cameras track, through `XR_EXT_hand_interaction`.
    ExtHand,
    /// Hands that cameras track, through Microsoft's hand interaction.
    MicrosoftHand,
    /// The Khronos simple controller, which has a select button and a menu button.
    KhrSimple,
}

impl InteractionProfile {
    // Every profile, in the order in which the backend suggests them.
    #[cfg_attr(
        not(any(test, all(feature = "openxr", target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) const ALL: [Self; 18] = [
        Self::OculusTouch,
        Self::MetaTouchPlus,
        Self::MetaTouchPro,
        Self::ValveIndex,
        Self::HtcVive,
        Self::HtcViveCosmos,
        Self::HtcViveFocus3,
        Self::MicrosoftMotion,
        Self::HpMixedReality,
        Self::SamsungOdyssey,
        Self::Pico4,
        Self::Pico4S,
        Self::PicoNeo3,
        Self::MagicLeap2,
        Self::KhrGeneric,
        Self::ExtHand,
        Self::MicrosoftHand,
        Self::KhrSimple,
    ];

    /// The profile's OpenXR path, such as `/interaction_profiles/oculus/touch_controller`.
    pub const fn path(self) -> &'static str {
        match self {
            Self::OculusTouch => "/interaction_profiles/oculus/touch_controller",
            Self::MetaTouchPlus => "/interaction_profiles/meta/touch_controller_plus",
            Self::MetaTouchPro => "/interaction_profiles/facebook/touch_controller_pro",
            Self::ValveIndex => "/interaction_profiles/valve/index_controller",
            Self::HtcVive => "/interaction_profiles/htc/vive_controller",
            Self::HtcViveCosmos => "/interaction_profiles/htc/vive_cosmos_controller",
            Self::HtcViveFocus3 => "/interaction_profiles/htc/vive_focus3_controller",
            Self::MicrosoftMotion => "/interaction_profiles/microsoft/motion_controller",
            Self::HpMixedReality => "/interaction_profiles/hp/mixed_reality_controller",
            Self::SamsungOdyssey => "/interaction_profiles/samsung/odyssey_controller",
            Self::Pico4 => "/interaction_profiles/bytedance/pico4_controller",
            Self::Pico4S => "/interaction_profiles/bytedance/pico4s_controller",
            Self::PicoNeo3 => "/interaction_profiles/bytedance/pico_neo3_controller",
            Self::MagicLeap2 => "/interaction_profiles/ml/ml2_controller",
            Self::KhrGeneric => "/interaction_profiles/khr/generic_controller",
            Self::ExtHand => "/interaction_profiles/ext/hand_interaction_ext",
            Self::MicrosoftHand => "/interaction_profiles/microsoft/hand_interaction",
            Self::KhrSimple => "/interaction_profiles/khr/simple_controller",
        }
    }
}
