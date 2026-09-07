use cranpose_core::NodeId;
use cranpose_ui_graphics::{Point, Rect};

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

/// Number of distinct [`LayerRasterCacheKey`] kinds; see
/// [`LayerRasterCacheKey::kind_slot`] and [`LAYER_RASTER_CACHE_KIND_LABELS`].
pub const LAYER_RASTER_CACHE_KIND_COUNT: usize = 4;

/// Short labels per kind slot, in [`LayerRasterCacheKey::kind_slot`] order.
pub const LAYER_RASTER_CACHE_KIND_LABELS: [&str; LAYER_RASTER_CACHE_KIND_COUNT] =
    ["src", "backdrop", "range", "prefix"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum LayerRasterCacheKind {
    SourceContent,
    BackdropEffect,
    SceneRange,
    PrefixSnapshot,
}

impl LayerRasterCacheKind {
    fn identity_kind(self) -> u8 {
        match self {
            Self::SourceContent => 0,
            Self::BackdropEffect => 1,
            Self::SceneRange => 2,
            Self::PrefixSnapshot => 3,
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
    device_phase_steps: [u32; 2],
}

const DEVICE_PHASE_STEPS: f32 = 16.0;

fn device_phase_steps(phase: Point) -> [u32; 2] {
    let steps = |value: f32| {
        ((value.rem_euclid(1.0) * DEVICE_PHASE_STEPS).round() as u32) % DEVICE_PHASE_STEPS as u32
    };
    [steps(phase.x), steps(phase.y)]
}

fn local_bounds_bits(local_bounds: Rect) -> [u32; 4] {
    [
        local_bounds.x.to_bits(),
        local_bounds.y.to_bits(),
        local_bounds.width.to_bits(),
        local_bounds.height.to_bits(),
    ]
}

impl LayerRasterCacheKey {
    pub fn source_content(
        stable_id: Option<NodeId>,
        content_hash: u64,
        local_bounds: Rect,
        pixel_size: (u32, u32),
        scale_bucket: ScaleBucket,
        device_phase: Point,
    ) -> Self {
        Self {
            kind: LayerRasterCacheKind::SourceContent,
            stable_id,
            content_hash,
            effect_hash: 0,
            local_bounds_bits: local_bounds_bits(local_bounds),
            pixel_size: [pixel_size.0, pixel_size.1],
            scale_bucket,
            device_phase_steps: device_phase_steps(device_phase),
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
            local_bounds_bits: local_bounds_bits(local_bounds),
            pixel_size: [pixel_size.0, pixel_size.1],
            scale_bucket,
            device_phase_steps: [0; 2],
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
            local_bounds_bits: local_bounds_bits(local_bounds),
            pixel_size: [pixel_size.0, pixel_size.1],
            scale_bucket,
            device_phase_steps: [0; 2],
        }
    }

    /// A snapshot of the scene's rendered prefix: the bytes the target held
    /// after drawing ops `[0, prefix_len)` over the pass's clear color. A
    /// replay of captured bytes is identical to direct rendering by
    /// construction — no flattening, so none of the chained-rounding
    /// divergence flatten entries carry.
    pub fn prefix_snapshot(
        content_hash: u64,
        prefix_len: u64,
        local_bounds: Rect,
        pixel_size: (u32, u32),
        scale_bucket: ScaleBucket,
    ) -> Self {
        Self {
            kind: LayerRasterCacheKind::PrefixSnapshot,
            stable_id: None,
            content_hash,
            effect_hash: prefix_len,
            local_bounds_bits: local_bounds_bits(local_bounds),
            pixel_size: [pixel_size.0, pixel_size.1],
            scale_bucket,
            device_phase_steps: [0; 2],
        }
    }

    /// Index of this key's kind in `0..LAYER_RASTER_CACHE_KIND_COUNT`, for
    /// per-kind accounting.
    pub fn kind_slot(self) -> usize {
        self.kind.identity_kind() as usize
    }

    pub fn stable_id(self) -> Option<NodeId> {
        self.stable_id
    }

    pub fn is_scene_range(self) -> bool {
        matches!(
            self.kind,
            LayerRasterCacheKind::SceneRange | LayerRasterCacheKind::PrefixSnapshot
        )
    }

    pub fn is_source_content(self) -> bool {
        self.kind == LayerRasterCacheKind::SourceContent
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

    /// The bit pattern of the entry's local bounds: the place a keyless entry
    /// occupies, stable while its content changes.
    pub fn local_bounds_bits(self) -> [u32; 4] {
        self.local_bounds_bits
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
        let base = LayerRasterCacheKey::source_content(
            Some(7),
            11,
            rect,
            (30, 40),
            ScaleBucket::from_scale(1.0),
            Point::default(),
        );
        let moved = LayerRasterCacheKey::source_content(
            Some(7),
            11,
            Rect { x: 2.0, ..rect },
            (30, 40),
            ScaleBucket::from_scale(1.0),
            Point::default(),
        );
        let resized = LayerRasterCacheKey::source_content(
            Some(7),
            11,
            rect,
            (60, 80),
            ScaleBucket::from_scale(2.0),
            Point::default(),
        );

        assert_ne!(base, moved);
        assert_ne!(base, resized);
        assert_eq!(base.stable_id(), Some(7));
        assert_eq!(base.pixel_size(), (30, 40));
    }

    #[test]
    fn layer_raster_cache_key_captures_the_device_phase() {
        let rect = Rect {
            x: 1.0,
            y: 2.0,
            width: 30.0,
            height: 40.0,
        };
        let key = |phase: Point| {
            LayerRasterCacheKey::source_content(
                Some(7),
                11,
                rect,
                (30, 40),
                ScaleBucket::from_scale(1.0),
                phase,
            )
        };
        assert_ne!(key(Point::default()), key(Point::new(0.5, 0.0)));
        assert_eq!(key(Point::new(0.5, 0.25)), key(Point::new(1.5, -0.75)));
        assert_eq!(key(Point::new(0.01, 0.0)), key(Point::default()));
    }

    #[test]
    fn source_content_keys_separate_by_content_hash() {
        let rect = Rect {
            x: 1.0,
            y: 2.0,
            width: 30.0,
            height: 40.0,
        };
        let scale = ScaleBucket::from_scale(1.0);
        let source = LayerRasterCacheKey::source_content(
            Some(7),
            11,
            rect,
            (30, 40),
            scale,
            Point::default(),
        );
        let other = LayerRasterCacheKey::source_content(
            Some(7),
            12,
            rect,
            (30, 40),
            scale,
            Point::default(),
        );

        assert_ne!(source, other);
        assert_eq!(source.identity(), other.identity());
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
        let source = LayerRasterCacheKey::source_content(
            Some(7),
            11,
            rect,
            (30, 40),
            scale,
            Point::default(),
        );

        assert_ne!(backdrop, source);
        assert_ne!(backdrop.identity(), source.identity());
    }

    #[test]
    fn prefix_snapshot_keys_share_the_scene_range_partition_but_never_a_key() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 240.0,
        };
        let scale = ScaleBucket::from_scale(1.0);
        let prefix = LayerRasterCacheKey::prefix_snapshot(11, 7, rect, (320, 240), scale);
        let range = LayerRasterCacheKey::scene_range(11, rect, (320, 240), scale);
        let longer = LayerRasterCacheKey::prefix_snapshot(11, 8, rect, (320, 240), scale);

        assert!(prefix.is_scene_range());
        assert_ne!(prefix, range);
        assert_ne!(prefix, longer);
        assert_eq!(prefix.identity(), None);
        assert_eq!(prefix.pixel_size(), (320, 240));
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
        let source = LayerRasterCacheKey::source_content(
            None,
            11,
            rect,
            (320, 240),
            scale,
            Point::default(),
        );

        assert_ne!(range, source);
        assert_eq!(range.identity(), None);
    }
}
