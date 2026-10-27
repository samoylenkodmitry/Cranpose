use bytemuck::{Pod, Zeroable};

use super::{RunTier, ViewportUniformParams};
use crate::run_store::PlacementData;

// The viewport uniform every overlay and shape batch binds, as the shaders
// declare it.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(super) struct Uniforms {
    pub(super) viewport: [f32; 2],
    pub(super) viewport_offset: [f32; 2],
    pub(super) transform: [f32; 4],
    pub(super) translation: [f32; 2],
    pub(super) reserved: [f32; 2],
    pub(super) inverse: [f32; 4],
    pub(super) origin: [f32; 2],
    pub(super) origin_reserved: [f32; 2],
    pub(super) placement: PlacementData,
}

impl Uniforms {
    pub(super) fn of(
        params: ViewportUniformParams,
        placement: PlacementData,
        tier: RunTier,
    ) -> Self {
        let (transform, translation, inverse) = params.transform.uniform_parts();
        Self {
            viewport: [params.width as f32, params.height as f32],
            viewport_offset: params.offset,
            transform,
            translation,
            reserved: [params.depth_base, f32::from(tier as u8)],
            inverse,
            origin: params.origin,
            origin_reserved: [0.0; 2],
            placement,
        }
    }
}
