//! Where a scrollbar's thumb sits and how long it is.
//!
//! Pure arithmetic over three lengths — how much content there is, how much of
//! it is on screen, how far it has travelled — shared by every indicator that
//! reports a scroll position: the rectangular [`Scrollbar`](crate::widgets::Scrollbar)
//! and the curved indicator a round watch draws.
//!
//! What differs between them is only how short and how long the thumb is
//! allowed to get, which is why [`ThumbBounds`] is a parameter rather than a
//! constant here.

/// Thumb length and position, both as fractions of the track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThumbGeometry {
    /// The thumb's share of the track, in `0..=1`.
    pub length: f32,
    /// The thumb's leading edge: `0.0` at the start of the track, and
    /// `1.0 - length` once the content is scrolled to the end.
    pub offset: f32,
}

impl ThumbGeometry {
    /// The thumb's trailing edge, as a fraction of the track.
    pub fn end(self) -> f32 {
        self.offset + self.length
    }

    /// How far through its travel the thumb is, in `0..=1`. `0.0` when the
    /// thumb fills the track and cannot travel at all.
    pub fn progress(self) -> f32 {
        let travel = 1.0 - self.length;
        if travel <= 0.0 {
            0.0
        } else {
            (self.offset / travel).clamp(0.0, 1.0)
        }
    }
}

/// How short and how long a thumb may get, as fractions of the track.
///
/// A very long list would otherwise produce a thumb too small to see or to
/// grab; a platform states its own floor rather than inheriting someone else's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThumbBounds {
    minimum: f32,
    maximum: f32,
}

impl ThumbBounds {
    /// A thumb free to be any length, which is what a caller that enforces a
    /// minimum in pixels rather than fractions wants.
    pub const FULL: Self = Self {
        minimum: 0.0,
        maximum: 1.0,
    };

    /// Bounds clamped into `0..=1` and ordered, so a caller cannot describe an
    /// empty range.
    pub fn new(minimum: f32, maximum: f32) -> Self {
        let minimum = if minimum.is_finite() {
            minimum.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let maximum = if maximum.is_finite() {
            maximum.clamp(0.0, 1.0)
        } else {
            1.0
        };
        Self {
            minimum: minimum.min(maximum),
            maximum: maximum.max(minimum),
        }
    }

    /// The bounds that keep a thumb at least `extent` long on a track of
    /// `track` — the pixel-shaped way to say it.
    pub fn at_least(extent: f32, track: f32) -> Self {
        if !(extent.is_finite() && track.is_finite()) || track <= 0.0 || extent <= 0.0 {
            return Self::FULL;
        }
        Self::new(extent / track, 1.0)
    }

    /// Shortest permitted thumb, as a fraction of the track.
    pub fn minimum(self) -> f32 {
        self.minimum
    }

    /// Longest permitted thumb, as a fraction of the track.
    pub fn maximum(self) -> f32 {
        self.maximum
    }
}

impl Default for ThumbBounds {
    fn default() -> Self {
        Self::FULL
    }
}

/// The thumb for a scroll position, or `None` when the content fits and there
/// is nothing to indicate.
///
/// `content` and `viewport` are lengths in one unit and `scrolled` is how far
/// the content has travelled in that same unit. Non-finite inputs and a
/// viewport that has not been measured yet both answer `None` rather than a
/// thumb drawn from a guess.
pub fn thumb_geometry(
    content: f32,
    viewport: f32,
    scrolled: f32,
    bounds: ThumbBounds,
) -> Option<ThumbGeometry> {
    if !(content.is_finite() && viewport.is_finite() && scrolled.is_finite()) {
        return None;
    }
    if viewport <= 0.0 || content <= viewport {
        return None;
    }
    let length = (viewport / content).clamp(bounds.minimum(), bounds.maximum());
    let travel = content - viewport;
    let progress = (scrolled / travel).clamp(0.0, 1.0);
    Some(ThumbGeometry {
        length,
        offset: progress * (1.0 - length),
    })
}

/// How far the content moves when the thumb is dragged `delta` along a track of
/// `track`, given the thumb it currently shows.
///
/// A thumb that fills its track cannot be dragged, and a scroll with no travel
/// cannot follow one, so both answer zero rather than dividing by zero.
pub fn content_delta_for_thumb_drag(
    delta: f32,
    track: f32,
    geometry: ThumbGeometry,
    max_offset: f32,
) -> f32 {
    if !(delta.is_finite() && track.is_finite() && max_offset.is_finite()) {
        return 0.0;
    }
    let travel = track * (1.0 - geometry.length);
    if travel <= 0.0 || max_offset <= 0.0 {
        return 0.0;
    }
    delta / travel * max_offset
}

#[cfg(test)]
#[path = "tests/scrollbar_tests.rs"]
mod tests;
