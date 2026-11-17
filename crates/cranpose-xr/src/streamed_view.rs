//! What a streamed scene says about the pictures of its frames.

use std::time::Duration;

use cranpose::CropRect;

use crate::{FieldOfView, FoveatedEncoding, Pose};

#[cfg_attr(
    not(any(test, all(headset, target_os = "android"))),
    expect(dead_code, reason = "the headset backend uses it")
)]
// An angle at or beyond this many radians from the view's direction has no picture plane.
const MAX_ANGLE: f32 = std::f32::consts::FRAC_PI_2 - 0.01;

/// One eye's view of a streamed frame: where the frame was rendered from, and which part of the
/// frame is the picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StreamedView {
    /// The eye's pose when the frame was rendered, in the app's room: the space of
    /// [`SpatialPanel`](crate::SpatialPanel)s, with its origin where the head was when the app
    /// started.
    pub pose: Pose,
    /// The field of view that the frame was rendered with.
    pub fov: FieldOfView,
    /// The part of the frame that holds this eye's picture, in pixels from the frame's top left
    /// corner. A foveated picture fills the rectangle as the host encoded it, with its padding.
    pub rect: CropRect,
    /// How the host compressed the picture's edges, or `None` for a picture that has the same
    /// density everywhere. The headset undoes the compression, so the compositor receives the
    /// eye's full view, whose size is [`FoveatedEncoding::full_size`], and the `fov` is the field
    /// of view of that full view.
    pub foveation: Option<FoveatedEncoding>,
}

/// The views of a streamed frame, one for each eye.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StreamedViews {
    /// The left eye's view.
    pub left: StreamedView,
    /// The right eye's view.
    pub right: StreamedView,
    /// The time at which the host wants the frame's pictures to reach the display, on the clock
    /// of [`XrFrame::predicted_display_time`](crate::XrFrame::predicted_display_time), or `None`
    /// when the host gives none. The headset shows the waiting frame whose target lies closest to
    /// the display time of the frame that it submits. Frames with no target follow the newest
    /// frame.
    pub target_display_time: Option<Duration>,
}

impl StreamedViews {
    #[cfg_attr(
        not(any(test, all(headset, target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The views with unit orientations, if the headset's compositor can show them: the poses are
    // finite, the fields of view are proper, and each rectangle lies inside a picture of
    // `picture` pixels. A quaternion of a few percent off unit length is a rounding of the app.
    pub(crate) fn checked(self, picture: (u32, u32)) -> Result<Self, &'static str> {
        Ok(Self {
            left: self.left.checked(picture)?,
            right: self.right.checked(picture)?,
            ..self
        })
    }
}

impl StreamedView {
    #[cfg_attr(
        not(any(test, all(headset, target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    fn checked(mut self, picture: (u32, u32)) -> Result<Self, &'static str> {
        if !self.pose.position.iter().all(|v| v.is_finite()) {
            return Err("a pose has a position that is not finite");
        }
        let squared: f32 = self.pose.orientation.iter().map(|v| v * v).sum();
        if !squared.is_finite() || !(0.81..=1.21).contains(&squared) {
            return Err("a pose has an orientation that is far from a unit quaternion");
        }
        let length = squared.sqrt();
        self.pose.orientation = self.pose.orientation.map(|v| v / length);
        let FieldOfView {
            angle_left,
            angle_right,
            angle_up,
            angle_down,
        } = self.fov;
        let angles = [angle_left, angle_right, angle_up, angle_down];
        if !angles.iter().all(|angle| angle.abs() < MAX_ANGLE) {
            return Err("a field of view has an angle that is not finite or reaches 90 degrees");
        }
        if angle_left >= angle_right || angle_down >= angle_up {
            return Err("a field of view has no area");
        }
        let CropRect {
            x,
            y,
            width,
            height,
        } = self.rect;
        let inside = x.checked_add(width).is_some_and(|right| right <= picture.0)
            && y.checked_add(height)
                .is_some_and(|bottom| bottom <= picture.1);
        if width == 0 || height == 0 || !inside {
            return Err("a rectangle is empty or lies outside the picture");
        }
        if let Some(foveation) = &self.foveation {
            foveation.checked()?;
        }
        Ok(self)
    }

    #[cfg_attr(
        not(any(test, all(headset, target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The size in pixels of the picture that the compositor receives: the full view of a foveated
    // picture, or the rectangle itself.
    pub(crate) fn output_size(&self) -> (u32, u32) {
        self.foveation
            .map_or((self.rect.width, self.rect.height), |foveation| {
                foveation.full_size
            })
    }
}

#[cfg(test)]
#[path = "tests/streamed_view_tests.rs"]
mod tests;
