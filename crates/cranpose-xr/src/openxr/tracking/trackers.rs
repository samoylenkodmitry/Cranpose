//! The trackers of a session: made once the user grants their permissions, sampled in each frame.

use std::time::Duration;

use openxr as xr;

use super::{
    body_tracker::BodyTracker, eye_gaze_action::EyeGazeAction, eye_tracker::EyeTracker,
    face_tracker::FaceTracker, hand_trackers::HandTrackers, tracking_setup::TrackingSetup,
};
use crate::{
    TrackingAvailability, TrackingCapabilities, TrackingFeature, XrFrame,
    openxr::{
        graphics::Api,
        host::Host,
        session::{SessionStatus, XrSession},
    },
    tracking_permission::{PermissionAnswer, PermissionStep, Prompt, may_try, permission_step},
    tracking_request::service_of,
};

// How long the backend waits between two looks at a pending feature: its permissions, and a new try
// to make its tracker. A frame that falls between does nothing for it.
const POLL: Duration = Duration::from_secs(1);

// The most groups of permissions that a feature has
const MOST_GROUPS: usize = 2;

// The trackers for what the app asked, with their permissions. An app that asked for nothing has
// none, and `sample` returns at once.
pub(crate) struct Trackers {
    capabilities: TrackingCapabilities,
    setup: TrackingSetup,
    // Whether the system's prompt showed for each feature
    prompts: [Prompt; TrackingFeature::ALL.len()],
    last_poll: Option<Duration>,
    hands: Option<HandTrackers>,
    body: Option<BodyTracker>,
    face: Option<FaceTracker>,
    eyes: Option<EyeTracker>,
    gaze: Option<EyeGazeAction>,
}

impl Trackers {
    pub(crate) fn new(setup: TrackingSetup, gaze: Option<EyeGazeAction>) -> Self {
        Self {
            capabilities: setup.capabilities,
            setup,
            prompts: [Prompt::NotShown; TrackingFeature::ALL.len()],
            last_poll: None,
            hands: None,
            body: None,
            face: None,
            eyes: None,
            gaze,
        }
    }

    // The gaze action's set, which each frame syncs with the controllers'.
    pub(crate) fn gaze_set(&self) -> Option<&xr::ActionSet> {
        self.gaze.as_ref().map(EyeGazeAction::set)
    }

    // Looks at the permissions of the features that wait for them, once a second, makes the trackers
    // that can run and writes what each tracker gives into `frame`. While the app has no input
    // focus the runtime gives no data, and the frame's tracking stays at rest.
    pub(crate) fn sample(
        &mut self,
        xr_session: &XrSession,
        host: &Host,
        now: Duration,
        time: xr::Time,
        status: SessionStatus,
        frame: &mut XrFrame,
    ) {
        if !self.setup.request.asks_any() {
            return;
        }
        if self.pending() && self.poll_due(now) {
            for feature in TrackingFeature::ALL {
                if self.waits(feature) {
                    self.advance(feature, xr_session, host);
                }
            }
        }
        frame.tracking = self.capabilities;
        if status.listens() {
            self.locate(xr_session, time, frame);
        } else {
            Self::rest(frame);
        }
    }

    fn availability(&self, feature: TrackingFeature) -> TrackingAvailability {
        self.capabilities.availability(feature)
    }

    // Whether the app asked for the feature and it has neither a tracker nor a refusal.
    fn waits(&self, feature: TrackingFeature) -> bool {
        self.setup.request.asks(feature)
            && matches!(
                self.availability(feature),
                TrackingAvailability::AwaitingPermission
            )
    }

    fn pending(&self) -> bool {
        TrackingFeature::ALL
            .into_iter()
            .any(|feature| self.waits(feature))
    }

    fn poll_due(&mut self, now: Duration) -> bool {
        let due = self
            .last_poll
            .is_none_or(|last| now.saturating_sub(last) >= POLL);
        if due {
            self.last_poll = Some(now);
        }
        due
    }

    // One look at a waiting feature: shows the prompt once, and tries to make the tracker unless the
    // user refused. A runtime that needs no permission makes it at the first look, and one that
    // lacks the permission refuses it until the user grants it.
    fn advance(&mut self, feature: TrackingFeature, xr_session: &XrSession, host: &Host) {
        let groups = service_of(feature).android_permission_groups();
        let mut answers = [PermissionAnswer::Granted; MOST_GROUPS];
        for (answer, group) in answers.iter_mut().zip(groups) {
            *answer = host.permission_answer(group);
        }
        let answers = &answers[..groups.len().min(MOST_GROUPS)];
        let step = permission_step(answers, self.prompts[feature.index()]);
        if step == PermissionStep::Refused {
            log::info!("cranpose xr: {feature:?} tracking: the user refused the permission");
            self.set(feature, TrackingAvailability::PermissionRefused);
            return;
        }
        if step == PermissionStep::Request {
            let wanted: Vec<&str> = groups
                .iter()
                .zip(answers)
                .filter(|(_, answer)| **answer != PermissionAnswer::Granted)
                .flat_map(|(group, _)| group.iter().copied())
                .collect();
            log::info!("cranpose xr: {feature:?} tracking: asks for {wanted:?}");
            host.request_permissions(&wanted);
            self.prompts[feature.index()] = Prompt::Shown;
        }
        if !may_try(answers, step) {
            return;
        }
        let ready = step == PermissionStep::Ready;
        match self.make(feature, xr_session, ready) {
            Ok(true) => {
                log::info!("cranpose xr: {feature:?} tracking runs");
                self.set(feature, TrackingAvailability::Active);
            }
            Ok(false) => {}
            // With the permissions granted the runtime still refuses: it has no such tracker, and
            // a new try would only repeat the refusal
            Err(error) if ready && error != xr::sys::Result::ERROR_PERMISSION_INSUFFICIENT => {
                log::warn!("cranpose xr: {feature:?} tracking is not available: {error}");
                self.set(feature, TrackingAvailability::Unavailable);
            }
            Err(error) => log::debug!("cranpose xr: {feature:?} tracker: {error}"),
        }
    }

    fn set(&mut self, feature: TrackingFeature, availability: TrackingAvailability) {
        self.capabilities = self.capabilities.with_availability(feature, availability);
    }

    // Makes the tracker of the feature. Returns whether the feature runs, or the result that the
    // runtime refused the tracker with. A feature that waits for its permission is `Ok(false)`.
    fn make(
        &mut self,
        feature: TrackingFeature,
        xr_session: &XrSession,
        permitted: bool,
    ) -> Result<bool, xr::sys::Result> {
        let (instance, session) = (xr_session.instance(), &xr_session.session);
        match feature {
            TrackingFeature::Hands => {
                self.hands = Some(HandTrackers::create(
                    session,
                    instance,
                    &self.setup.enabled,
                )?);
            }
            TrackingFeature::Body => {
                self.body = Some(BodyTracker::create(session, instance, &self.capabilities)?);
            }
            TrackingFeature::Face => {
                let provider = self
                    .capabilities
                    .face_tracking_provider()
                    .ok_or(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT)?;
                self.face = Some(FaceTracker::create(provider, session, instance)?);
            }
            TrackingFeature::Eyes => return self.make_eyes(session, instance, permitted),
        }
        Ok(true)
    }

    // The eyes run with the social tracker or, once permitted, with the gaze action.
    fn make_eyes(
        &mut self,
        session: &xr::Session<Api>,
        instance: &xr::Instance,
        permitted: bool,
    ) -> Result<bool, xr::sys::Result> {
        let mut refused = None;
        if self.capabilities.is_eye_tracking_social_available() && self.eyes.is_none() {
            match EyeTracker::create(session, instance) {
                Ok(tracker) => self.eyes = Some(tracker),
                Err(error) => refused = Some(error),
            }
        }
        if self.eyes.is_some() || (self.gaze.is_some() && permitted) {
            Ok(true)
        } else {
            refused.map_or(Ok(false), Err)
        }
    }

    fn locate(&self, xr_session: &XrSession, time: xr::Time, frame: &mut XrFrame) {
        let base = xr_session.local.as_raw();
        if let Some(hands) = &self.hands {
            hands.locate(base, time, [&mut frame.left_hand, &mut frame.right_hand]);
        }
        if let Some(body) = &self.body {
            body.locate(base, time, &mut frame.body);
        }
        if let Some(face) = &self.face {
            face.locate(time, &mut frame.face, &mut frame.meta_face);
        }
        if self.availability(TrackingFeature::Eyes) == TrackingAvailability::Active {
            if let Some(eyes) = &self.eyes {
                eyes.locate(base, time, &mut frame.eyes);
            }
            frame.eyes.gaze = self
                .gaze
                .as_ref()
                .and_then(|gaze| gaze.locate(xr_session, time));
        }
    }

    // Puts what the trackers fill back to rest.
    fn rest(frame: &mut XrFrame) {
        frame.left_hand = Default::default();
        frame.right_hand = Default::default();
        frame.body = Default::default();
        frame.face = Default::default();
        frame.meta_face = Default::default();
        frame.eyes = Default::default();
    }
}
