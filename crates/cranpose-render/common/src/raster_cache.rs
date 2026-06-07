use cranpose_core::NodeId;
use cranpose_ui_graphics::Rect;

const SCALE_BUCKET_STEPS: f32 = 256.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScaleBucket(u32);

impl ScaleBucket {
    pub fn from_scale(scale: f32) -> Self {
        let normalized = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        Self((normalized * SCALE_BUCKET_STEPS).round().max(1.0) as u32)
    }

    pub fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerRasterCacheHashes {
    pub target_content: u64,
    pub effect: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum LayerRasterCacheKind {
    FullSurface,
    SourceContent,
    BackdropEffect,
    SceneRange,
}

impl LayerRasterCacheKind {
    fn identity_kind(self) -> u8 {
        match self {
            Self::FullSurface => 0,
            Self::SourceContent => 1,
            Self::BackdropEffect => 2,
            Self::SceneRange => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerRasterCacheIdentity {
    stable_id: NodeId,
    kind: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerRasterCacheKey {
    kind: LayerRasterCacheKind,
    stable_id: Option<NodeId>,
    content_hash: u64,
    effect_hash: u64,
    local_bounds_bits: [u32; 4],
    pixel_size: [u32; 2],
    scale_bucket: ScaleBucket,
}

impl LayerRasterCacheKey {
    pub fn new(
        stable_id: Option<NodeId>,
        content_hash: u64,
        effect_hash: u64,
        local_bounds: Rect,
        pixel_size: (u32, u32),
        scale_bucket: ScaleBucket,
    ) -> Self {
        Self {
            kind: LayerRasterCacheKind::FullSurface,
            stable_id,
            content_hash,
            effect_hash,
            local_bounds_bits: [
                local_bounds.x.to_bits(),
                local_bounds.y.to_bits(),
                local_bounds.width.to_bits(),
                local_bounds.height.to_bits(),
            ],
            pixel_size: [pixel_size.0, pixel_size.1],
            scale_bucket,
        }
    }

    pub fn source_content(
        stable_id: Option<NodeId>,
        content_hash: u64,
        local_bounds: Rect,
        pixel_size: (u32, u32),
        scale_bucket: ScaleBucket,
    ) -> Self {
        Self {
            kind: LayerRasterCacheKind::SourceContent,
            stable_id,
            content_hash,
            effect_hash: 0,
            local_bounds_bits: [
                local_bounds.x.to_bits(),
                local_bounds.y.to_bits(),
                local_bounds.width.to_bits(),
                local_bounds.height.to_bits(),
            ],
            pixel_size: [pixel_size.0, pixel_size.1],
            scale_bucket,
        }
    }

    pub fn backdrop_effect(
        stable_id: Option<NodeId>,
        input_hash: u64,
        effect_hash: u64,
        local_bounds: Rect,
        pixel_size: (u32, u32),
        scale_bucket: ScaleBucket,
    ) -> Self {
        Self {
            kind: LayerRasterCacheKind::BackdropEffect,
            stable_id,
            content_hash: input_hash,
            effect_hash,
            local_bounds_bits: [
                local_bounds.x.to_bits(),
                local_bounds.y.to_bits(),
                local_bounds.width.to_bits(),
                local_bounds.height.to_bits(),
            ],
            pixel_size: [pixel_size.0, pixel_size.1],
            scale_bucket,
        }
    }

    pub fn scene_range(
        content_hash: u64,
        local_bounds: Rect,
        pixel_size: (u32, u32),
        scale_bucket: ScaleBucket,
    ) -> Self {
        Self {
            kind: LayerRasterCacheKind::SceneRange,
            stable_id: None,
            content_hash,
            effect_hash: 0,
            local_bounds_bits: [
                local_bounds.x.to_bits(),
                local_bounds.y.to_bits(),
                local_bounds.width.to_bits(),
                local_bounds.height.to_bits(),
            ],
            pixel_size: [pixel_size.0, pixel_size.1],
            scale_bucket,
        }
    }

    pub fn stable_id(self) -> Option<NodeId> {
        self.stable_id
    }

    pub fn is_scene_range(self) -> bool {
        self.kind == LayerRasterCacheKind::SceneRange
    }

    pub fn identity(self) -> Option<LayerRasterCacheIdentity> {
        Some(LayerRasterCacheIdentity {
            stable_id: self.stable_id?,
            kind: self.kind.identity_kind(),
        })
    }

    pub fn pixel_size(self) -> (u32, u32) {
        (self.pixel_size[0], self.pixel_size[1])
    }

    pub fn scale_bucket(self) -> ScaleBucket {
        self.scale_bucket
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_bucket_normalizes_invalid_values() {
        assert_eq!(
            ScaleBucket::from_scale(0.0).raw(),
            ScaleBucket::from_scale(1.0).raw()
        );
        assert_eq!(
            ScaleBucket::from_scale(-3.0).raw(),
            ScaleBucket::from_scale(1.0).raw()
        );
        assert_eq!(
            ScaleBucket::from_scale(f32::NAN).raw(),
            ScaleBucket::from_scale(1.0).raw()
        );
    }

    #[test]
    fn scale_bucket_quantizes_small_fractional_changes() {
        let a = ScaleBucket::from_scale(1.0);
        let b = ScaleBucket::from_scale(1.001);
        let c = ScaleBucket::from_scale(1.01);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn layer_raster_cache_key_captures_bounds_and_pixel_size() {
        let rect = Rect {
            x: 1.0,
            y: 2.0,
            width: 30.0,
            height: 40.0,
        };
        let base = LayerRasterCacheKey::new(
            Some(7),
            11,
            13,
            rect,
            (30, 40),
            ScaleBucket::from_scale(1.0),
        );
        let moved = LayerRasterCacheKey::new(
            Some(7),
            11,
            13,
            Rect { x: 2.0, ..rect },
            (30, 40),
            ScaleBucket::from_scale(1.0),
        );
        let resized = LayerRasterCacheKey::new(
            Some(7),
            11,
            13,
            rect,
            (60, 80),
            ScaleBucket::from_scale(2.0),
        );

        assert_ne!(base, moved);
        assert_ne!(base, resized);
        assert_eq!(base.stable_id(), Some(7));
        assert_eq!(base.pixel_size(), (30, 40));
    }

    #[test]
    fn source_content_keys_do_not_collide_with_full_surface_keys() {
        let rect = Rect {
            x: 1.0,
            y: 2.0,
            width: 30.0,
            height: 40.0,
        };
        let scale = ScaleBucket::from_scale(1.0);
        let source = LayerRasterCacheKey::source_content(Some(7), 11, rect, (30, 40), scale);
        let full = LayerRasterCacheKey::new(Some(7), 11, 0, rect, (30, 40), scale);

        assert_ne!(source, full);
        assert_ne!(source.identity(), full.identity());
    }

    #[test]
    fn backdrop_effect_keys_do_not_collide_with_layer_surface_keys() {
        let rect = Rect {
            x: 1.0,
            y: 2.0,
            width: 30.0,
            height: 40.0,
        };
        let scale = ScaleBucket::from_scale(1.0);
        let backdrop = LayerRasterCacheKey::backdrop_effect(Some(7), 11, 13, rect, (30, 40), scale);
        let source = LayerRasterCacheKey::source_content(Some(7), 11, rect, (30, 40), scale);
        let full = LayerRasterCacheKey::new(Some(7), 11, 13, rect, (30, 40), scale);

        assert_ne!(backdrop, source);
        assert_ne!(backdrop, full);
        assert_ne!(backdrop.identity(), source.identity());
        assert_ne!(backdrop.identity(), full.identity());
    }

    #[test]
    fn scene_range_keys_do_not_collide_with_layer_surface_keys() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 240.0,
        };
        let scale = ScaleBucket::from_scale(1.0);
        let range = LayerRasterCacheKey::scene_range(11, rect, (320, 240), scale);
        let source = LayerRasterCacheKey::source_content(None, 11, rect, (320, 240), scale);
        let full = LayerRasterCacheKey::new(None, 11, 0, rect, (320, 240), scale);

        assert_ne!(range, source);
        assert_ne!(range, full);
        assert_eq!(range.identity(), None);
    }
}
