use cranpose_core::NodeId;
use cranpose_ui_graphics::{Point, Rect};

/// The device scale a layer raster is drawn at, compared exactly: a raster
/// drawn at one scale places its edges and glyphs where no other scale does,
/// so it serves only that scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RasterScale(u32);

impl RasterScale {
    /// The raster scale of `scale`; a scale no raster can be drawn at (zero,
    /// negative or not finite) is the unit scale.
    pub fn from_scale(scale: f32) -> Self {
        let normalized = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        Self(normalized.to_bits())
    }

    /// The scale's bit pattern.
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
pub const LAYER_RASTER_CACHE_KIND_COUNT: usize = 5;

/// Short labels per kind slot, in [`LayerRasterCacheKey::kind_slot`] order.
pub const LAYER_RASTER_CACHE_KIND_LABELS: [&str; LAYER_RASTER_CACHE_KIND_COUNT] =
    ["src", "backdrop", "range", "prefix", "effect"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum LayerRasterCacheKind {
    SourceContent,
    BackdropEffect,
    SceneRange,
    PrefixSnapshot,
    LayerEffect,
}

impl LayerRasterCacheKind {
    fn identity_kind(self) -> u8 {
        match self {
            Self::SourceContent => 0,
            Self::BackdropEffect => 1,
            Self::SceneRange => 2,
            Self::PrefixSnapshot => 3,
            Self::LayerEffect => 4,
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
    raster_scale: RasterScale,
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
        raster_scale: RasterScale,
        device_phase: Point,
    ) -> Self {
        Self {
            kind: LayerRasterCacheKind::SourceContent,
            stable_id,
            content_hash,
            effect_hash: 0,
            local_bounds_bits: local_bounds_bits(local_bounds),
            pixel_size: [pixel_size.0, pixel_size.1],
            raster_scale,
            device_phase_steps: device_phase_steps(device_phase),
        }
    }

    pub fn backdrop_effect(
        stable_id: Option<NodeId>,
        input_hash: u64,
        effect_hash: u64,
        local_bounds: Rect,
        pixel_size: (u32, u32),
        raster_scale: RasterScale,
    ) -> Self {
        Self::effect(
            LayerRasterCacheKind::BackdropEffect,
            stable_id,
            input_hash,
            effect_hash,
            local_bounds,
            pixel_size,
            raster_scale,
        )
    }

    /// A layer's render effect applied over its retained surface: the output
    /// is a pure function of the surface's content, the effect and the
    /// layer's pixel rect within the surface.
    pub fn layer_effect(
        stable_id: Option<NodeId>,
        input_hash: u64,
        effect_hash: u64,
        local_bounds: Rect,
        pixel_size: (u32, u32),
        raster_scale: RasterScale,
    ) -> Self {
        Self::effect(
            LayerRasterCacheKind::LayerEffect,
            stable_id,
            input_hash,
            effect_hash,
            local_bounds,
            pixel_size,
            raster_scale,
        )
    }

    fn effect(
        kind: LayerRasterCacheKind,
        stable_id: Option<NodeId>,
        input_hash: u64,
        effect_hash: u64,
        local_bounds: Rect,
        pixel_size: (u32, u32),
        raster_scale: RasterScale,
    ) -> Self {
        Self {
            kind,
            stable_id,
            content_hash: input_hash,
            effect_hash,
            local_bounds_bits: local_bounds_bits(local_bounds),
            pixel_size: [pixel_size.0, pixel_size.1],
            raster_scale,
            device_phase_steps: [0; 2],
        }
    }

    pub fn scene_range(
        content_hash: u64,
        local_bounds: Rect,
        pixel_size: (u32, u32),
        raster_scale: RasterScale,
    ) -> Self {
        Self {
            kind: LayerRasterCacheKind::SceneRange,
            stable_id: None,
            content_hash,
            effect_hash: 0,
            local_bounds_bits: local_bounds_bits(local_bounds),
            pixel_size: [pixel_size.0, pixel_size.1],
            raster_scale,
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
        raster_scale: RasterScale,
    ) -> Self {
        Self {
            kind: LayerRasterCacheKind::PrefixSnapshot,
            stable_id: None,
            content_hash,
            effect_hash: prefix_len,
            local_bounds_bits: local_bounds_bits(local_bounds),
            pixel_size: [pixel_size.0, pixel_size.1],
            raster_scale,
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

    pub fn raster_scale(self) -> RasterScale {
        self.raster_scale
    }
}

#[cfg(test)]
#[path = "tests/raster_cache_tests.rs"]
mod tests;
