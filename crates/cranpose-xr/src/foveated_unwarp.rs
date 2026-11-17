//! The headset's half of ALVR's foveated encoding: where a pixel of the full view sits in the
//! encoded picture.
//!
//! The host's encoder compresses each axis of an eye with a function of three pieces: the outer
//! edge, the center and the inner edge. The center has the full density, and the edges have
//! `edge_ratio` times fewer pixels. The unwarp runs the other way: for each pixel of the full view
//! it says where to read the encoded picture. The inverse of the two edge pieces is a quadratic
//! formula, and the center piece is linear. The stream shader evaluates the same three pieces from
//! the same constants as [`FoveatedUnwarp::sample`], so a test of that function covers the shader.

use crate::FoveatedEncoding;

// One axis of the unwarp in the vec4s that the shader reads:
// - the bounds of the three pieces in the full view, the center piece's offset, and its slope;
// - the outer edge's quadratic `a` and `b`, and the share of the rectangle that the eye fills;
// - the inner edge's quadratic `a`, `b` and `c`.
type Axis = [[f32; 4]; 3];

// The unwarp of one eye.
pub(crate) struct FoveatedUnwarp {
    axes: [Axis; 2],
    // 1 for a picture stored mirrored from left to right, and 0 otherwise: the shader mixes with it.
    mirror: f32,
}

impl FoveatedUnwarp {
    pub(crate) fn new(encoding: &FoveatedEncoding) -> Self {
        let axis = |i: usize| {
            axis_of(
                encoding.center_size[i],
                encoding.center_shift[i],
                encoding.edge_ratio[i],
                encoding.eye_size_ratio[i],
            )
        };
        Self {
            axes: [axis(0), axis(1)],
            mirror: if encoding.mirrored { 1.0 } else { 0.0 },
        }
    }

    #[cfg_attr(
        not(all(headset, target_os = "android")),
        expect(dead_code, reason = "the headset backend uploads it")
    )]
    // The six vec4s of the shader's `unwarp` uniform: the horizontal axis, then the vertical one.
    pub(crate) fn axes(&self) -> [f32; 24] {
        let mut flat = [0.0; 24];
        for (slot, value) in flat.iter_mut().zip(self.axes.iter().flatten().flatten()) {
            *slot = *value;
        }
        flat
    }

    #[cfg_attr(
        not(all(headset, target_os = "android")),
        expect(dead_code, reason = "the headset backend uploads it")
    )]
    // The shader's `mirror` uniform.
    pub(crate) fn mirror(&self) -> f32 {
        self.mirror
    }

    // Where to read the encoded picture for the point `view` of the full view, as fractions of
    // the encoded picture's rectangle from its top left corner. `view` runs from the top left
    // corner of the full view. The shader computes the same values for each pixel.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the shader evaluates it, and the test calls it")
    )]
    pub(crate) fn sample(&self, view: [f32; 2]) -> [f32; 2] {
        let mix = |a: f32, b: f32| a + (b - a) * self.mirror;
        let x = mix(view[0], 1.0 - view[0]);
        let encoded = [unwarp(x, &self.axes[0]), unwarp(view[1], &self.axes[1])];
        [mix(encoded[0], 1.0 - encoded[0]), encoded[1]]
    }
}

// The constants of one axis, from the encoder's aligned values. `size` is the center's share of the
// view, `shift` its place, `ratio` the edge compression, and `fill` the share of the rectangle that
// the eye fills.
fn axis_of(size: f32, shift: f32, ratio: f32, fill: f32) -> Axis {
    let outer = (1.0 - size) * 0.5;
    let offset = (ratio - 1.0) * outer * (shift + 1.0) / ratio;
    let slope = (ratio - 1.0) * size + 1.0;
    let lo_bound = outer * (shift + 1.0);
    let hi_bound = outer * (shift - 1.0) + 1.0;
    let lo_center = lo_bound / slope;
    let hi_center = outer * (shift - 1.0) / slope + 1.0;
    let hi_room = 1.0 - hi_center;
    let lo_a = slope * (1.0 - ratio) / (ratio * lo_center);
    let lo_b = (offset + slope * lo_center) / lo_center;
    let hi_a = slope * (ratio - 1.0) / (ratio * hi_room);
    let hi_b = (slope - ratio * offset - 2.0 * ratio * slope + slope * ratio * hi_room + ratio)
        / (ratio * hi_room);
    let hi_c = (slope * ratio - slope) * (offset - hi_center + slope * hi_center)
        / (ratio * hi_room * hi_room);
    [
        [lo_bound, hi_bound, offset, ratio / slope],
        [lo_a, lo_b, fill, 0.0],
        [hi_a, hi_b, hi_c, 0.0],
    ]
}

// The encoded position of `view` on one axis. The max keeps a rounding error at an edge from
// taking the root of a tiny negative number.
fn unwarp(view: f32, axis: &Axis) -> f32 {
    let [lo_bound, hi_bound, offset, scale] = axis[0];
    let [lo_a, lo_b, fill, _] = axis[1];
    let [hi_a, hi_b, hi_c, _] = axis[2];
    let position = if view < lo_bound {
        ((lo_b * lo_b + 4.0 * lo_a * view).max(0.0).sqrt() - lo_b) / (2.0 * lo_a)
    } else if view > hi_bound {
        ((hi_b * hi_b - 4.0 * (hi_c - hi_a * view)).max(0.0).sqrt() - hi_b) / (2.0 * hi_a)
    } else {
        (view - offset) * scale
    };
    position * fill
}

#[cfg(test)]
#[path = "tests/foveated_unwarp_tests.rs"]
mod tests;
