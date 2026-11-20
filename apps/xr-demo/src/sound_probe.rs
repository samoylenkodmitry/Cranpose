// A build with `XR_DEMO_SOUND=left`, `right` or `swing` plays the hum from a place that stays level
// with the head, 50 cm from it to its left or right. `swing` switches sides every three seconds. The
// head does not move on a desk. These builds show on a device that the sound follows the pose of the
// head. The audio tap of the build logs the level of each ear and where the source stands relative to
// the head. The left level is the higher one while the source is at the left.

use cranpose::{SoundId, prelude::*};
use cranpose_xr::{Pose, SpatialFrameEffect, SpatialPointSource, SubspaceModifier};

use crate::hum;

// What the build asks for: `left`, `right` or `swing`.
const SOUND_BUILD: Option<&str> = option_env!("XR_DEMO_SOUND");

// How far the source stands from the head, in meters.
const DISTANCE: f32 = 0.5;
// How long `swing` keeps a side, in seconds.
const SIDE_SECONDS: u64 = 3;
// The source moves with the head when the head has moved this far, in meters, so that the noise of
// the head's tracking does not compose the source in each frame.
const SETTLE: f32 = 0.015;
// A subspace's origin stands this far ahead of where the head started, along z. Subspace documents
// it.
const SUBSPACE_AHEAD: f32 = -1.0;

// Whether this build plays its hum from the head's side.
pub(crate) fn probe_build() -> bool {
    SOUND_BUILD.is_some()
}

// The side of the head that the source stands on at `seconds` of the headset's clock: -1 for the
// left and 1 for the right.
fn side(setting: &str, seconds: u64) -> f32 {
    match setting {
        "right" => 1.0,
        "swing" if (seconds / SIDE_SECONDS).is_multiple_of(2) => -1.0,
        "swing" => 1.0,
        _ => -1.0,
    }
}

// The hum, played from the side that the build names. The source's place is the head's place plus
// the head's own left or right, in the frame of the subspace.
#[composable]
pub(crate) fn SoundProbe(sound: SoundId) {
    let setting = SOUND_BUILD.unwrap_or("left");
    let place = rememberMutableStateOf(|| [-DISTANCE, 0.0, -SUBSPACE_AHEAD]);
    SpatialFrameEffect(move |frame| {
        let Some(head) = frame.device_pose else {
            return;
        };
        let side = side(setting, frame.predicted_display_time.as_secs());
        let ear = head.rotate([side * DISTANCE, 0.0, 0.0]);
        let wanted = [
            head.position[0] + ear[0],
            head.position[1] + ear[1],
            head.position[2] + ear[2] - SUBSPACE_AHEAD,
        ];
        let here = place.get_non_reactive();
        let moved = (0..3).any(|axis| (wanted[axis] - here[axis]).abs() > SETTLE);
        if moved {
            place.set(wanted);
        }
    });
    SpatialPointSource(
        SubspaceModifier::new().with_pose(Pose::at(place.get())),
        hum::source(sound),
    );
}
