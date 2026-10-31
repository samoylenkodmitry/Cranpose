use bytemuck::{Pod, Zeroable};

use crate::{
    geometry::{SegmentTransform, snap_delta_for_anchor, snapped_anchor_device_origin},
    scene::{
        PLACEMENT_CANONICALIZE, PLACEMENT_CLIPPED, PLACEMENT_FILTERED, PLACEMENT_PAINTED,
        PLACEMENT_TURNED, Placement,
    },
};

/// A placement as the vertex stage reads it: the offset with the snap
/// delta folded in, the device clip and its corner radius, the dither
/// origin and the paint.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct PlacementData {
    pub(super) offset: [f32; 2],
    pub(super) root_scale: f32,
    pub(super) flags: u32,
    pub(super) clip: [f32; 4],
    pub(super) dither_origin: [f32; 2],
    pub(super) alpha: f32,
    pub(super) clip_radius: f32,
    pub(super) color_matrix: [[f32; 4]; 4],
    pub(super) color_offset: [f32; 4],
    pub(super) transform: [f32; 4],
    pub(super) translation: [f32; 2],
    pub(super) transform_reserved: [f32; 2],
}

impl PlacementData {
    /// How the fallback triangles of a record under this placement land
    /// and paint (see [`crate::mesh_fallback`]).
    pub(crate) fn mesh(&self) -> crate::mesh_fallback::MeshPlacement {
        let flagged = |flag: u32| self.flags & flag != 0;
        crate::mesh_fallback::MeshPlacement {
            offset: self.offset,
            scale: self.root_scale,
            canonicalize: flagged(PLACEMENT_CANONICALIZE),
            clip: flagged(PLACEMENT_CLIPPED).then_some(self.clip),
            turn: flagged(PLACEMENT_TURNED).then_some((self.transform, self.translation)),
            paint: flagged(PLACEMENT_PAINTED).then(|| crate::mesh_fallback::MeshPaint {
                alpha: self.alpha,
                filter: flagged(PLACEMENT_FILTERED)
                    .then_some((self.color_matrix, self.color_offset)),
            }),
        }
    }

    /// The placement data of `placement` at `root_scale`, drawn in place
    /// under `turn`: the identity except for a layer that turns. `shift`
    /// moves its device space by whole pixels into the space of a binding
    /// the segment shares with others beside it: zero for every other
    /// segment.
    pub(crate) fn of(
        placement: &Placement,
        root_scale: f32,
        turn: SegmentTransform,
        shift: [f32; 2],
    ) -> Self {
        let snap_delta = placement
            .snap_anchor
            .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
            .unwrap_or_default();
        let mut flags = placement.data_flags();
        let clip = match placement.tested_device_clip(root_scale) {
            Some([x, y, width, height]) => {
                flags |= PLACEMENT_CLIPPED;
                [x + shift[0], y + shift[1], width, height]
            }
            None => [0.0; 4],
        };
        let dither_origin = placement
            .snap_anchor
            .map(|anchor| snapped_anchor_device_origin(anchor, root_scale))
            .unwrap_or_default();
        if !turn.is_identity() {
            flags |= PLACEMENT_TURNED;
        }
        let (transform, translation, _) = turn.uniform_parts();
        let (color_matrix, color_offset) = match &placement.color_filter {
            Some(filter) => {
                flags |= PLACEMENT_FILTERED;
                let m = filter.as_matrix();
                let column = |j: usize| [m[j], m[5 + j], m[10 + j], m[15 + j]];
                (
                    [column(0), column(1), column(2), column(3)],
                    [m[4], m[9], m[14], m[19]],
                )
            }
            None => (
                [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
                [0.0; 4],
            ),
        };
        Self {
            offset: [
                placement.offset.x + snap_delta.x + shift[0] / root_scale,
                placement.offset.y + snap_delta.y + shift[1] / root_scale,
            ],
            root_scale,
            flags,
            clip,
            dither_origin: [dither_origin.x + shift[0], dither_origin.y + shift[1]],
            alpha: placement.alpha,
            clip_radius: placement
                .rounding
                .filter(|_| placement.clip_rounded())
                .map_or(0.0, |rounding| rounding.radius * root_scale),
            color_matrix,
            color_offset,
            transform,
            translation,
            transform_reserved: [0.0; 2],
        }
    }
}
