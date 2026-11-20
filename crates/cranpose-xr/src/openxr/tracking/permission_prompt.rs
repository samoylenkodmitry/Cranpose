//! The system's prompt for the permissions that tracking needs, shown before the session starts.

use std::time::{Duration, Instant};

use cranpose::capabilities::Service;

use crate::{
    TrackingAvailability, TrackingFeature,
    extension_offer::ExtensionOffer,
    openxr::host::Host,
    session_status::SessionStatus,
    spatial_setup::SpatialSetup,
    tracking_permission::{PermissionAnswer, PermissionStep, permission_step},
    tracking_request::service_of,
};

// How long the app waits for the user to answer before it starts anyway. A feature that is still
// unanswered then stays `AwaitingPermission`, and a later grant makes its tracker.
const PATIENCE: Duration = Duration::from_secs(300);

// The most groups of permissions that a feature has
const MOST_GROUPS: usize = 2;

// Where the feature stands, from what the user answered so far.
pub(crate) fn step_of(host: &Host, feature: TrackingFeature) -> PermissionStep {
    step_of_service(host, service_of(feature))
}

// Where the service stands, from what the user answered so far.
pub(crate) fn step_of_service(host: &Host, service: Service) -> PermissionStep {
    let groups = service.android_permission_groups();
    let mut answers = [PermissionAnswer::Granted; MOST_GROUPS];
    for (answer, group) in answers.iter_mut().zip(groups) {
        *answer = host.permission_answer(group);
    }
    permission_step(&answers[..groups.len().min(MOST_GROUPS)])
}

// The services of the anchors and planes that wait for the user's answer to a prompt.
fn spatial_services(spatial: &SpatialSetup) -> impl Iterator<Item = Service> {
    [
        (spatial.anchors.persistence == TrackingAvailability::AwaitingPermission)
            .then_some(Service::Anchors),
        (spatial.planes == TrackingAvailability::AwaitingPermission)
            .then_some(Service::PlaneTracking),
    ]
    .into_iter()
    .flatten()
}

// Shows the system's prompt for the permissions of every feature that the app asked for and the
// runtime offers (`enabled` holds the extensions of the tracking, and `spatial` those of the anchors
// and planes), in one request, and waits for the user to answer. An immersive app that asks while its
// session runs loses the session: on a Meta Quest the system takes the app off the headset to show the
// prompt, and the runtime ends the session. So the prompt comes first, while the app has no session
// yet. The wait handles the host's events, so the activity stays responsive. It ends when every such
// feature is answered, when the activity ends or after `PATIENCE`.
pub(crate) fn ask_before_session(host: &Host, enabled: ExtensionOffer, spatial: &SpatialSetup) {
    let mut wanted: Vec<&'static str> = Vec::new();
    let asked = || {
        TrackingFeature::ALL
            .into_iter()
            .filter(move |feature| enabled.offers(*feature))
            .map(service_of)
            .chain(spatial_services(spatial))
    };
    for service in asked() {
        for group in service.android_permission_groups() {
            if host.permission_answer(group) == PermissionAnswer::NotAsked {
                wanted.extend(group.iter().copied());
            }
        }
    }
    if wanted.is_empty() {
        return;
    }
    log::info!("cranpose xr: asks for {wanted:?} before the session starts");
    host.request_permissions(&wanted);
    let start = Instant::now();
    while asked().any(|service| step_of_service(host, service) == PermissionStep::Wait) {
        if host.ended(SessionStatus::Idle) || start.elapsed() > PATIENCE {
            return;
        }
    }
}
