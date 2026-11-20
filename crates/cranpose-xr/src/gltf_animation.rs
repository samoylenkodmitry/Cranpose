//! Playing an animation of a glTF model.

use std::{sync::Arc, time::Duration};

use cranpose_core::{MutableState, mutableStateOf};

/// Whether a [`GltfAnimation`] plays. Jetpack XR's `GltfAnimation.AnimationState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GltfAnimationState {
    /// The animation runs.
    Playing,
    /// The animation stands at the time where it was paused.
    Paused,
    /// The animation has not started, was stopped, or ran to its end with no loop.
    Stopped,
}

// Where an animation is in its clip and how it runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Clock {
    // Seconds into the clip
    pub(crate) time: f32,
    // Whether the clip's pose is on the model. A clip that ran out holds its last pose after it
    // stops, and a clip that was stopped leaves the model at rest.
    pub(crate) posing: bool,
    pub(crate) looping: bool,
    pub(crate) speed: f32,
}

impl Clock {
    // Moves `seconds` of the clock forward, scaled by the speed, in a clip of `duration` seconds.
    // Returns whether the clip ran out. A looping clip never does.
    pub(crate) fn step(&mut self, seconds: f32, duration: f32) -> bool {
        let time = self.time + seconds * self.speed;
        if self.looping {
            self.time = if duration > 0.0 {
                time.rem_euclid(duration)
            } else {
                0.0
            };
            return false;
        }
        if self.speed == 0.0 {
            return false;
        }
        let ended = time >= duration || time <= 0.0;
        self.time = time.clamp(0.0, duration.max(0.0));
        ended
    }
}

/// One animation of a glTF model: the keys of its nodes' translation, rotation and scale over
/// time. Jetpack XR's `GltfAnimation`, which a `GltfModelEntity` lists.
///
/// An animation plays once, forward, at normal speed, unless [`set_looping`](Self::set_looping) and
/// [`set_speed`](Self::set_speed) say otherwise. Several animations play at once. Where two of them
/// key the same property of the same node, the one with the higher index wins.
///
/// A handle is a cheap copy: every copy controls the same animation. Skins and morph targets are
/// not animated.
#[derive(Clone)]
pub struct GltfAnimation {
    index: usize,
    name: Option<Arc<str>>,
    duration: f32,
    state: MutableState<GltfAnimationState>,
    clock: MutableState<Clock>,
    // Counts the starts and stops of the model's animations. A start and a stop between two frames
    // leave the state as it was and change the pose, so the model reads this count too.
    requests: MutableState<u32>,
}

impl GltfAnimation {
    // An animation that has not started. `requests` is the count of the model's animations.
    pub(crate) fn new(
        index: usize,
        name: Option<&str>,
        duration: f32,
        requests: MutableState<u32>,
    ) -> Self {
        Self {
            requests,
            index,
            name: name.map(Arc::from),
            duration,
            state: mutableStateOf(GltfAnimationState::Stopped),
            clock: mutableStateOf(Clock {
                time: 0.0,
                posing: false,
                looping: false,
                speed: 1.0,
            }),
        }
    }

    /// The index of this animation in the glTF file's `animations` array.
    pub fn index(&self) -> usize {
        self.index
    }

    /// The name that the file gives this animation, if it names it.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// How long one run takes at normal speed: the time of the latest key.
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f32(self.duration.max(0.0))
    }

    /// Whether the animation plays, stands or has stopped. A composable that reads it composes
    /// again when it changes.
    pub fn animation_state(&self) -> GltfAnimationState {
        self.state.get()
    }

    /// Whether the animation starts over when it reaches its end. Jetpack XR's `loop`, which is a
    /// keyword in Rust.
    pub fn looping(&self) -> bool {
        self.clock.get_non_reactive().looping
    }

    /// Sets whether the animation starts over when it reaches its end, while it plays or is
    /// paused. An animation that does not loop stops at its last pose. Jetpack XR's `loop`.
    pub fn set_looping(&self, looping: bool) {
        self.clock.update(|clock| clock.looping = looping);
    }

    /// The playback speed as a multiple of the normal speed.
    pub fn speed(&self) -> f32 {
        self.clock.get_non_reactive().speed
    }

    /// Sets the playback speed, while the animation plays or is paused: 2.0 plays twice as fast, 0.5
    /// half as fast, 0.0 holds the current pose with the animation still playing, and a negative
    /// speed plays backward. A backward animation that does not loop and starts at time 0
    /// reaches its end at once. Jetpack XR's `speed`.
    pub fn set_speed(&self, speed: f32) {
        self.clock.update(|clock| clock.speed = speed);
    }

    /// Plays the animation from its start. It restarts if it plays or is paused.
    pub fn start(&self) {
        self.clock.update(|clock| {
            clock.time = 0.0;
            clock.posing = true;
        });
        self.state.set(GltfAnimationState::Playing);
        self.requests.update(|count| *count = count.wrapping_add(1));
    }

    /// Stops a playing or paused animation and puts the model back at rest. Nothing happens to an
    /// animation that is already stopped, so one that ran to its end keeps its last pose.
    pub fn stop(&self) {
        if self.state.get_non_reactive() == GltfAnimationState::Stopped {
            return;
        }
        self.clock.update(|clock| {
            clock.time = 0.0;
            clock.posing = false;
        });
        self.state.set(GltfAnimationState::Stopped);
        self.requests.update(|count| *count = count.wrapping_add(1));
    }

    /// Holds a playing animation at its current pose. [`resume`](Self::resume) goes on from there.
    pub fn pause(&self) {
        if self.state.get_non_reactive() == GltfAnimationState::Playing {
            self.state.set(GltfAnimationState::Paused);
        }
    }

    /// Goes on with a paused animation from its pose.
    pub fn resume(&self) {
        if self.state.get_non_reactive() == GltfAnimationState::Paused {
            self.state.set(GltfAnimationState::Playing);
        }
    }

    // Runs the clock `seconds` forward if the animation plays, and stops the animation at its end.
    pub(crate) fn advance(&self, seconds: f32) {
        if self.state.get_non_reactive() != GltfAnimationState::Playing {
            return;
        }
        let ended = self
            .clock
            .update(|clock| clock.step(seconds, self.duration));
        if ended {
            self.state.set(GltfAnimationState::Stopped);
        }
    }

    // The time that the animation poses the model at, or `None` while the model stands at rest.
    pub(crate) fn pose_time(&self) -> Option<f32> {
        let clock = self.clock.get_non_reactive();
        clock.posing.then_some(clock.time)
    }
}

#[cfg(test)]
#[path = "tests/gltf_animation_tests.rs"]
mod tests;
