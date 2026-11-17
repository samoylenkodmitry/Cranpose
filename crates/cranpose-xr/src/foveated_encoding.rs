//! How a host compressed the edges of an eye's picture.

/// ALVR's axis-aligned foveated encoding of one eye's picture. The host keeps the full pixel
/// density in a center region of the eye and compresses the edges by `edge_ratio`, so the encoder
/// codes a third of the pixels. The headset undoes the compression: a streamed scene shows the
/// full view that the host rendered, and the compositor receives a picture of
/// [`full_size`](Self::full_size).
///
/// Every field holds the value that the host's encoder used, after it aligned the region to its
/// compression blocks. A host that follows ALVR v20.14.1 takes `center_size`, `center_shift`,
/// `edge_ratio` and `eye_size_ratio` from its encoding setup. Each pair holds the horizontal
/// value, then the vertical one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoveatedEncoding {
    /// The width and the height in pixels of the eye's full view, before the host compressed it.
    /// The streamed scene draws the unfoveated picture into a swapchain of this size.
    pub full_size: (u32, u32),
    /// The part of the full view that keeps full pixel density, from 0 to 1 on each axis.
    pub center_size: [f32; 2],
    /// How far the center lies toward the outer edge of the view, as a fraction of the room that
    /// the center leaves, from -1 to 1 on each axis. The default of ALVR is 0.4 horizontally and
    /// 0.1 vertically.
    pub center_shift: [f32; 2],
    /// How much the host compressed the edges, more than 1 on each axis: an edge pixel of the
    /// encoded picture covers this many pixels of the full view.
    pub edge_ratio: [f32; 2],
    /// The share of the encoded picture's rectangle that the compressed eye fills, from 0 to 1
    /// on each axis. The host pads the encoded eye to a multiple of 32 pixels, and the ratio is
    /// the unpadded size divided by the padded one. The padding lies at the picture's far edge.
    pub eye_size_ratio: [f32; 2],
    /// Whether the host stored the picture mirrored from left to right. ALVR mirrors the right
    /// eye, so that the compressed outer edge of both eyes lies on the same side of the
    /// rectangle. The rectangle of a mirrored picture starts at the outer edge on the right.
    pub mirrored: bool,
}

impl FoveatedEncoding {
    #[cfg_attr(
        not(any(test, all(headset, target_os = "android"))),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Whether the unwarp has a result for this description. The pieces of each axis divide by
    // the edge ratio less 1, by the distance of the center from both edges and by the center's
    // size.
    pub(crate) fn checked(&self) -> Result<(), &'static str> {
        let open = |values: [f32; 2], low: f32, high: f32| {
            values
                .iter()
                .all(|v| v.is_finite() && *v > low && *v < high)
        };
        if self.full_size.0 == 0 || self.full_size.1 == 0 {
            return Err("a foveated picture has an empty full size");
        }
        if !open(self.center_size, 0.0, 1.0) {
            return Err("a foveated picture has a center size outside 0 to 1");
        }
        if !open(self.center_shift, -1.0, 1.0) {
            return Err("a foveated picture has a center shift outside -1 to 1");
        }
        if !open(self.edge_ratio, 1.0, f32::INFINITY) {
            return Err("a foveated picture has an edge ratio that does not exceed 1");
        }
        let ratio_in_range = self.eye_size_ratio.iter().all(|v| *v > 0.0 && *v <= 1.0);
        if !ratio_in_range {
            return Err("a foveated picture has an eye size ratio outside 0 to 1");
        }
        Ok(())
    }
}
