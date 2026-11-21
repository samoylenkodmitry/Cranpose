//! How long the scene of the volumes keeps its window and the eyes' swapchains with nothing to draw.

use std::time::{Duration, Instant};

// A pointer that crosses the gap between two panels can hide a controller's model and show it again
// within a second. A scene that comes back makes its swapchains, its window's renderer and that
// renderer's pipelines again, which holds one frame for about 36 ms on a Quest 3. The scene keeps
// them this long after its last content went. The swapchains of a Quest 3 hold about 143 MB.
const HOLD: Duration = Duration::from_secs(5);

// Where the scene stands between frames that draw.
#[derive(Default)]
pub(crate) enum SceneHold {
    // The scene holds no window or swapchain
    #[default]
    Free,
    // The last frame had content
    Drawing,
    // The content went at this time, and the scene keeps its window and swapchains
    Holding(Instant),
}

impl SceneHold {
    // Takes a frame at `now` that has `contents` things to draw. Returns true at the one frame at
    // which the hold ends: the scene then lets its window and its swapchains go.
    pub(crate) fn frame(&mut self, contents: usize, now: Instant) -> bool {
        match *self {
            _ if contents > 0 => {
                *self = Self::Drawing;
                false
            }
            Self::Drawing => {
                *self = Self::Holding(now);
                false
            }
            Self::Holding(since) if now.saturating_duration_since(since) >= HOLD => {
                *self = Self::Free;
                true
            }
            Self::Holding(_) | Self::Free => false,
        }
    }
}

#[cfg(test)]
#[path = "tests/scene_hold_tests.rs"]
mod tests;
