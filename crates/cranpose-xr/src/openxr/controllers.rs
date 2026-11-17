//! The controllers' aim and trigger, as OpenXR actions.

use openxr as xr;

use super::session::{XrSession, pose_of};
use crate::{Ray, pointer::Aim};

// The input paths of each hand, left then right.
const HANDS: [&str; 2] = ["/user/hand/left", "/user/hand/right"];

// Where each controller points and whether its trigger is down.
pub(crate) struct Controllers {
    set: xr::ActionSet,
    selects: [xr::Action<bool>; 2],
    aims: [xr::Space; 2],
}

impl Controllers {
    pub(crate) fn new(xr_session: &XrSession) -> Result<Self, String> {
        let instance = xr_session.instance();
        let failed = |what: &str| {
            let what = what.to_owned();
            move |error: xr::sys::Result| format!("OpenXR {what}: {error}")
        };
        let set = instance
            .create_action_set("pointer", "Pointer", 0)
            .map_err(failed("action set"))?;
        let poses = [
            set.create_action::<xr::Posef>("left_aim", "Left aim", &[])
                .map_err(failed("action"))?,
            set.create_action::<xr::Posef>("right_aim", "Right aim", &[])
                .map_err(failed("action"))?,
        ];
        let selects = [
            set.create_action::<bool>("left_select", "Left select", &[])
                .map_err(failed("action"))?,
            set.create_action::<bool>("right_select", "Right select", &[])
                .map_err(failed("action"))?,
        ];
        let path = |path: String| instance.string_to_path(&path).map_err(failed("path"));
        for (profile, select) in [
            (
                "/interaction_profiles/oculus/touch_controller",
                "trigger/value",
            ),
            (
                "/interaction_profiles/khr/simple_controller",
                "select/click",
            ),
        ] {
            let mut bindings = Vec::new();
            for hand in 0..2 {
                bindings.push(xr::Binding::new(
                    &poses[hand],
                    path(format!("{}/input/aim/pose", HANDS[hand]))?,
                ));
                bindings.push(xr::Binding::new(
                    &selects[hand],
                    path(format!("{}/input/{select}", HANDS[hand]))?,
                ));
            }
            let profile = path(profile.to_owned())?;
            if let Err(error) = instance.suggest_interaction_profile_bindings(profile, &bindings) {
                log::warn!("cranpose xr: bindings for a controller: {error}");
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
        Ok(Self { set, selects, aims })
    }

    // The controllers at `time`, left then right. A controller the runtime does not track has
    // none.
    pub(crate) fn aims(&self, xr_session: &XrSession, time: xr::Time) -> [Option<Aim>; 2] {
        if let Err(error) = xr_session
            .session
            .sync_actions(&[xr::ActiveActionSet::new(&self.set)])
        {
            // The session syncs nothing while it is not focused, such as under the system menu
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
            let select = self.selects[hand]
                .state(&xr_session.session, xr::Path::NULL)
                .is_ok_and(|state| state.is_active && state.current_state);
            Some(Aim {
                ray: Ray::of_aim(&pose_of(&location.pose)),
                select,
            })
        })
    }
}
