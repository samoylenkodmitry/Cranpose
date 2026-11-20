/// How the platform should vote the display's frame rate on behalf of the app.
///
/// Compose apps get 120 Hz gameplay on a 120 Hz panel not by presenting faster
/// but because HWUI votes a rate on the window while gestures run, content
/// moves or the window redraws continuously, and clears it when they stop. A window that never votes is pinned by
/// SurfaceFlinger's cadence inference instead — which also throttles the app's
/// choreographer, so the inference reinforces itself. `Auto` reproduces the
/// HWUI behaviour; the platform backends read it every frame and vote through
/// the native window when the desired rate changes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FrameRatePreference {
    /// Ask for the panel's fastest rate while [`FrameRateBoost`] holds, a
    /// quiet rate while frames arrive only intermittently, no preference
    /// when the scene is still. This is the default, matching what Compose/HWUI do for every
    /// app without the app's involvement.
    #[default]
    Auto,
    /// Never vote; the OS infers a rate from presentation cadence.
    NoPreference,
    /// Always vote exactly this rate in Hz. Values `<= 0` behave like
    /// [`FrameRatePreference::NoPreference`].
    Exact(f32),
}

impl FrameRatePreference {
    /// The baseline `Auto` votes while frames arrive without a boost — the
    /// same rate HWUI's NORMAL frame-rate category resolves to on phone
    /// panels. The quiet vote cannot simply be "no vote": SurfaceFlinger
    /// infers a non-voting window's rate from whatever cadence it last
    /// observed and pins it, so an app that ever ran the panel's fast rate
    /// would stay there forever (measured on a Pixel 9 Pro, both directions).
    pub const AUTO_QUIET_RATE_HZ: f32 = 60.0;

    /// The rate the platform should vote right now, in Hz, where `0.0` means
    /// "clear the vote". `producing_frames` is whether the frame loop has a
    /// frame scheduled, `boosted` what [`FrameRateBoost::boosted`] says, and
    /// `panel_max_hz` the display's fastest supported rate when the platform
    /// knows it.
    ///
    /// While boosted, `Auto` holds the boost even through moments with no
    /// frame scheduled: a gesture sequence crosses still screens (a tap lands,
    /// the old scene stops animating, the new one hasn't started), and letting
    /// each of those instantly clear the vote flapped the display between the
    /// boost rate and no-vote several times per second on device. This mirrors
    /// SurfaceFlinger's own touch boost, which also outlives the touch by
    /// seconds regardless of what the app presents in between.
    pub fn desired_rate_hz(
        self,
        producing_frames: bool,
        boosted: bool,
        panel_max_hz: Option<f32>,
    ) -> f32 {
        match self {
            Self::Auto => {
                if boosted {
                    panel_max_hz
                        .filter(|rate| *rate > 0.0)
                        .unwrap_or(Self::AUTO_QUIET_RATE_HZ)
                } else if producing_frames {
                    Self::AUTO_QUIET_RATE_HZ
                } else {
                    0.0
                }
            }
            Self::NoPreference => 0.0,
            Self::Exact(rate) if rate > 0.0 => rate,
            Self::Exact(_) => 0.0,
        }
    }
}
