//! A callback that runs once in each frame of a headset.

use cranpose::prelude::*;

use crate::{XrFrame, frame_effects::local_frame_effects};

/// Runs `on_frame` once in each frame of the headset, with the state of the head, the eyes and
/// the controllers. It runs on the thread of the frame loop, before the panels draw, so a state
/// that it sets shows in the same frame.
///
/// The frame rate is the display's refresh rate, 120 Hz on a Quest 3 at its highest setting. The
/// callback receives a borrowed [`XrFrame`] that the backend fills in place, and the frame loop
/// allocates nothing for it. A callback that changes a `MutableState` in each frame draws the
/// panels that read it in each frame. A callback that sets a state only when its value changes
/// leaves a still scene at no cost.
///
/// The latest `on_frame` of the composition runs. Several effects run in the order in which they
/// entered the composition. Outside a headset the callback never runs.
///
/// ```ignore
/// let height = rememberMutableStateOf(|| 0_i32);
/// SpatialFrameEffect(move |frame| {
///     let centimeters = frame.device_pose.map_or(0, |pose| (pose.position[1] * 100.0) as i32);
///     if height.get_non_reactive() != centimeters {
///         height.set(centimeters);
///     }
/// });
/// ```
#[composable]
pub fn SpatialFrameEffect(on_frame: impl FnMut(&XrFrame) + 'static) {
    let Some(effects) = local_frame_effects().current() else {
        return;
    };
    let slot = remember({
        let effects = effects.clone();
        move || effects.add()
    })
    .with(Clone::clone);
    slot.set(Box::new(on_frame));
    DisposableEffect((), move |_| {
        DisposableEffectResult::new(move || effects.remove(&slot))
    });
}

#[cfg(test)]
#[path = "tests/spatial_frame_effect_tests.rs"]
mod tests;
