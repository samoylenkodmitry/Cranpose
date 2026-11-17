//! The controllers' aim and trigger, as OpenXR actions.

use openxr as xr;

use super::session::{XrSession, pose_of};
use crate::{Ray, pointer::Aim};

// The input paths of each hand, left then right.
const HANDS: [&str; 2] = ["/user/hand/left", "/user/hand/right"];
// Each profile: its trigger, and its thumbstick's or trackpad's forward axis if it has one.
// Hands tracked by cameras point and pinch through the hand interaction profile. A runtime that
// lacks a profile's extension rejects its bindings, and the others still bind.
const PROFILES: [(&str, &str, Option<&str>); 18] = [
    (
        "oculus/touch_controller",
        "trigger/value",
        Some("thumbstick/y"),
    ),
    (
        "meta/touch_controller_plus",
        "trigger/value",
        Some("thumbstick/y"),
    ),
    (
        "facebook/touch_controller_pro",
        "trigger/value",
        Some("thumbstick/y"),
    ),
    (
        "valve/index_controller",
        "trigger/value",
        Some("thumbstick/y"),
    ),
    ("htc/vive_controller", "trigger/value", Some("trackpad/y")),
    (
        "htc/vive_cosmos_controller",
        "trigger/value",
        Some("thumbstick/y"),
    ),
    (
        "htc/vive_focus3_controller",
        "trigger/value",
        Some("thumbstick/y"),
    ),
    (
        "microsoft/motion_controller",
        "trigger/value",
        Some("thumbstick/y"),
    ),
    (
        "hp/mixed_reality_controller",
        "trigger/value",
        Some("thumbstick/y"),
    ),
    (
        "samsung/odyssey_controller",
        "trigger/value",
        Some("thumbstick/y"),
    ),
    (
        "bytedance/pico4_controller",
        "trigger/value",
        Some("thumbstick/y"),
    ),
    (
        "bytedance/pico4s_controller",
        "trigger/value",
        Some("thumbstick/y"),
    ),
    (
        "bytedance/pico_neo3_controller",
        "trigger/value",
        Some("thumbstick/y"),
    ),
    ("ml/ml2_controller", "trigger/value", Some("trackpad/y")),
    (
        "khr/generic_controller",
        "trigger/value",
        Some("thumbstick/y"),
    ),
    ("ext/hand_interaction_ext", "aim_activate_ext/value", None),
    ("microsoft/hand_interaction", "select/value", None),
    ("khr/simple_controller", "select/click", None),
];

// Where each controller points and how far its trigger is pulled.
pub(crate) struct Controllers {
    set: xr::ActionSet,
    triggers: [xr::Action<f32>; 2],
    sticks: [xr::Action<f32>; 2],
    aims: [xr::Space; 2],
}

impl Controllers {
    pub(crate) fn new(xr_session: &XrSession) -> Result<Self, String> {
        let set = xr_session
            .instance()
            .create_action_set("pointer", "Pointer", 0)
            .map_err(failed("action set"))?;
        let poses = pair::<xr::Posef>(&set, "aim", "aim")?;
        let triggers = pair::<f32>(&set, "trigger", "trigger")?;
        let sticks = pair::<f32>(&set, "stick", "thumbstick")?;
        for (profile, select, stick) in PROFILES {
            let profile = format!("/interaction_profiles/{profile}");
            if let Err(error) = suggest(xr_session.instance(), &profile, |hand, path| {
                let mut bindings = vec![
                    xr::Binding::new(&poses[hand], path("aim/pose")?),
                    xr::Binding::new(&triggers[hand], path(select)?),
                ];
                if let Some(stick) = stick {
                    bindings.push(xr::Binding::new(&sticks[hand], path(stick)?));
                }
                Ok(bindings)
            }) {
                log::debug!("cranpose xr: no bindings for {profile}: {error}");
            }
        }
        xr_session
            .session
            .attach_action_sets(&[&set])
            .map_err(failed("attach actions"))?;
        let space = |pose: &xr::Action<xr::Posef>| {
            pose.create_space(&xr_session.session, xr::Path::NULL, xr::Posef::IDENTITY)
                .map_err(failed("aim space"))
        };
        let aims = [space(&poses[0])?, space(&poses[1])?];
        Ok(Self {
            set,
            triggers,
            sticks,
            aims,
        })
    }

    // The controllers at `time`, left then right. A controller the runtime does not track has
    // none.
    pub(crate) fn aims(&self, xr_session: &XrSession, time: xr::Time) -> [Option<Aim>; 2] {
        if let Err(error) = xr_session
            .session
            .sync_actions(&[xr::ActiveActionSet::new(&self.set)])
        {
            log::debug!("cranpose xr: actions: {error}");
            return [None, None];
        }
        [0, 1].map(|hand| {
            let location = self.aims[hand].locate(&xr_session.local, time).ok()?;
            let tracked =
                xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID;
            if !location.location_flags.contains(tracked) {
                return None;
            }
            let value = |action: &xr::Action<f32>| {
                action
                    .state(&xr_session.session, xr::Path::NULL)
                    .ok()
                    .filter(|state| state.is_active)
                    .map_or(0.0, |state| state.current_state)
            };
            let pose = pose_of(&location.pose);
            Some(Aim {
                ray: Ray::of_aim(&pose),
                pose,
                trigger: value(&self.triggers[hand]),
                stick: value(&self.sticks[hand]),
            })
        })
    }
}

fn failed(what: &str) -> impl Fn(xr::sys::Result) -> String + use<> {
    let what = what.to_owned();
    move |error| format!("OpenXR {what}: {error}")
}

// An action for each hand, named `left_{name}` and `right_{name}`.
fn pair<T: xr::ActionTy>(
    set: &xr::ActionSet,
    name: &str,
    label: &str,
) -> Result<[xr::Action<T>; 2], String> {
    let action = |side: &str, title: &str| {
        set.create_action::<T>(&format!("{side}_{name}"), &format!("{title} {label}"), &[])
            .map_err(failed("action"))
    };
    Ok([action("left", "Left")?, action("right", "Right")?])
}

// Suggests the bindings of `profile` that `bindings` gives for each hand, from a function that
// turns an input's name into its path on that hand.
fn suggest<'a>(
    instance: &xr::Instance,
    profile: &str,
    bindings: impl Fn(
        usize,
        &dyn Fn(&str) -> Result<xr::Path, String>,
    ) -> Result<Vec<xr::Binding<'a>>, String>,
) -> Result<(), String> {
    let path = |path: &str| instance.string_to_path(path).map_err(failed("path"));
    let mut all = Vec::new();
    for (hand, side) in HANDS.iter().enumerate() {
        all.extend(bindings(hand, &|name| {
            path(&format!("{side}/input/{name}"))
        })?);
    }
    instance
        .suggest_interaction_profile_bindings(path(profile)?, &all)
        .map_err(failed("bindings"))
}
