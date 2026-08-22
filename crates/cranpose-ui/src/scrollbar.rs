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
mod tests {
    use super::*;

    #[test]
    fn content_that_fits_shows_no_thumb() {
        assert_eq!(thumb_geometry(100.0, 100.0, 0.0, ThumbBounds::FULL), None);
        assert_eq!(thumb_geometry(80.0, 100.0, 0.0, ThumbBounds::FULL), None);
        assert_eq!(thumb_geometry(500.0, 0.0, 0.0, ThumbBounds::FULL), None);
    }

    #[test]
    fn a_measurement_that_is_not_a_number_shows_no_thumb() {
        assert_eq!(
            thumb_geometry(f32::NAN, 100.0, 0.0, ThumbBounds::FULL),
            None
        );
        assert_eq!(
            thumb_geometry(400.0, f32::INFINITY, 0.0, ThumbBounds::FULL),
            None
        );
        assert_eq!(
            thumb_geometry(400.0, 100.0, f32::NAN, ThumbBounds::FULL),
            None
        );
    }

    #[test]
    fn the_thumb_is_the_share_of_content_on_screen_and_travels_with_it() {
        let top = thumb_geometry(400.0, 100.0, 0.0, ThumbBounds::FULL).expect("scrollable");
        assert_eq!(top.length, 0.25);
        assert_eq!(top.offset, 0.0);
        assert_eq!(top.progress(), 0.0);

        let bottom = thumb_geometry(400.0, 100.0, 300.0, ThumbBounds::FULL).expect("scrollable");
        assert_eq!(bottom.offset, 0.75);
        assert_eq!(bottom.end(), 1.0);
        assert_eq!(bottom.progress(), 1.0);

        let middle = thumb_geometry(400.0, 100.0, 150.0, ThumbBounds::FULL).expect("scrollable");
        assert_eq!(middle.offset, 0.375);
        assert_eq!(middle.progress(), 0.5);
    }

    #[test]
    fn scrolling_past_either_end_stays_on_the_track() {
        let before = thumb_geometry(400.0, 100.0, -50.0, ThumbBounds::FULL).expect("scrollable");
        assert_eq!(before.offset, 0.0);
        let after = thumb_geometry(400.0, 100.0, 900.0, ThumbBounds::FULL).expect("scrollable");
        assert_eq!(after.end(), 1.0);
    }

    #[test]
    fn a_very_long_list_still_shows_a_grabbable_thumb() {
        let bounds = ThumbBounds::at_least(24.0, 240.0);
        let geometry = thumb_geometry(100_000.0, 240.0, 0.0, bounds).expect("scrollable");
        assert_eq!(geometry.length, 0.1);

        // The floor only raises a thumb that is too short; a thumb already
        // longer than it keeps its own length.
        let roomy = thumb_geometry(480.0, 240.0, 0.0, bounds).expect("scrollable");
        assert_eq!(roomy.length, 0.5);
    }

    #[test]
    fn bounds_cannot_describe_an_empty_or_inverted_range() {
        let inverted = ThumbBounds::new(0.8, 0.2);
        assert_eq!(inverted.minimum(), 0.2);
        assert_eq!(inverted.maximum(), 0.8);

        let unmeasurable = ThumbBounds::new(f32::NAN, f32::NAN);
        assert_eq!(unmeasurable, ThumbBounds::FULL);

        assert_eq!(ThumbBounds::at_least(24.0, 0.0), ThumbBounds::FULL);
        assert_eq!(ThumbBounds::at_least(f32::NAN, 240.0), ThumbBounds::FULL);
    }

    #[test]
    fn dragging_the_thumb_across_its_travel_scrolls_the_whole_content() {
        let geometry = thumb_geometry(400.0, 100.0, 0.0, ThumbBounds::FULL).expect("scrollable");
        // Track 200, thumb 25% of it: 150 of thumb travel maps onto 300 of scroll.
        assert_eq!(
            content_delta_for_thumb_drag(150.0, 200.0, geometry, 300.0),
            300.0
        );
        assert_eq!(
            content_delta_for_thumb_drag(-75.0, 200.0, geometry, 300.0),
            -150.0
        );
    }

    #[test]
    fn a_thumb_with_nowhere_to_go_does_not_scroll() {
        let full = ThumbGeometry {
            length: 1.0,
            offset: 0.0,
        };
        assert_eq!(content_delta_for_thumb_drag(40.0, 200.0, full, 300.0), 0.0);

        let geometry = thumb_geometry(400.0, 100.0, 0.0, ThumbBounds::FULL).expect("scrollable");
        assert_eq!(
            content_delta_for_thumb_drag(40.0, 200.0, geometry, 0.0),
            0.0
        );
        assert_eq!(
            content_delta_for_thumb_drag(f32::NAN, 200.0, geometry, 300.0),
            0.0
        );
    }
}
