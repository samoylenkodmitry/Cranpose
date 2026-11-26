//! The controllers' action set: its bindings, and what the hands report in each frame.

use openxr as xr;

use super::{
    hand_actions::{HandActions, HandInput, failed},
    session::XrSession,
};
use crate::{
    Hand, InteractionProfile, XrFrame, controller_bindings::bindings, haptics::HapticCommand,
    session_status::SessionStatus,
};

// The user path of each hand, left then right.
const USERS: [&str; 2] = ["/user/hand/left", "/user/hand/right"];

// The hands' inputs and outputs, as OpenXR actions. Hands tracked by cameras point and pinch
// through the hand interaction profiles. A runtime that lacks a profile's extension rejects its
// bindings, and the others still bind.
pub(crate) struct Controllers {
    set: xr::ActionSet,
    hands: [HandInput; 2],
    users: [xr::Path; 2],
    // The path of each profile, to name the one that the runtime reports for a hand
    profiles: Vec<(xr::Path, InteractionProfile)>,
}

impl Controllers {
    // Attaches the controllers' set, and `extra` with it when the app has a set of its own: a
    // session attaches its action sets once.
    pub(crate) fn new(
        xr_session: &XrSession,
        extra: Option<&xr::ActionSet>,
    ) -> Result<Self, String> {
        let instance = xr_session.instance();
        let set = instance
            .create_action_set("controllers", "Controllers", 0)
            .map_err(failed("action set"))?;
        let actions = [
            HandActions::new(&set, "left")?,
            HandActions::new(&set, "right")?,
        ];
        bind_profiles(instance, &actions);
        let sets: Vec<&xr::ActionSet> = std::iter::once(&set).chain(extra).collect();
        xr_session
            .session
            .attach_action_sets(&sets)
            .map_err(failed("attach actions"))?;
        let [left, right] = actions;
        let path = |path: &str| instance.string_to_path(path).map_err(failed("path"));
        let mut profiles = Vec::with_capacity(InteractionProfile::ALL.len());
        for profile in InteractionProfile::ALL {
            profiles.push((path(profile.path())?, profile));
        }
        Ok(Self {
            set,
            hands: [
                HandInput::new(left, &xr_session.session)?,
                HandInput::new(right, &xr_session.session)?,
            ],
            users: [path(USERS[0])?, path(USERS[1])?],
            profiles,
        })
    }

    // Writes the controllers of `frame` at `time`, and syncs `extra` with them. While the app has
    // no input focus, or the runtime does not sync the actions, the controllers stay at rest.
    pub(crate) fn sample(
        &self,
        xr_session: &XrSession,
        time: xr::Time,
        status: SessionStatus,
        extra: Option<&xr::ActionSet>,
        frame: &mut XrFrame,
    ) {
        let synced = status.listens() && self.sync(xr_session, extra);
        let controllers = [&mut frame.left_controller, &mut frame.right_controller];
        for ((hand, user), controller) in self.hands.iter().zip(self.users).zip(controllers) {
            let before = std::mem::take(controller);
            if synced {
                hand.read(xr_session, time, controller);
                controller.profile = self.profile_of(xr_session, user);
                controller.settle_presses(&before);
            }
        }
    }

    // The space of the aim pose of the controller in `hand`.
    pub(crate) fn aim_space(&self, hand: Hand) -> xr::sys::Space {
        self.hands[hand.index()].aim_space()
    }

    // Plays or stops the vibrations that the app asked for, left hand first. A hand that the
    // runtime does not track drops its request.
    pub(crate) fn vibrate(&self, xr_session: &XrSession, commands: [Option<HapticCommand>; 2]) {
        for ((hand, command), side) in self.hands.iter().zip(commands).zip(Hand::BOTH) {
            let result = match command {
                Some(HapticCommand::Vibrate(vibration)) => {
                    hand.vibrate(&xr_session.session, Some(&vibration))
                }
                Some(HapticCommand::Stop) => hand.vibrate(&xr_session.session, None),
                None => continue,
            };
            if let Err(error) = result {
                log::debug!("cranpose xr: haptics of the {side:?} hand: {error}");
            }
        }
    }

    fn sync(&self, xr_session: &XrSession, extra: Option<&xr::ActionSet>) -> bool {
        let own = xr::ActiveActionSet::new(&self.set);
        let (both, alone);
        let active: &[xr::ActiveActionSet<'_>] = match extra {
            Some(extra) => {
                both = [own, xr::ActiveActionSet::new(extra)];
                &both
            }
            None => {
                alone = [own];
                &alone
            }
        };
        match xr_session.session.sync_actions(active) {
            Ok(()) => true,
            Err(error) => {
                log::debug!("cranpose xr: actions: {error}");
                false
            }
        }
    }

    // The profile that the runtime reports for the hand at `user`, or `None` while it reports
    // none or one that the backend did not suggest.
    fn profile_of(&self, xr_session: &XrSession, user: xr::Path) -> Option<InteractionProfile> {
        let reported = xr_session.session.current_interaction_profile(user).ok()?;
        self.profiles
            .iter()
            .find(|(path, _)| *path == reported)
            .map(|&(_, profile)| profile)
    }
}

// Suggests the bindings of every profile. A runtime rejects a profile's whole list when one path
// is not the profile's, so a rejected profile binds the pointer's inputs only. A table mistake
// then cannot cost the pointer. A profile that the runtime does not know, because its extension
// is off, binds nothing.
fn bind_profiles(instance: &xr::Instance, actions: &[HandActions; 2]) {
    let (mut all, mut pointer_only, mut none) = (Vec::new(), Vec::new(), Vec::new());
    for profile in InteractionProfile::ALL {
        match suggest(instance, actions, profile, false) {
            Ok(()) => all.push(profile),
            Err(error) => match suggest(instance, actions, profile, true) {
                Ok(()) => {
                    log::warn!("cranpose xr: {profile:?} rejects its inputs: {error}");
                    pointer_only.push(profile);
                }
                Err(_) => none.push(profile),
            },
        }
    }
    log::info!(
        "cranpose xr: bindings: all inputs of {all:?}; the pointer's inputs of {pointer_only:?}; none for {} unknown profiles",
        none.len()
    );
}

// Suggests the bindings of `profile` for both hands: every input, or the pointer's only.
fn suggest(
    instance: &xr::Instance,
    actions: &[HandActions; 2],
    profile: InteractionProfile,
    pointer_only: bool,
) -> Result<(), String> {
    let path = |path: &str| instance.string_to_path(path).map_err(failed("path"));
    let mut suggested = Vec::new();
    for (hand, user) in Hand::BOTH.into_iter().zip(USERS) {
        let lists = bindings(profile, hand);
        for &(input, name) in lists.iter().flat_map(|list| list.iter()) {
            if !pointer_only || input.points() {
                suggested
                    .push(actions[hand.index()].binding(input, path(&format!("{user}/{name}"))?));
            }
        }
    }
    instance
        .suggest_interaction_profile_bindings(path(profile.path())?, &suggested)
        .map_err(failed("bindings"))
}
