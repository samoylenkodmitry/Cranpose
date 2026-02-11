//! Render effects that can be applied to graphics layers.
//!
//! Matches the Jetpack Compose `RenderEffect` API with extensions for custom
//! WGSL shaders (`RuntimeShader`).

use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Edge treatment for blur effects at the boundary of the blurred region.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TileMode {
    /// Clamp to the edge pixel color.
    #[default]
    Clamp,
    /// Treat pixels outside the boundary as transparent.
    Decal,
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
    uniforms: Vec<f32>,
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
    pub fn new(wgsl_source: &str) -> Self {
        Self {
            source: Arc::from(wgsl_source),
            uniforms: Vec::new(),
        }
    }

    /// Set a single float uniform at the given index.
    pub fn set_float(&mut self, index: usize, value: f32) {
        self.ensure_capacity(index + 1);
        self.uniforms[index] = value;
    }

    /// Set a vec2 uniform at the given index (consumes indices `[index, index+1]`).
    pub fn set_float2(&mut self, index: usize, x: f32, y: f32) {
        self.ensure_capacity(index + 2);
        self.uniforms[index] = x;
        self.uniforms[index + 1] = y;
    }

    /// Set a vec4 uniform at the given index (consumes indices `[index..index+4]`).
    pub fn set_float4(&mut self, index: usize, x: f32, y: f32, z: f32, w: f32) {
        self.ensure_capacity(index + 4);
        self.uniforms[index] = x;
        self.uniforms[index + 1] = y;
        self.uniforms[index + 2] = z;
        self.uniforms[index + 3] = w;
    }

    /// Get the WGSL source code.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Get the uniform data as a float slice (for uploading to GPU).
    pub fn uniforms(&self) -> &[f32] {
        &self.uniforms
    }

    /// Get the uniform data padded to full 256-float array (for GPU uniform buffer).
    pub fn uniforms_padded(&self) -> [f32; Self::MAX_UNIFORMS] {
        let mut padded = [0.0f32; Self::MAX_UNIFORMS];
        let len = self.uniforms.len().min(Self::MAX_UNIFORMS);
        padded[..len].copy_from_slice(&self.uniforms[..len]);
        padded
    }

    /// Compute a hash of the shader source for pipeline caching.
    pub fn source_hash(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.source.hash(&mut hasher);
        hasher.finish()
    }

    fn ensure_capacity(&mut self, min_len: usize) {
        assert!(
            min_len <= Self::MAX_USER_UNIFORMS,
            "uniform index {} exceeds user maximum {}; slots {}..{} are reserved for renderer data",
            min_len - 1,
            Self::MAX_USER_UNIFORMS - 1,
            Self::RESERVED_UNIFORM_START,
            Self::MAX_UNIFORMS - 1
        );
        if self.uniforms.len() < min_len {
            self.uniforms.resize(min_len, 0.0);
        }
    }
}

impl PartialEq for RuntimeShader {
    fn eq(&self, other: &Self) -> bool {
        self.source.as_ref() == other.source.as_ref() && self.uniforms == other.uniforms
    }
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
        Self::Blur {
            radius_x: radius,
            radius_y: radius,
            edge_treatment: TileMode::default(),
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
    #[should_panic(expected = "uniform index 256 exceeds user maximum 247")]
    fn runtime_shader_overflow() {
        let mut shader = RuntimeShader::new("// test");
        shader.set_float(256, 1.0);
    }

    #[test]
    #[should_panic(expected = "reserved for renderer data")]
    fn runtime_shader_rejects_reserved_uniform_slots() {
        let mut shader = RuntimeShader::new("// test");
        shader.set_float(RuntimeShader::RESERVED_UNIFORM_START, 1.0);
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
    fn source_hash_consistent() {
        let s1 = RuntimeShader::new("fn main() {}");
        let s2 = RuntimeShader::new("fn main() {}");
        assert_eq!(s1.source_hash(), s2.source_hash());
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
}
