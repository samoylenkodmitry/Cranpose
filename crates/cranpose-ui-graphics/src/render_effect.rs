//! Render effects that can be applied to graphics layers.
//!
//! Matches the Jetpack Compose `RenderEffect` API with extensions for custom
//! WGSL shaders (`RuntimeShader`).

use crate::LayerShape;
use std::sync::{Arc, Mutex, OnceLock, Weak};

const RUNTIME_SHADER_INLINE_UNIFORMS: usize = 16;

/// Edge treatment for blur effects at the boundary of the blurred region.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TileMode {
    /// Clamp to the edge pixel color.
    #[default]
    Clamp,
    /// Repeat the gradient/effect from start to end.
    Repeated,
    /// Mirror the gradient/effect every other repetition.
    Mirror,
    /// Treat pixels outside the boundary as transparent.
    Decal,
}

/// Controls blur behavior outside source bounds.
///
/// This mirrors Compose's `BlurredEdgeTreatment`:
/// - bounded treatment (`shape != None`) clips blur output and uses `TileMode::Clamp`
/// - unbounded treatment (`shape == None`) does not clip and uses `TileMode::Decal`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlurredEdgeTreatment {
    shape: Option<LayerShape>,
}

impl BlurredEdgeTreatment {
    /// Bounded treatment that clips to a rectangle.
    pub const RECTANGLE: Self = Self {
        shape: Some(LayerShape::Rectangle),
    };

    /// Unbounded treatment that does not clip blurred output.
    pub const UNBOUNDED: Self = Self { shape: None };

    /// Bounded treatment with a specific clip shape.
    pub const fn with_shape(shape: LayerShape) -> Self {
        Self { shape: Some(shape) }
    }

    pub fn shape(self) -> Option<LayerShape> {
        self.shape
    }

    pub fn clip(self) -> bool {
        self.shape.is_some()
    }

    pub fn tile_mode(self) -> TileMode {
        if self.clip() {
            TileMode::Clamp
        } else {
            TileMode::Decal
        }
    }
}

impl Default for BlurredEdgeTreatment {
    fn default() -> Self {
        Self::RECTANGLE
    }
}

/// A custom WGSL shader effect, analogous to Android's `RuntimeShader`.
///
/// The shader source must be a complete WGSL module that declares:
/// ```wgsl
/// @group(0) @binding(0) var input_texture: texture_2d<f32>;
/// @group(0) @binding(1) var input_sampler: sampler;
/// @group(1) @binding(0) var<uniform> u: array<vec4<f32>, 64>;
/// ```
///
/// Float uniforms are packed linearly into the `u` array. Access them in WGSL
/// as `u[index / 4][index % 4]` for individual floats, or `u[index / 4].xy`
/// for vec2, etc. User uniforms may use indices `0..248`; slots `248..256`
/// are reserved for renderer metadata.
///
/// RuntimeShader pipelines operate on premultiplied-alpha textures. Custom
/// shaders should preserve premultiplied output semantics.
#[derive(Clone, Debug)]
pub struct RuntimeShader {
    source: Arc<str>,
    source_hash: u64,
    uniforms: RuntimeShaderUniforms,
    input_padding: f32,
}

#[derive(Clone, Debug, PartialEq)]
struct RuntimeShaderUniforms {
    len: usize,
    inline: [f32; RUNTIME_SHADER_INLINE_UNIFORMS],
    heap: Option<Vec<f32>>,
}

impl RuntimeShaderUniforms {
    fn new() -> Self {
        Self {
            len: 0,
            inline: [0.0; RUNTIME_SHADER_INLINE_UNIFORMS],
            heap: None,
        }
    }

    fn as_slice(&self) -> &[f32] {
        if let Some(heap) = &self.heap {
            heap.as_slice()
        } else {
            &self.inline[..self.len]
        }
    }

    fn len(&self) -> usize {
        self.as_slice().len()
    }

    fn ensure_len(&mut self, min_len: usize) {
        if let Some(heap) = &mut self.heap {
            if heap.len() < min_len {
                heap.resize(min_len, 0.0);
            }
            return;
        }

        if min_len <= RUNTIME_SHADER_INLINE_UNIFORMS {
            self.len = self.len.max(min_len);
            return;
        }

        let mut heap = Vec::with_capacity(min_len);
        heap.extend_from_slice(&self.inline[..self.len]);
        heap.resize(min_len, 0.0);
        self.heap = Some(heap);
    }

    fn set(&mut self, index: usize, value: f32) {
        if let Some(heap) = &mut self.heap {
            heap[index] = value;
        } else {
            self.inline[index] = value;
        }
    }

    #[cfg(test)]
    fn is_inline(&self) -> bool {
        self.heap.is_none()
    }
}

/// Error returned when a shader uniform write targets renderer-owned storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RuntimeShaderUniformError {
    #[error(
        "uniform range starting at {index} with width {width} exceeds user uniform range 0..{max_user_uniforms}; slots {reserved_start}..{max_uniforms} are reserved for renderer data"
    )]
    OutOfUserRange {
        index: usize,
        width: usize,
        max_user_uniforms: usize,
        reserved_start: usize,
        max_uniforms: usize,
    },
}

impl RuntimeShader {
    /// Total uniform storage size in floats (64 vec4s = 256 floats).
    ///
    /// The final slots are reserved for renderer-managed data.
    pub const MAX_UNIFORMS: usize = 256;
    /// First renderer-reserved uniform slot.
    pub const RESERVED_UNIFORM_START: usize = 248;
    /// Maximum user-addressable uniform count.
    pub const MAX_USER_UNIFORMS: usize = Self::RESERVED_UNIFORM_START;

    /// Create a new RuntimeShader from WGSL source code.
    #[track_caller]
    pub fn new(wgsl_source: &str) -> Self {
        let (source, source_hash) =
            cached_shader_source(std::panic::Location::caller(), wgsl_source);
        Self {
            source,
            source_hash,
            uniforms: RuntimeShaderUniforms::new(),
            input_padding: 0.0,
        }
    }

    /// Create a RuntimeShader from shared WGSL source code.
    ///
    /// This avoids repeatedly copying large shader modules for animated effects
    /// that rebuild only their uniform payload every frame.
    pub fn from_shared_source(source: Arc<str>) -> Self {
        let source_hash = cached_shared_shader_source_hash(&source);
        Self {
            source,
            source_hash,
            uniforms: RuntimeShaderUniforms::new(),
            input_padding: 0.0,
        }
    }

    /// Declares how far the shader may sample outside its effect rect, in
    /// logical pixels. Backdrop rendering uses this to capture enough input
    /// around refractive and displacement shaders.
    pub fn set_input_padding(&mut self, padding: f32) {
        self.input_padding = if padding.is_finite() {
            padding.max(0.0)
        } else {
            0.0
        };
    }

    /// Returns the declared input padding in logical pixels.
    pub fn input_padding(&self) -> f32 {
        self.input_padding
    }

    /// Set a single float uniform at the given index.
    ///
    /// Invalid renderer-reserved ranges are ignored. Use [`Self::try_set_float`]
    /// when the caller needs to handle invalid uniform writes explicitly.
    pub fn set_float(&mut self, index: usize, value: f32) {
        let _ = self.try_set_float(index, value);
    }

    /// Set a single float uniform at the given index.
    pub fn try_set_float(
        &mut self,
        index: usize,
        value: f32,
    ) -> Result<(), RuntimeShaderUniformError> {
        self.try_ensure_capacity(index, 1)?;
        self.uniforms.set(index, value);
        Ok(())
    }

    /// Set a vec2 uniform at the given index (consumes indices `[index, index+1]`).
    ///
    /// Invalid renderer-reserved ranges are ignored. Use [`Self::try_set_float2`]
    /// when the caller needs to handle invalid uniform writes explicitly.
    pub fn set_float2(&mut self, index: usize, x: f32, y: f32) {
        let _ = self.try_set_float2(index, x, y);
    }

    /// Set a vec2 uniform at the given index (consumes indices `[index, index+1]`).
    pub fn try_set_float2(
        &mut self,
        index: usize,
        x: f32,
        y: f32,
    ) -> Result<(), RuntimeShaderUniformError> {
        self.try_ensure_capacity(index, 2)?;
        self.uniforms.set(index, x);
        self.uniforms.set(index + 1, y);
        Ok(())
    }

    /// Set a vec4 uniform at the given index (consumes indices `[index..index+4]`).
    ///
    /// Invalid renderer-reserved ranges are ignored. Use [`Self::try_set_float4`]
    /// when the caller needs to handle invalid uniform writes explicitly.
    pub fn set_float4(&mut self, index: usize, x: f32, y: f32, z: f32, w: f32) {
        let _ = self.try_set_float4(index, x, y, z, w);
    }

    /// Set a vec4 uniform at the given index (consumes indices `[index..index+4]`).
    pub fn try_set_float4(
        &mut self,
        index: usize,
        x: f32,
        y: f32,
        z: f32,
        w: f32,
    ) -> Result<(), RuntimeShaderUniformError> {
        self.try_ensure_capacity(index, 4)?;
        self.uniforms.set(index, x);
        self.uniforms.set(index + 1, y);
        self.uniforms.set(index + 2, z);
        self.uniforms.set(index + 3, w);
        Ok(())
    }

    /// Get the WGSL source code.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Get the uniform data as a float slice (for uploading to GPU).
    pub fn uniforms(&self) -> &[f32] {
        self.uniforms.as_slice()
    }

    /// Get the uniform data padded to full 256-float array (for GPU uniform buffer).
    pub fn uniforms_padded(&self) -> [f32; Self::MAX_UNIFORMS] {
        let mut padded = [0.0f32; Self::MAX_UNIFORMS];
        let len = self.uniforms.len().min(Self::MAX_UNIFORMS);
        padded[..len].copy_from_slice(&self.uniforms.as_slice()[..len]);
        padded
    }

    /// Compute a hash of the shader source for pipeline caching.
    pub fn source_hash(&self) -> u64 {
        self.source_hash
    }

    fn try_ensure_capacity(
        &mut self,
        index: usize,
        width: usize,
    ) -> Result<(), RuntimeShaderUniformError> {
        let min_len = index
            .checked_add(width)
            .ok_or_else(|| Self::uniform_range_error(index, width))?;
        if min_len > Self::MAX_USER_UNIFORMS {
            return Err(Self::uniform_range_error(index, width));
        }
        self.uniforms.ensure_len(min_len);
        Ok(())
    }

    fn uniform_range_error(index: usize, width: usize) -> RuntimeShaderUniformError {
        RuntimeShaderUniformError::OutOfUserRange {
            index,
            width,
            max_user_uniforms: Self::MAX_USER_UNIFORMS,
            reserved_start: Self::RESERVED_UNIFORM_START,
            max_uniforms: Self::MAX_UNIFORMS,
        }
    }
}

impl PartialEq for RuntimeShader {
    fn eq(&self, other: &Self) -> bool {
        self.source_hash == other.source_hash
            && (Arc::ptr_eq(&self.source, &other.source)
                || self.source.as_ref() == other.source.as_ref())
            && self.uniforms == other.uniforms
            && self.input_padding.to_bits() == other.input_padding.to_bits()
    }
}

fn hash_shader_source(source: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    source
        .as_bytes()
        .iter()
        .fold(FNV_OFFSET_BASIS, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ShaderSourceCallsite {
    file: &'static str,
    line: u32,
    column: u32,
}

struct CachedShaderSource {
    callsite: ShaderSourceCallsite,
    source_hash: u64,
    source: Arc<str>,
}

struct CachedSharedShaderSourceHash {
    byte_ptr: usize,
    len: usize,
    source_hash: u64,
    source: Weak<str>,
}

fn cached_shared_shader_source_hash(source: &Arc<str>) -> u64 {
    static CACHE: OnceLock<Mutex<Vec<CachedSharedShaderSourceHash>>> = OnceLock::new();
    let byte_ptr = source.as_ptr() as usize;
    let len = source.len();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    cache.retain(|entry| entry.source.strong_count() > 0);
    if let Some(entry) = cache.iter().find(|entry| {
        entry.byte_ptr == byte_ptr
            && entry.len == len
            && entry
                .source
                .upgrade()
                .is_some_and(|cached| Arc::ptr_eq(&cached, source))
    }) {
        return entry.source_hash;
    }

    let source_hash = hash_shader_source(source);
    cache.push(CachedSharedShaderSourceHash {
        byte_ptr,
        len,
        source_hash,
        source: Arc::downgrade(source),
    });
    source_hash
}

fn cached_shader_source(
    location: &'static std::panic::Location<'static>,
    source: &str,
) -> (Arc<str>, u64) {
    static CACHE: OnceLock<Mutex<Vec<CachedShaderSource>>> = OnceLock::new();
    let callsite = ShaderSourceCallsite {
        file: location.file(),
        line: location.line(),
        column: location.column(),
    };
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(entry) = cache.iter_mut().find(|entry| entry.callsite == callsite) {
        if entry.source.as_ref() == source {
            return (entry.source.clone(), entry.source_hash);
        }
        let source_hash = hash_shader_source(source);
        entry.source_hash = source_hash;
        entry.source = Arc::<str>::from(source);
        return (entry.source.clone(), entry.source_hash);
    }

    let source_hash = hash_shader_source(source);
    let shared = Arc::<str>::from(source);
    cache.push(CachedShaderSource {
        callsite,
        source_hash,
        source: shared.clone(),
    });
    (shared, source_hash)
}

/// A render effect applied to a graphics layer's rendered content.
///
/// Matches Jetpack Compose's `RenderEffect` sealed class hierarchy,
/// extended with `Shader` for custom WGSL effects.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderEffect {
    /// Gaussian blur applied to the layer's rendered content.
    Blur {
        radius_x: f32,
        radius_y: f32,
        edge_treatment: TileMode,
    },
    /// Offset the rendered content by a fixed amount.
    Offset { offset_x: f32, offset_y: f32 },
    /// Apply a custom WGSL shader effect.
    Shader { shader: RuntimeShader },
    /// Chain two effects: apply `first`, then apply `second` to the result.
    Chain {
        first: Box<RenderEffect>,
        second: Box<RenderEffect>,
    },
}

impl RenderEffect {
    /// Create a blur effect with equal radius in both directions.
    pub fn blur(radius: f32) -> Self {
        Self::blur_with_edge_treatment(radius, TileMode::default())
    }

    /// Create a blur effect with equal radius in both directions and explicit
    /// edge treatment semantics.
    pub fn blur_with_edge_treatment(radius: f32, edge_treatment: TileMode) -> Self {
        Self::Blur {
            radius_x: radius,
            radius_y: radius,
            edge_treatment,
        }
    }

    /// Create a blur effect with separate horizontal and vertical radii.
    pub fn blur_xy(radius_x: f32, radius_y: f32, edge_treatment: TileMode) -> Self {
        Self::Blur {
            radius_x,
            radius_y,
            edge_treatment,
        }
    }

    /// Create an offset effect.
    pub fn offset(offset_x: f32, offset_y: f32) -> Self {
        Self::Offset { offset_x, offset_y }
    }

    /// Create a custom shader effect from a RuntimeShader.
    pub fn runtime_shader(shader: RuntimeShader) -> Self {
        Self::Shader { shader }
    }

    /// Chain this effect with another: `self` is applied first, then `other`.
    pub fn then(self, other: RenderEffect) -> Self {
        Self::Chain {
            first: Box::new(self),
            second: Box::new(other),
        }
    }

    /// Returns `true` if this effect or any chained sub-effect is a
    /// `RuntimeShader`. Animated shaders produce different output every frame,
    /// so layer surface caching is counterproductive for them.
    pub fn contains_runtime_shader(&self) -> bool {
        match self {
            RenderEffect::Shader { .. } => true,
            RenderEffect::Chain { first, second } => {
                first.contains_runtime_shader() || second.contains_runtime_shader()
            }
            _ => false,
        }
    }

    /// Maximum logical-pixel input padding required by this effect.
    pub fn input_padding(&self) -> f32 {
        match self {
            RenderEffect::Shader { shader } => shader.input_padding(),
            RenderEffect::Chain { first, second } => {
                first.input_padding().max(second.input_padding())
            }
            RenderEffect::Blur { .. } | RenderEffect::Offset { .. } => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RoundedCornerShape;

    #[test]
    fn runtime_shader_set_uniforms() {
        let mut shader = RuntimeShader::new("// test");
        shader.set_float(0, 1.0);
        shader.set_float2(2, 3.0, 4.0);
        shader.set_float4(4, 5.0, 6.0, 7.0, 8.0);

        assert_eq!(shader.uniforms()[0], 1.0);
        assert_eq!(shader.uniforms()[1], 0.0); // gap
        assert_eq!(shader.uniforms()[2], 3.0);
        assert_eq!(shader.uniforms()[3], 4.0);
        assert_eq!(shader.uniforms()[4], 5.0);
        assert_eq!(shader.uniforms()[5], 6.0);
        assert_eq!(shader.uniforms()[6], 7.0);
        assert_eq!(shader.uniforms()[7], 8.0);
    }

    #[test]
    fn runtime_shader_padded() {
        let mut shader = RuntimeShader::new("// test");
        shader.set_float(0, 42.0);
        let padded = shader.uniforms_padded();
        assert_eq!(padded[0], 42.0);
        assert_eq!(padded[1], 0.0);
        assert_eq!(padded[255], 0.0);
    }

    #[test]
    fn runtime_shader_keeps_common_uniform_payload_inline() {
        let mut shader = RuntimeShader::new("// test");
        shader.set_float4(0, 1.0, 2.0, 3.0, 4.0);
        shader.set_float4(4, 5.0, 6.0, 7.0, 8.0);
        shader.set_float4(8, 9.0, 10.0, 11.0, 12.0);
        shader.set_float4(12, 13.0, 14.0, 15.0, 16.0);

        assert!(shader.uniforms.is_inline());
        assert_eq!(shader.uniforms().len(), 16);

        shader.set_float(16, 17.0);
        assert!(!shader.uniforms.is_inline());
        assert_eq!(shader.uniforms()[16], 17.0);
    }

    #[test]
    fn runtime_shader_try_set_reports_reserved_uniform_slots() {
        let mut shader = RuntimeShader::new("// test");

        let err = shader
            .try_set_float(RuntimeShader::RESERVED_UNIFORM_START, 1.0)
            .unwrap_err();
        assert_eq!(
            err,
            RuntimeShaderUniformError::OutOfUserRange {
                index: RuntimeShader::RESERVED_UNIFORM_START,
                width: 1,
                max_user_uniforms: RuntimeShader::MAX_USER_UNIFORMS,
                reserved_start: RuntimeShader::RESERVED_UNIFORM_START,
                max_uniforms: RuntimeShader::MAX_UNIFORMS,
            }
        );
        assert!(shader.uniforms().is_empty());

        let err = shader
            .try_set_float4(RuntimeShader::MAX_USER_UNIFORMS - 3, 1.0, 2.0, 3.0, 4.0)
            .unwrap_err();
        assert_eq!(
            err,
            RuntimeShaderUniformError::OutOfUserRange {
                index: RuntimeShader::MAX_USER_UNIFORMS - 3,
                width: 4,
                max_user_uniforms: RuntimeShader::MAX_USER_UNIFORMS,
                reserved_start: RuntimeShader::RESERVED_UNIFORM_START,
                max_uniforms: RuntimeShader::MAX_UNIFORMS,
            }
        );
    }

    #[test]
    fn runtime_shader_setters_ignore_invalid_uniform_slots_without_panicking() {
        let mut shader = RuntimeShader::new("// test");
        shader.set_float(0, 7.0);

        shader.set_float(RuntimeShader::RESERVED_UNIFORM_START, 1.0);
        shader.set_float4(RuntimeShader::MAX_USER_UNIFORMS - 3, 1.0, 2.0, 3.0, 4.0);

        assert_eq!(shader.uniforms(), &[7.0]);
    }

    #[test]
    fn render_effect_chaining() {
        let blur = RenderEffect::blur(10.0);
        let offset = RenderEffect::offset(5.0, 5.0);
        let chained = blur.then(offset);
        match chained {
            RenderEffect::Chain { first, second } => {
                assert!(matches!(*first, RenderEffect::Blur { .. }));
                assert!(matches!(*second, RenderEffect::Offset { .. }));
            }
            _ => panic!("expected Chain"),
        }
    }

    #[test]
    fn blur_convenience() {
        let effect = RenderEffect::blur(15.0);
        match effect {
            RenderEffect::Blur {
                radius_x,
                radius_y,
                edge_treatment,
            } => {
                assert_eq!(radius_x, 15.0);
                assert_eq!(radius_y, 15.0);
                assert_eq!(edge_treatment, TileMode::Clamp);
            }
            _ => panic!("expected Blur"),
        }
    }

    #[test]
    fn blur_with_edge_treatment_uses_explicit_mode() {
        let effect = RenderEffect::blur_with_edge_treatment(6.0, TileMode::Decal);
        match effect {
            RenderEffect::Blur {
                radius_x,
                radius_y,
                edge_treatment,
            } => {
                assert_eq!(radius_x, 6.0);
                assert_eq!(radius_y, 6.0);
                assert_eq!(edge_treatment, TileMode::Decal);
            }
            _ => panic!("expected Blur"),
        }
    }

    #[test]
    fn source_hash_consistent() {
        let s1 = RuntimeShader::new("fn main() {}");
        let s2 = RuntimeShader::new("fn main() {}");
        assert_eq!(s1.source_hash(), s2.source_hash());
    }

    #[test]
    fn runtime_shader_from_shared_source_reuses_shared_source() {
        let source = Arc::<str>::from("fn fragment() -> vec4<f32> { return vec4<f32>(1.0); }");
        let s1 = RuntimeShader::from_shared_source(source.clone());
        let s2 = RuntimeShader::from_shared_source(source);

        assert!(Arc::ptr_eq(&s1.source, &s2.source));
        assert_eq!(s1.source_hash(), s2.source_hash());
    }

    fn runtime_shader_from_reuse_callsite(source: &str) -> RuntimeShader {
        RuntimeShader::new(source)
    }

    fn runtime_shader_from_replacement_callsite(source: &str) -> RuntimeShader {
        RuntimeShader::new(source)
    }

    #[test]
    fn runtime_shader_new_reuses_same_callsite_source() {
        let source = "fn fragment() -> vec4<f32> { return vec4<f32>(1.0); }";
        let s1 = runtime_shader_from_reuse_callsite(source);
        let s2 = runtime_shader_from_reuse_callsite(source);

        assert!(Arc::ptr_eq(&s1.source, &s2.source));
        assert_eq!(s1.source_hash(), s2.source_hash());
    }

    #[test]
    fn runtime_shader_new_replaces_changed_callsite_source() {
        let s1 = runtime_shader_from_replacement_callsite("fn a() {}");
        let s2 = runtime_shader_from_replacement_callsite("fn b() {}");

        assert!(!Arc::ptr_eq(&s1.source, &s2.source));
        assert_ne!(s1.source_hash(), s2.source_hash());
        assert_eq!(s2.source(), "fn b() {}");
    }

    #[test]
    fn runtime_shader_source_storage_has_no_process_global_interner() {
        let source = include_str!("render_effect.rs");
        let blocked_static = ["static ", "INTERNER"].concat();
        let blocked_type = ["ShaderSource", "Interner"].concat();

        assert!(
            !source.contains(&blocked_static) && !source.contains(&blocked_type),
            "RuntimeShader source sharing must be explicit via from_shared_source, not a process-global interner"
        );
    }

    #[test]
    fn blur_xy_preserves_tile_mode() {
        let effect = RenderEffect::blur_xy(3.0, 7.0, TileMode::Clamp);
        match effect {
            RenderEffect::Blur {
                radius_x,
                radius_y,
                edge_treatment,
            } => {
                assert_eq!(radius_x, 3.0);
                assert_eq!(radius_y, 7.0);
                assert_eq!(edge_treatment, TileMode::Clamp);
            }
            _ => panic!("expected Blur"),
        }
    }

    #[test]
    fn offset_constructor_sets_components() {
        let effect = RenderEffect::offset(11.0, -5.0);
        match effect {
            RenderEffect::Offset { offset_x, offset_y } => {
                assert_eq!(offset_x, 11.0);
                assert_eq!(offset_y, -5.0);
            }
            _ => panic!("expected Offset"),
        }
    }

    #[test]
    fn runtime_shader_equality_is_source_value_based() {
        let mut s1 = RuntimeShader::new("fn main() {}");
        let mut s2 = RuntimeShader::new("fn main() {}");
        s1.set_float(0, 1.0);
        s2.set_float(0, 1.0);
        assert_eq!(s1, s2);
    }

    #[test]
    fn blurred_edge_treatment_defaults_to_bounded_rectangle() {
        let treatment = BlurredEdgeTreatment::default();
        assert_eq!(treatment.shape(), Some(LayerShape::Rectangle));
        assert!(treatment.clip());
        assert_eq!(treatment.tile_mode(), TileMode::Clamp);
    }

    #[test]
    fn blurred_edge_treatment_unbounded_uses_decal_and_no_clip() {
        let treatment = BlurredEdgeTreatment::UNBOUNDED;
        assert_eq!(treatment.shape(), None);
        assert!(!treatment.clip());
        assert_eq!(treatment.tile_mode(), TileMode::Decal);
    }

    #[test]
    fn blurred_edge_treatment_with_shape_uses_bounded_mode() {
        let rounded = LayerShape::Rounded(RoundedCornerShape::uniform(8.0));
        let treatment = BlurredEdgeTreatment::with_shape(rounded);
        assert_eq!(treatment.shape(), Some(rounded));
        assert!(treatment.clip());
        assert_eq!(treatment.tile_mode(), TileMode::Clamp);
    }
}
