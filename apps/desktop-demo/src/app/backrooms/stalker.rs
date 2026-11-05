// The entity, haunting the eye after it says to turn around: it stands
// behind the eye's back in the room the eye stands in. While the eye sees
// it, it stands still; while the eye looks away, it glides back behind the
// eye, slower than a quick turn. It goes when the eye comes close.

use cranpose_core::{derivedStateOf, rememberMutableStateOf, LaunchedEffectAsync};
use cranpose_ui::{
    composable,
    space::{EyePose, Vec3},
    Box, BoxSpec,
};

use super::{figure::figure, finish::Sight};

// How far behind the eye the entity stands, and how fast it glides there.
const BEHIND: f32 = 170.0;
const GLIDE: f32 = 80.0;
// How near the eye it lets the eye come before it goes.
const CLOSEST: f32 = 90.0;
// How near a wall it stands.
const EDGE: f32 = 50.0;
// How far off the way the eye looks, in radians, the entity still shows: a
// little past the side of a wide window.
const SEEN: f32 = 1.1;

// The entity in the eye's room while it haunts the eye. Only this reads
// how long it haunts, so the room around it does not compose again.
#[composable(no_skip)]
pub(super) fn Haunting(sight: Sight, size: (f32, f32)) {
    if sight.level.journey.haunted.get() > 0 {
        Stalker(sight, size);
    }
}

#[composable(no_skip)]
fn Stalker(sight: Sight, size: (f32, f32)) {
    // Where the entity stands; `None` until the room has space behind the
    // eye, and after the eye comes close.
    let foot = rememberMutableStateOf(|| None::<Vec3>);
    // Each frame, an entity the eye does not see glides toward the spot
    // behind the eye. The effect reads the eye outside composition, and
    // only the draw closure reads the foot, so the glide composes nothing.
    let eye = sight.eye.clone();
    LaunchedEffectAsync((), move |scope| {
        Box::pin(async move {
            let clock = scope.runtime().frame_clock();
            let mut last = clock.next_perpetual_frame().await;
            while scope.is_active() {
                let frame = clock.next_perpetual_frame().await;
                let seconds = frame.saturating_sub(last) as f32 * 1.0e-9;
                last = frame;
                let pose = eye.pose();
                let now = foot.get_non_reactive();
                let next = match now {
                    Some(at) if distance(&pose, at) < CLOSEST => None,
                    Some(at) if seen(&pose, at) => Some(at),
                    Some(at) => {
                        Some(behind(&pose, size).map_or(at, |to| glide(at, to, GLIDE * seconds)))
                    }
                    None => behind(&pose, size).filter(|at| !seen(&pose, *at)),
                };
                if next != now {
                    foot.set(next);
                }
            }
        })
    });
    let stands = derivedStateOf(move || foot.get().is_some());
    if stands.get() {
        Box(
            figure(sight, move || foot.get().unwrap_or_default()),
            BoxSpec::default(),
            || {},
        );
    }
}

// The floor point behind the eye's back; `None` when it lies outside a room
// of `size`.
fn behind(pose: &EyePose, (width, depth): (f32, f32)) -> Option<Vec3> {
    let back = Vec3::new(-pose.forward.x, 0.0, -pose.forward.z).normalized()?;
    let at = pose.position + back * BEHIND;
    let inside = (EDGE..=width - EDGE).contains(&at.x) && (EDGE..=depth - EDGE).contains(&at.z);
    inside.then_some(Vec3::new(at.x, 0.0, at.z))
}

// The point `reach` from `from` toward `to`, or `to` when nearer.
fn glide(from: Vec3, to: Vec3, reach: f32) -> Vec3 {
    let way = to - from;
    let length = way.length();
    if length <= reach {
        to
    } else {
        from + way * (reach / length)
    }
}

// How far the floor point `foot` lies from the eye, along the floor.
fn distance(pose: &EyePose, foot: Vec3) -> f32 {
    Vec3::new(foot.x - pose.position.x, 0.0, foot.z - pose.position.z).length()
}

// Whether the eye sees the floor point `foot`.
fn seen(pose: &EyePose, foot: Vec3) -> bool {
    let toward = Vec3::new(foot.x - pose.position.x, 0.0, foot.z - pose.position.z);
    let ahead = Vec3::new(pose.forward.x, 0.0, pose.forward.z);
    match (toward.normalized(), ahead.normalized()) {
        (Some(toward), Some(ahead)) => toward.dot(ahead) > SEEN.cos(),
        _ => true,
    }
}
