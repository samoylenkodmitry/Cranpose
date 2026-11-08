/// How the shell paces its frames against the display.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FramePacingMode {
    /// Pace frames to the display refresh interval. The production default:
    /// animations advance once per vsync instead of re-rendering uncapped.
    #[default]
    Vsync,
    /// Cap frames at 60 per second with a timer, whatever the display refresh rate is.
    Hard60,
    /// Cap frames at 120 per second with a timer, whatever the display refresh rate is.
    Hard120,
    /// Render as fast as possible. For perf harnesses and robot drivers that
    /// measure work throughput; saturates the GPU if used in a real app.
    NoVsync,
}

impl FramePacingMode {
    /// Every pacing mode, in the order the dev overlay lists them.
    pub const ALL: [Self; 4] = [Self::Vsync, Self::Hard60, Self::Hard120, Self::NoVsync];

    /// Returns the short name the dev overlay shows for this mode.
    pub fn label(self) -> &'static str {
        match self {
            Self::Vsync => "VSync",
            Self::Hard60 => "60fps",
            Self::Hard120 => "120fps",
            Self::NoVsync => "NoVSync",
        }
    }

    /// Returns the frames per second this mode caps at with a timer, or `None` when the mode sets
    /// no cap.
    pub fn target_fps(self) -> Option<u32> {
        match self {
            Self::Hard60 => Some(60),
            Self::Hard120 => Some(120),
            Self::Vsync | Self::NoVsync => None,
        }
    }
}

/// Development options for debugging and performance monitoring.
///
/// These are rendered directly by the renderer (not via composition)
/// to avoid affecting performance measurements.
#[derive(Clone, Debug, Default)]
pub struct DevOptions {
    /// Show FPS counter overlay
    pub fps_counter: bool,
    /// Show recomposition count
    pub recomposition_counter: bool,
    /// Show layout timing breakdown
    pub layout_timing: bool,
    /// Show buttons in the dev overlay that switch the frame pacing mode.
    pub frame_pacing_controls: bool,
    /// How the shell paces its frames.
    pub frame_pacing_mode: FramePacingMode,
}
