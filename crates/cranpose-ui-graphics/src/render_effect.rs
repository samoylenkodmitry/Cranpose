//! Render effects that can be applied to graphics layers.
//!
//! Matches the Jetpack Compose `RenderEffect` API with extensions for custom
//! WGSL shaders (`RuntimeShader`).

use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};

use arrayvec::ArrayVec;

use crate::{LayerShape, Rect};

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

/// The vertex stage and bindings every runtime shader starts from: a
/// fullscreen triangle whose `uv` spans the input, the input texture and
/// sampler at group 0, and the 64 uniform vectors at group 1. A shader
/// source is this prelude followed by an `effect_fs` fragment stage.
pub const RUNTIME_SHADER_PRELUDE_WGSL: &str = concat!(
    include_str!("../shaders/fullscreen_quad_vs.wgsl"),
    include_str!("../shaders/runtime_shader_bindings.wgsl"),
);

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
/// for vec2, etc. User uniforms may use indices `0..224`; slots `224..256`
/// are reserved for renderer metadata:
///
/// | slots     | content                                                     |
/// |-----------|-------------------------------------------------------------|
/// | 224..236  | substrate regions `(x, y, w, h)` in input texels, the third at 224, the second at 228, the first at 232; zero = none |
/// | 236..240  | source region `(x, y, w, h)` in input texels; zero = whole  |
/// | 240..244  | composite mask rect `(x, y, w, h)` in region pixels; zero = none |
/// | 244..248  | composite mask corner radii (top-left, top-right, bottom-left, bottom-right) |
/// | 248..252  | effect rect `(x, y, w, h)` in region pixels                 |
/// | 252..254  | logical size the input represents; zero = its texel size   |
/// | 254       | composite alpha                                             |
///
/// A shader that reads the source region, mask and alpha slots declares it
/// with [`set_batched_source`](Self::set_batched_source); one that reads a
/// low-frequency copy of its source declares each with
/// [`set_substrates`](Self::set_substrates) and samples it through its
/// substrate region, held to that region's texel centers, so one tap
/// stands for a neighbourhood the shader would otherwise walk tap by tap.
/// The renderer then
/// packs its input edge to edge beside other effects' inputs in one texture
/// and draws it straight into the final pass with its clip applied. Such a
/// shader holds every sample coordinate to its region's texel centers: the
/// texels beside the region belong to other effects, or to no one. Every
/// other shader is given the whole texture as its input and `uv` spans it.
///
/// RuntimeShader pipelines operate on premultiplied-alpha textures. Custom
/// shaders should preserve premultiplied output semantics.
#[derive(Clone, Debug)]
pub struct RuntimeShader {
    source: Arc<str>,
    source_hash: u64,
    uniforms: RuntimeShaderUniforms,
    specialization: Option<Arc<ShaderSpecialization>>,
    input_padding: f32,
    output_padding: f32,
    batched_source: bool,
    preserves_transparency: bool,
    domains: Option<Box<ShaderDomains>>,
}

#[derive(Clone, Debug, Default)]
struct ShaderSpecialization {
    overrides: Vec<(&'static str, f64)>,
    overrides_hash: OnceLock<u64>,
    substrates: ArrayVec<SubstrateSpec, MAX_SUBSTRATES>,
    draw_split: Option<&'static str>,
    exact: bool,
}

pub(crate) struct ShaderSpecializationCache<K, const N: usize> {
    entries: ArrayVec<CachedShaderSpecialization<K>, N>,
}

struct CachedShaderSpecialization<K> {
    source: Option<Arc<ShaderSpecialization>>,
    key: K,
    result: Option<Arc<ShaderSpecialization>>,
}

impl<K: PartialEq, const N: usize> ShaderSpecializationCache<K, N> {
    pub(crate) const fn new() -> Self {
        assert!(N > 0);
        Self {
            entries: ArrayVec::new_const(),
        }
    }

    pub(crate) fn apply(
        &mut self,
        shader: &mut RuntimeShader,
        key: K,
        specialize: impl FnOnce(&mut RuntimeShader, &K),
    ) {
        let hit = self.entries.iter().rposition(|entry| {
            entry.key == key
                && match (&entry.source, &shader.specialization) {
                    (Some(source), Some(current)) => Arc::ptr_eq(source, current),
                    (None, None) => true,
                    _ => false,
                }
        });
        if let Some(index) = hit {
            let entry = self.entries.remove(index);
            shader.specialization.clone_from(&entry.result);
            self.entries.push(entry);
            return;
        }
        if shader
            .specialization
            .as_ref()
            .is_some_and(|source| Arc::strong_count(source) == 1)
        {
            specialize(shader, &key);
            return;
        }
        let source = shader.specialization.clone();
        specialize(shader, &key);
        if self.entries.is_full() {
            self.entries.remove(0);
        }
        self.entries.push(CachedShaderSpecialization {
            source,
            key,
            result: shader.specialization.clone(),
        });
    }
}

static DEFAULT_SHADER_SPECIALIZATION: ShaderSpecialization = ShaderSpecialization {
    overrides: Vec::new(),
    overrides_hash: OnceLock::new(),
    substrates: ArrayVec::new_const(),
    draw_split: None,
    exact: false,
};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ShaderDomains {
    output_support: Option<Rect>,
    sample_domain: Option<Rect>,
}

fn finite_rect(rect: Option<Rect>) -> Option<Rect> {
    rect.filter(|rect| {
        rect.x.is_finite()
            && rect.y.is_finite()
            && rect.width.is_finite()
            && rect.height.is_finite()
    })
}

/// The most substrates one shader may declare.
pub const MAX_SUBSTRATES: usize = 3;

/// A low-frequency copy of a shader's source the renderer packs beside it
/// and hands the shader through a reserved substrate region slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SubstrateSpec {
    /// The componentwise source mean over the layer's bounds, stored in one texel.
    /// Filter padding is excluded; bounds are clipped to the capture and rounded
    /// outward to texels. The renderer averages rows and then columns in its
    /// render-target format. A capture outside the layer uses its complete source.
    Mean,
    /// The source averaged in blocks of `block` x `block` texels, one
    /// substrate texel per block.
    Average { block: u32 },
    /// The source blurred by a Gaussian of `radius_px` device pixels, kept
    /// at the blur's scratch resolution.
    Blur { radius_px: f32 },
}

impl SubstrateSpec {
    fn same_bits(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Mean, Self::Mean) => true,
            (Self::Average { block: a }, Self::Average { block: b }) => a == b,
            (Self::Blur { radius_px: a }, Self::Blur { radius_px: b }) => {
                a.to_bits() == b.to_bits()
            }
            _ => false,
        }
    }

    fn hash_bits<H: std::hash::Hasher>(&self, state: &mut H) {
        use std::hash::Hash;
        match self {
            Self::Mean => 2u8.hash(state),
            Self::Average { block } => {
                0u8.hash(state);
                block.hash(state);
            }
            Self::Blur { radius_px } => {
                1u8.hash(state);
                radius_px.to_bits().hash(state);
            }
        }
    }
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
    pub const RESERVED_UNIFORM_START: usize = 224;
    /// Reserved slots of the substrate regions `(x, y, w, h)` in input
    /// texels, in declaration order.
    pub const SUBSTRATE_REGION_UNIFORMS: [usize; MAX_SUBSTRATES] = [232, 228, 224];
    /// Reserved slot of the source region `(x, y, w, h)` in input texels.
    pub const SOURCE_REGION_UNIFORM: usize = 236;
    /// Reserved slot of the composite mask rect `(x, y, w, h)` in region pixels.
    pub const MASK_RECT_UNIFORM: usize = 240;
    /// Reserved slot of the composite mask corner radii.
    pub const MASK_RADII_UNIFORM: usize = 244;
    /// Reserved slot of the effect rect `(x, y, w, h)` in region pixels.
    pub const EFFECT_RECT_UNIFORM: usize = 248;
    /// Reserved slot of the logical size the input represents.
    pub const LOGICAL_SIZE_UNIFORM: usize = 252;
    /// Reserved slot of the composite alpha.
    pub const ALPHA_UNIFORM: usize = 254;
    /// Maximum user-addressable uniform count.
    pub const MAX_USER_UNIFORMS: usize = Self::RESERVED_UNIFORM_START;

    /// Create a new RuntimeShader from WGSL source code.
    #[track_caller]
    pub fn new(wgsl_source: &str) -> Self {
        let (source, source_hash) =
            cached_shader_source(std::panic::Location::caller(), wgsl_source);
        Self::with_source(source, source_hash)
    }

    /// Create a RuntimeShader from shared WGSL source code.
    ///
    /// This avoids repeatedly copying large shader modules for animated effects
    /// that rebuild only their uniform payload every frame.
    pub fn from_shared_source(source: Arc<str>) -> Self {
        let source_hash = cached_shared_shader_source_hash(&source);
        Self::with_source(source, source_hash)
    }

    fn with_source(source: Arc<str>, source_hash: u64) -> Self {
        Self {
            source,
            source_hash,
            uniforms: RuntimeShaderUniforms::new(),
            specialization: None,
            input_padding: 0.0,
            output_padding: 0.0,
            batched_source: false,
            preserves_transparency: false,
            domains: None,
        }
    }

    fn specialization(&self) -> &ShaderSpecialization {
        self.specialization
            .as_deref()
            .unwrap_or(&DEFAULT_SHADER_SPECIALIZATION)
    }

    fn specialization_mut(&mut self) -> &mut ShaderSpecialization {
        Arc::make_mut(self.specialization.get_or_insert_with(Arc::default))
    }

    /// Fixes a pipeline-overridable constant (`override NAME: T = ...;` in
    /// the WGSL) for every pipeline compiled from this shader. The value is
    /// converted to the constant's declared scalar type the way WebGPU does
    /// (a `bool` is `value != 0`). Each distinct override set compiles its
    /// own pipeline; renderers use this to fold a material's inactive
    /// features away without changing the shader text.
    ///
    /// The pipeline compiles inside the frame that first draws the shader,
    /// unless the shader declares its specialization exact with
    /// [`Self::set_specialization_exact`]: then the renderer compiles it in
    /// the background and draws with the general pipeline meanwhile.
    pub fn set_override(&mut self, name: &'static str, value: f64) {
        let position = self
            .overrides()
            .binary_search_by(|(existing, _)| existing.cmp(&name));
        if position.is_ok_and(|index| self.overrides()[index].1.to_bits() == value.to_bits()) {
            return;
        }
        let specialization = self.specialization_mut();
        specialization.overrides_hash.take();
        let overrides = &mut specialization.overrides;
        match position {
            Ok(index) => overrides[index].1 = value,
            Err(index) => overrides.insert(index, (name, value)),
        }
    }

    /// Removes a pipeline override by name, returning whether one was present.
    pub fn clear_override(&mut self, name: &str) -> bool {
        let Ok(index) = self
            .overrides()
            .binary_search_by(|(existing, _)| (*existing).cmp(name))
        else {
            return false;
        };
        let specialization = self.specialization_mut();
        specialization.overrides_hash.take();
        specialization.overrides.remove(index);
        true
    }

    /// The pipeline-overridable constants fixed by [`Self::set_override`],
    /// ordered by name.
    pub fn overrides(&self) -> &[(&'static str, f64)] {
        &self.specialization().overrides
    }

    /// Hash of the fixed override set; zero when no override is fixed.
    pub fn overrides_hash(&self) -> u64 {
        let specialization = self.specialization();
        if specialization.overrides.is_empty() {
            return 0;
        }
        *specialization.overrides_hash.get_or_init(|| {
            #[cfg(test)]
            OVERRIDE_HASH_COMPUTATIONS.with(|count| count.set(count.get() + 1));
            hash_shader_bytes(specialization.overrides.iter().flat_map(|(name, value)| {
                name.bytes().chain([0]).chain(value.to_bits().to_le_bytes())
            }))
        })
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

    /// Declares how far the shader WRITES outside its effect rect, in logical
    /// pixels. Backdrop compositing widens its scissor by this amount so
    /// SDF-driven coverage (rim glow, wobble, glued neighbor shapes) can
    /// extend past the node bounds instead of being clipped to them.
    pub fn set_output_padding(&mut self, padding: f32) {
        self.output_padding = if padding.is_finite() {
            padding.max(0.0)
        } else {
            0.0
        };
    }

    /// Returns the declared output padding in logical pixels.
    pub fn output_padding(&self) -> f32 {
        self.output_padding
    }

    /// Declares the rect outside which the shader writes nothing: every
    /// pixel its coverage can make nonzero at its current uniforms, the
    /// output padding's reach included, in logical pixels with the origin
    /// at the effect rect's top-left. A renderer composites only the part
    /// of the effect rect inside it; the capture it reads stays whole, so a
    /// node that carries headroom around a smaller material pays the
    /// composite for the material alone. It says nothing about sampling:
    /// see [`Self::set_sample_domain`]. `None`, the default, means the
    /// whole effect rect and its output padding. A rect with a non-finite
    /// side clears the declaration.
    pub fn set_output_support(&mut self, support: Option<Rect>) {
        self.set_domains(ShaderDomains {
            output_support: finite_rect(support),
            sample_domain: self.sample_domain(),
        });
    }

    /// The declared output support, when the shader gave one.
    pub fn output_support(&self) -> Option<Rect> {
        self.domains
            .as_ref()
            .and_then(|domains| domains.output_support)
    }

    fn set_domains(&mut self, domains: ShaderDomains) {
        self.domains = (domains != ShaderDomains::default()).then(|| Box::new(domains));
    }

    /// Declares the rect outside which the shader never samples its input,
    /// in logical pixels with the origin at the effect rect's top-left. A
    /// renderer may leave the input outside it unresolved: a blur feeding
    /// this shader need only write the domain. The default, `None`, is the
    /// whole effect rect and its input padding, which the input padding
    /// contract already promises; an output support says nothing about
    /// sampling, so a shader that shades a small region but reads a far
    /// one keeps the default. A rect with a non-finite side clears it.
    pub fn set_sample_domain(&mut self, domain: Option<Rect>) {
        self.set_domains(ShaderDomains {
            output_support: self.output_support(),
            sample_domain: finite_rect(domain),
        });
    }

    /// The declared sample domain, when the shader gave one.
    pub fn sample_domain(&self) -> Option<Rect> {
        self.domains
            .as_ref()
            .and_then(|domains| domains.sample_domain)
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

    /// Declares that the shader reads the reserved source region, mask and
    /// alpha slots and samples only within its region's texel centers, so the
    /// renderer may hand it an input region packed edge to edge beside others
    /// and draw it straight into the final pass with its clip applied.
    pub fn set_batched_source(&mut self, batched: bool) {
        self.batched_source = batched;
    }

    /// Whether the shader reads the reserved source region, mask and alpha
    /// slots.
    pub fn batched_source(&self) -> bool {
        self.batched_source
    }

    /// Declares that the shader returns zero wherever every texel it reads
    /// is zero. A layer that draws nothing under such a shader composites
    /// nothing, so the renderer leaves the page as it is instead of shading
    /// the layer's pixels to prove it.
    pub fn set_preserves_transparency(&mut self, preserves: bool) {
        self.preserves_transparency = preserves;
    }

    /// Whether the shader declared it returns zero over a transparent input.
    pub fn preserves_transparency(&self) -> bool {
        self.preserves_transparency
    }

    /// Declares the low-frequency copies of its source the shader reads
    /// through the reserved substrate region slots, in slot order. Only a
    /// batched shader packed with its stage is handed them; a shader
    /// without finds the slots zero and samples the source itself.
    ///
    /// # Panics
    ///
    /// When more than [`MAX_SUBSTRATES`] are declared.
    pub fn set_substrates(&mut self, substrates: &[SubstrateSpec]) {
        assert!(
            substrates.len() <= MAX_SUBSTRATES,
            "a runtime shader declares at most {MAX_SUBSTRATES} substrates"
        );
        if self.substrates().len() == substrates.len()
            && self
                .substrates()
                .iter()
                .zip(substrates)
                .all(|(existing, incoming)| existing.same_bits(incoming))
        {
            return;
        }
        self.specialization_mut().substrates = substrates.iter().copied().collect();
    }

    /// The substrates the shader declared, in slot order.
    pub fn substrates(&self) -> &[SubstrateSpec] {
        &self.specialization().substrates
    }

    /// Hashes the declared substrates and the draw split into `state`.
    pub fn hash_substrates<H: std::hash::Hasher>(&self, state: &mut H) {
        use std::hash::Hash;
        self.substrates().len().hash(state);
        for substrate in self.substrates() {
            substrate.hash_bits(state);
        }
        self.draw_split().hash(state);
    }

    /// Declares an `override NAME: i32` the renderer sets to 1 and 2 to draw
    /// the shader twice in the final pass, once for its interior and once
    /// for its rim, each pipeline compiled without the other's work and
    /// discarding the other's fragments before its fetches. Nothing else
    /// about the draw changes: the two draws partition the pixels the one
    /// draw shaded and land on the same bits.
    pub fn set_draw_split(&mut self, override_name: Option<&'static str>) {
        if self.draw_split() == override_name {
            return;
        }
        self.specialization_mut().draw_split = override_name;
    }

    /// The override selecting the interior or the rim draw, when declared.
    pub fn draw_split(&self) -> Option<&'static str> {
        self.specialization().draw_split
    }

    /// Declares that every override and the draw split of this shader are
    /// folds: a specialized pipeline lands on the same bytes as the general
    /// pipeline, which reads every folded value from its uniform. The
    /// renderer then compiles specializations in the background and draws
    /// with the general pipeline until they land. An override that selects
    /// a different picture, such as a pass switch, must leave this unset;
    /// its pipeline compiles inside the frame that first draws it.
    pub fn set_specialization_exact(&mut self, exact: bool) {
        if self.specialization_exact() == exact {
            return;
        }
        self.specialization_mut().exact = exact;
    }

    /// Whether the shader declared its specialization exact.
    pub fn specialization_exact(&self) -> bool {
        self.specialization().exact
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

#[cfg(test)]
thread_local! {
    static OVERRIDE_HASH_COMPUTATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl PartialEq for RuntimeShader {
    fn eq(&self, other: &Self) -> bool {
        self.source_hash == other.source_hash
            && (Arc::ptr_eq(&self.source, &other.source)
                || self.source.as_ref() == other.source.as_ref())
            && self.uniforms == other.uniforms
            && self.overrides().len() == other.overrides().len()
            && self
                .overrides()
                .iter()
                .zip(other.overrides())
                .all(|(a, b)| a.0 == b.0 && a.1.to_bits() == b.1.to_bits())
            && self.input_padding.to_bits() == other.input_padding.to_bits()
            && self.output_padding.to_bits() == other.output_padding.to_bits()
            && self.batched_source == other.batched_source
            && self.preserves_transparency == other.preserves_transparency
            && self.substrates() == other.substrates()
            && self.draw_split() == other.draw_split()
            && self.domains == other.domains
    }
}

fn hash_shader_source(source: &str) -> u64 {
    hash_shader_bytes(source.bytes())
}

fn hash_shader_bytes(bytes: impl IntoIterator<Item = u8>) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    bytes.into_iter().fold(FNV_OFFSET_BASIS, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
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
        .unwrap_or_else(PoisonError::into_inner);

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
        .unwrap_or_else(PoisonError::into_inner);

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

/// Where a runtime shader's pipeline draws, which decides how its output
/// blends: `Page` composites the shader over what lies beneath (a backdrop
/// effect, or a render effect the renderer draws straight onto the page),
/// `Layer` renders into the layer's own texture, whose content the shader
/// replaces (a render effect under a blend mode or clip the page draw cannot
/// apply, such as a `DstOut` mask).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ShaderTarget {
    Page,
    Layer,
}

/// A runtime shader to compile before its first draw, at the target it will
/// draw to, so a renderer's background compiler builds the pipeline at
/// start instead of inside the frame that first needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct ShaderWarmUp {
    pub shader: RuntimeShader,
    pub target: ShaderTarget,
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
    Shader {
        /// Shared shader configuration; use [`Arc::make_mut`] to edit a cloned effect independently.
        shader: Arc<RuntimeShader>,
    },
    /// Chain two effects: apply `first`, then apply `second` to the result.
    ///
    /// Child effects are shared; use [`Arc::make_mut`] to edit a cloned chain independently.
    Chain {
        first: Arc<RenderEffect>,
        second: Arc<RenderEffect>,
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
        Self::Shader {
            shader: Arc::new(shader),
        }
    }

    /// Chain this effect with another: `self` is applied first, then `other`.
    pub fn then(self, other: RenderEffect) -> Self {
        Self::Chain {
            first: Arc::new(self),
            second: Arc::new(other),
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

    /// Whether the effect returns zero over a transparent input: a blur or
    /// an offset of nothing is nothing, a shader when it declares so, and a
    /// chain when every step does.
    pub fn preserves_transparency(&self) -> bool {
        match self {
            RenderEffect::Blur { .. } | RenderEffect::Offset { .. } => true,
            RenderEffect::Shader { shader } => shader.preserves_transparency(),
            RenderEffect::Chain { first, second } => {
                first.preserves_transparency() && second.preserves_transparency()
            }
        }
    }

    /// Maximum logical-pixel input padding required by this effect.
    pub fn input_padding(&self) -> f32 {
        match self {
            RenderEffect::Blur {
                radius_x, radius_y, ..
            } => radius_x.abs().max(radius_y.abs()),
            RenderEffect::Offset { offset_x, offset_y } => offset_x.abs().max(offset_y.abs()),
            RenderEffect::Shader { shader } => shader.input_padding(),
            RenderEffect::Chain { first, second } => first.input_padding() + second.input_padding(),
        }
    }

    /// Maximum logical-pixel distance this effect WRITES outside its rect.
    /// Only runtime shaders may declare one (SDF coverage past node bounds);
    /// blur/offset stay confined to their tight rect.
    pub fn output_padding(&self) -> f32 {
        match self {
            RenderEffect::Blur { .. } | RenderEffect::Offset { .. } => 0.0,
            RenderEffect::Shader { shader } => shader.output_padding(),
            RenderEffect::Chain { first, second } => {
                first.output_padding() + second.output_padding()
            }
        }
    }

    /// The rect outside which this effect writes nothing, in its logical
    /// space with the origin at its rect's top-left, when the stage that
    /// produces its output declared one; blur and offset write their whole
    /// rect and declare none.
    pub fn output_support(&self) -> Option<Rect> {
        match self {
            RenderEffect::Blur { .. } | RenderEffect::Offset { .. } => None,
            RenderEffect::Shader { shader } => shader.output_support(),
            RenderEffect::Chain { second, .. } => second.output_support(),
        }
    }

    /// The rect outside which the stage that produces this effect's output
    /// never samples what it is given, when it declared one; the whole
    /// input otherwise. A blur samples everything it writes and more.
    pub fn sample_domain(&self) -> Option<Rect> {
        match self {
            RenderEffect::Blur { .. } | RenderEffect::Offset { .. } => None,
            RenderEffect::Shader { shader } => shader.sample_domain(),
            RenderEffect::Chain { second, .. } => second.sample_domain(),
        }
    }
}

#[cfg(test)]
#[path = "tests/render_effect_tests.rs"]
mod tests;
