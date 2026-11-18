//! The sizes that a resize may give.

/// The sizes that the user may give a resizable composable, in dp. They are the `minimumSize`,
/// `maximumSize` and `maintainAspectRatio` parameters of Jetpack XR's `resizable`. Rust has no
/// named or default arguments, so they come in one value, and `SubspaceModifier::resizable_with`
/// takes it.
///
/// The user cannot make the content smaller than 20 dp in either direction, whatever the
/// minimum is. A maximum below the minimum is the minimum.
///
/// ```
/// use cranpose_xr::ResizeLimits;
///
/// let limits = ResizeLimits::new()
///     .minimum_size(320.0, 200.0)
///     .maximum_size(1600.0, 1000.0)
///     .maintain_aspect_ratio(true);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResizeLimits {
    minimum: [f32; 2],
    maximum: [f32; 2],
    keep_ratio: bool,
}

impl Default for ResizeLimits {
    fn default() -> Self {
        Self::new()
    }
}

impl ResizeLimits {
    /// No minimum, no maximum and a free aspect ratio, as Jetpack XR's defaults.
    pub const fn new() -> Self {
        Self {
            minimum: [0.0; 2],
            maximum: [f32::INFINITY; 2],
            keep_ratio: false,
        }
    }

    /// Sets the smallest width and height, in dp. Jetpack XR's `minimumSize`.
    pub const fn minimum_size(mut self, width: f32, height: f32) -> Self {
        self.minimum = [width, height];
        self
    }

    /// Sets the largest width and height, in dp. Jetpack XR's `maximumSize`.
    pub const fn maximum_size(mut self, width: f32, height: f32) -> Self {
        self.maximum = [width, height];
        self
    }

    /// Whether the user's resize keeps the content's proportions, which are those of its size at
    /// the grab. Jetpack XR's `maintainAspectRatio`, which defaults to `false`.
    pub const fn maintain_aspect_ratio(mut self, maintain: bool) -> Self {
        self.keep_ratio = maintain;
        self
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The smallest and the largest width and height, in that order.
    pub(crate) const fn bounds(&self) -> ([f32; 2], [f32; 2]) {
        (self.minimum, self.maximum)
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) const fn keeps_ratio(&self) -> bool {
        self.keep_ratio
    }
}
