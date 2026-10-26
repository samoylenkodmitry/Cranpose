use std::{borrow::Cow, hash::Hash};

use cranpose_ui_graphics::{
    BlendMode, FRAGMENT_KIND_ARC, FRAGMENT_KIND_FILL, FRAGMENT_KIND_LINE, FRAGMENT_KIND_STROKE,
    FRAGMENT_KIND_TRAPEZOID, RecordSegment,
};

use super::{
    DEPTH_FORMAT, PassDepth, ViewportUniformParams, blend_state_for_mode,
    create_render_pipeline_logged, supported_blend_mode,
};
use crate::{
    ablation::ShapeAblation,
    geometry::SegmentTransform,
    record_columns::record_vertex_layouts,
    run_store::RunBufferMode,
    shaders,
    shared_shader::{ShaderCode, SharedShader},
};

/// The shape shader's source for `mode`, built when its module is parsed.
pub(super) fn shape_shader_code(mode: RunBufferMode) -> ShaderCode {
    if mode.trig_fill {
        ShaderCode::wgsl(|| Cow::Owned(shaders::filling_shape_shader()))
    } else if mode.storage {
        ShaderCode::wgsl(|| Cow::Owned(shaders::storage_shape_shader()))
    } else {
        shaders::SHAPE
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum RunTier {
    Store = 0,
    Arena = 1,
    Either = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ShapeVariant {
    kinds: u8,
    brush: Option<u8>,
    solid: bool,
    /// How its records meet their placements' clips. A rounded clip gives
    /// every fragment its coverage, on the general entry points.
    clip: SegmentClip,
    interior: bool,
    dither: bool,
    ablation: ShapeAblation,
}

const FLAT_FILL_ENTRIES: [[(&str, &str); 2]; 2] = [
    [
        ("vs_record_plain_fill", "fs_plain_fill"),
        ("vs_record_dithered_fill", "fs_dithered_fill"),
    ],
    [
        ("vs_record_clipped_fill", "fs_clipped_fill"),
        ("vs_record_solid", "fs_solid"),
    ],
];

/// The entry points of a variant that draws slices of path fills alone and
/// flat: a solid brush, and a gradient between two stops from the vertices.
const SLICE_ENTRIES: [(&str, &str); 2] = [
    ("vs_record_slice", "fs_slice"),
    ("vs_record_gradient_slice", "fs_gradient_slice"),
];

const ALL_SHAPE_KINDS: u8 =
    (1 << FRAGMENT_KIND_FILL) | (1 << FRAGMENT_KIND_STROKE) | (1 << FRAGMENT_KIND_ARC) | LINE_KINDS;

/// The kinds the shader draws from line records: segments, and slices of
/// path fills. A variant compiles in the code of only those it draws.
pub(super) const LINE_KINDS: u8 = (1 << FRAGMENT_KIND_LINE) | TRAPEZOID_KIND;

const TRAPEZOID_KIND: u8 = 1 << FRAGMENT_KIND_TRAPEZOID;

fn variant_kinds(kinds: u8) -> u8 {
    if kinds.count_ones() <= 1 {
        kinds
    } else {
        (ALL_SHAPE_KINDS & !LINE_KINDS) | (kinds & LINE_KINDS)
    }
}

impl ShapeVariant {
    /// Every kind but slices of path fills, which only a segment holding
    /// them compiles in.
    const GENERAL: Self = Self {
        kinds: ALL_SHAPE_KINDS & !TRAPEZOID_KIND,
        brush: None,
        solid: false,
        clip: SegmentClip::Tested,
        interior: true,
        dither: false,
        ablation: ShapeAblation {
            material: false,
            fill: false,
        },
    };

    /// The variant `segment` draws with. `vertex_gradients` says a rect
    /// fill may take its gradient from its vertices here; a segment of
    /// slices of path fills may wherever it is drawn `flat`. Under a rounded
    /// clip the variant keeps the general entry points, which carry the
    /// clip's radius to the fragment, but compiles in only the segment's
    /// kinds and brush: the general program with every kind and brush is
    /// twice the code (Mali-G76: 108-115 ms to compile or link, the
    /// specialized ones 22-62).
    pub(crate) fn of_segment(
        segment: &RecordSegment,
        clip: SegmentClip,
        ablation: ShapeAblation,
        laid: bool,
        (flat, vertex_gradients): (bool, bool),
    ) -> Self {
        if !shape_variants_enabled() {
            return Self {
                kinds: Self::GENERAL.kinds | (segment.kinds & TRAPEZOID_KIND),
                ablation,
                ..Self::GENERAL
            }
            .clipped_by(clip);
        }
        let vertex_gradients = if segment.kinds == TRAPEZOID_KIND {
            flat
        } else {
            vertex_gradients
        };
        let gradient = segment.gradient || (segment.vertex_gradient && !vertex_gradients);
        Self {
            kinds: variant_kinds(segment.kinds),
            brush: gradient
                .then(|| segment.uniform_brush())
                .flatten()
                .map(|brush| brush as u8),
            solid: !gradient,
            clip,
            // A rounded clip's corners are not opaque: no interior path.
            interior: clip != SegmentClip::Rounded
                && gradient
                && segment.interiors
                && !(laid && segment.occluders && !segment.bare_interiors),
            dither: segment.vertex_gradient && vertex_gradients,
            ablation,
        }
    }

    /// The one kind the variant draws, as the shader's packed kind names
    /// it: a slice of a path fill is a line record.
    fn kind(self) -> Option<u8> {
        (self.kinds.count_ones() == 1).then(|| match self.kinds.trailing_zeros() {
            FRAGMENT_KIND_TRAPEZOID => FRAGMENT_KIND_LINE as u8,
            kind => kind as u8,
        })
    }

    /// The vertex and fragment entry points of this variant, for records
    /// drawn `flat` (none of them turned).
    fn entries(self, flat: bool) -> (&'static str, &'static str) {
        let fill = self.kind() == Some(FRAGMENT_KIND_FILL as u8);
        if self.rounded() {
            ("vs_record", "fs_main")
        } else if self.solid && flat && self.kinds == TRAPEZOID_KIND {
            SLICE_ENTRIES[usize::from(self.dither)]
        } else if self.solid && fill && flat {
            FLAT_FILL_ENTRIES[usize::from(self.clip == SegmentClip::Tested)]
                [usize::from(self.dither)]
        } else if self.solid {
            ("vs_record_solid", "fs_solid")
        } else if fill && !self.ablation.material {
            ("vs_record_gradient_fill", "fs_gradient_fill")
        } else {
            ("vs_record", "fs_main")
        }
    }

    fn general(self) -> Self {
        Self {
            kinds: (ALL_SHAPE_KINDS & !LINE_KINDS) | (self.kinds & LINE_KINDS),
            ablation: self.ablation,
            ..Self::GENERAL
        }
        .clipped_by(self.clip)
    }

    /// The general variant with every kind, lines and path slices too.
    fn universal(self) -> Self {
        Self {
            kinds: ALL_SHAPE_KINDS,
            ablation: self.ablation,
            ..Self::GENERAL
        }
        .clipped_by(self.clip)
    }

    /// The variant, keeping the rounded clip `clip` names: the general
    /// variant tests every rect clip, but no fallback may drop a rounding.
    fn clipped_by(self, clip: SegmentClip) -> Self {
        if clip == SegmentClip::Rounded {
            Self { clip, ..self }
        } else {
            self
        }
    }

    fn rounded(self) -> bool {
        self.clip == SegmentClip::Rounded
    }

    fn joined(self) -> Self {
        if self.solid {
            Self {
                dither: true,
                ..self
            }
        } else {
            Self {
                interior: true,
                ..self
            }
        }
    }
}

/// How a segment's draw meets its placement's clip: a scissor or no clip
/// leaves the shader nothing to test, a rect clip off the scissor's grid is
/// tested per fragment, and a rounded clip gives every fragment coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SegmentClip {
    Untested,
    Tested,
    Rounded,
}

impl SegmentClip {
    /// The clip a draw under `placement` tests, given whether its rect clip
    /// became the draw's scissor.
    pub(crate) fn of(placement: &crate::scene::Placement, scissored: bool) -> Self {
        if placement.clip_rounded() {
            Self::Rounded
        } else if placement.clip.is_some() && !scissored {
            Self::Tested
        } else {
            Self::Untested
        }
    }
}

fn shape_variants_enabled() -> bool {
    !cranpose_core::DIAGNOSTICS.shape_variants.equals("0")
}

/// How a pipeline meets its pass's depth buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ShapeDepth {
    /// The pass has no depth buffer.
    Off,
    /// Paints what the opaque interiors of later records leave visible.
    Tested,
    /// Lays down opaque interiors, front to back, ahead of the paint.
    Interior,
}

impl ShapeDepth {
    pub(super) fn stencil_state(self) -> Option<wgpu::DepthStencilState> {
        let (depth_write_enabled, depth_compare) = match self {
            Self::Off => return None,
            Self::Tested => (false, wgpu::CompareFunction::Less),
            Self::Interior => (true, wgpu::CompareFunction::Less),
        };
        Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(depth_write_enabled),
            depth_compare: Some(depth_compare),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        })
    }
}

/// Which records a shape pipeline draws turned, as `SHAPE_TURNS` in the
/// shape shader: none, every one, or each as its placement says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ShapeTurns {
    None,
    All,
    Mixed,
}

impl ShapeTurns {
    /// The turns of records placed under `turn`, in a pass whose flat and
    /// turned records alternate often when `mixed`.
    pub(crate) fn of(turn: SegmentTransform, mixed: bool) -> Self {
        if mixed {
            Self::Mixed
        } else if turn.is_identity() {
            Self::None
        } else {
            Self::All
        }
    }

    fn constant(self) -> f64 {
        match self {
            Self::None => 0.0,
            Self::All => 1.0,
            Self::Mixed => 2.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ShapePipelineKey {
    pub(crate) blend_mode: BlendMode,
    pub(crate) tier: RunTier,
    pub(crate) variant: ShapeVariant,
    pub(crate) turns: ShapeTurns,
    pub(crate) depth: ShapeDepth,
}

/// Packs a key's fields into a number, low bit first.
#[derive(Default)]
struct KeyWriter {
    bits: u64,
    width: u32,
}

impl KeyWriter {
    fn put(&mut self, value: u64, width: u32) {
        self.bits |= value << self.width;
        self.width += width;
    }

    fn put_byte(&mut self, value: Option<u8>) {
        self.put(u64::from(value.is_some()), 1);
        self.put(u64::from(value.unwrap_or_default()), 8);
    }
}

/// Unpacks what [`KeyWriter`] packed, low bit first.
/// The bits a key's shape kinds take: one per fragment kind.
const KIND_BITS: u32 = FRAGMENT_KIND_TRAPEZOID + 1;

struct KeyReader(u64);

impl KeyReader {
    fn take(&mut self, width: u32) -> u64 {
        let value = self.0 & ((1 << width) - 1);
        self.0 >>= width;
        value
    }

    fn take_flag(&mut self) -> bool {
        self.take(1) == 1
    }

    fn take_byte(&mut self) -> Option<u8> {
        let present = self.take_flag();
        let value = u8::try_from(self.take(8)).ok()?;
        present.then_some(value)
    }

    fn take_kinds(&mut self) -> Option<u8> {
        u8::try_from(self.take(KIND_BITS))
            .ok()
            .filter(|kinds| *kinds != 0)
    }
}

impl ShapePipelineKey {
    pub(crate) const DISK_LAYOUT: [u8; 8] = *b"CPKEY003";

    /// The flat solid rect fill of a pass without a depth buffer, which
    /// every screen draws: on all 38 demo pages, and in a launch's first
    /// frames, which draw without the depth pre-pass.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) const SOLID_RECT: Self = Self {
        blend_mode: BlendMode::SrcOver,
        tier: RunTier::Arena,
        variant: ShapeVariant {
            kinds: 1 << FRAGMENT_KIND_FILL,
            brush: None,
            solid: true,
            clip: SegmentClip::Untested,
            interior: false,
            dither: false,
            ablation: ShapeAblation {
                material: false,
                fill: false,
            },
        },
        turns: ShapeTurns::None,
        depth: ShapeDepth::Off,
    };

    /// The key as a number a later launch reads back with
    /// [`Self::from_bits`], to build the pipeline ahead of its first frame.
    pub(crate) fn to_bits(self) -> u64 {
        let mut bits = KeyWriter::default();
        bits.put(self.blend_mode as u64, 5);
        bits.put(self.tier as u64, 2);
        let variant = self.variant;
        bits.put(u64::from(variant.kinds), KIND_BITS);
        bits.put_byte(variant.brush);
        bits.put(u64::from(variant.solid), 1);
        bits.put(
            match variant.clip {
                SegmentClip::Untested => 0,
                SegmentClip::Tested => 1,
                SegmentClip::Rounded => 2,
            },
            2,
        );
        bits.put(u64::from(variant.interior), 1);
        bits.put(u64::from(variant.dither), 1);
        bits.put(u64::from(variant.ablation.material), 1);
        bits.put(u64::from(variant.ablation.fill), 1);
        bits.put(
            match self.turns {
                ShapeTurns::None => 0,
                ShapeTurns::All => 1,
                ShapeTurns::Mixed => 2,
            },
            2,
        );
        bits.put(
            match self.depth {
                ShapeDepth::Off => 0,
                ShapeDepth::Tested => 1,
                ShapeDepth::Interior => 2,
            },
            2,
        );
        bits.bits
    }

    /// The key [`Self::to_bits`] packed, or `None` for a number no key
    /// packs to.
    pub(crate) fn from_bits(packed: u64) -> Option<Self> {
        let mut bits = KeyReader(packed);
        let blend_mode = *BlendMode::ALL.get(usize::try_from(bits.take(5)).ok()?)?;
        let tier = match bits.take(2) {
            0 => RunTier::Store,
            1 => RunTier::Arena,
            2 => RunTier::Either,
            _ => return None,
        };
        let kinds = bits.take_kinds()?;
        let brush = bits.take_byte();
        let solid = bits.take_flag();
        let clip = match bits.take(2) {
            0 => SegmentClip::Untested,
            1 => SegmentClip::Tested,
            2 => SegmentClip::Rounded,
            _ => return None,
        };
        let interior = bits.take_flag();
        let dither = bits.take_flag();
        let ablation = ShapeAblation {
            material: bits.take_flag(),
            fill: bits.take_flag(),
        };
        let turns = match bits.take(2) {
            0 => ShapeTurns::None,
            1 => ShapeTurns::All,
            2 => ShapeTurns::Mixed,
            _ => return None,
        };
        let depth = match bits.take(2) {
            0 => ShapeDepth::Off,
            1 => ShapeDepth::Tested,
            2 => ShapeDepth::Interior,
            _ => return None,
        };
        (bits.0 == 0).then_some(Self {
            blend_mode,
            tier,
            variant: ShapeVariant {
                kinds,
                brush,
                solid,
                clip,
                interior,
                dither,
                ablation,
            },
            turns,
            depth,
        })
    }

    #[cfg(test)]
    pub(crate) fn general_for(blend_mode: BlendMode, tier: RunTier) -> Self {
        Self {
            blend_mode,
            tier,
            variant: ShapeVariant::GENERAL,
            turns: ShapeTurns::None,
            depth: ShapeDepth::Off,
        }
    }

    /// The pipeline that lays down the opaque interiors of this key's
    /// draws, if they have any: plain source-over draws. A turned record's
    /// layer draws in place only under a rigid turn, which keeps the
    /// interior's half-pixel inset from the fill's edge exact. The interior
    /// shader reads only the tier and the turns, so every variant shares one
    /// pipeline per tier and turns: one compile, and draws of different
    /// variants lay their interiors down together.
    pub(crate) fn interior(self) -> Option<Self> {
        (self.depth == ShapeDepth::Tested
            && !self.variant.rounded()
            && self.blend_mode == BlendMode::SrcOver
            && !self.variant.ablation.material
            && !self.variant.ablation.fill)
            .then_some(Self {
                variant: ShapeVariant::GENERAL,
                depth: ShapeDepth::Interior,
                ..self
            })
    }

    pub(crate) fn general(self) -> Self {
        Self {
            tier: RunTier::Either,
            variant: self.variant.general(),
            ..self
        }
    }

    /// The one pipeline that draws every key with this one's blend, depth
    /// and clip rounding: every kind, records turned or not, from either
    /// tier.
    pub(crate) fn universal(self) -> Self {
        Self {
            tier: RunTier::Either,
            variant: self.variant.universal(),
            turns: ShapeTurns::Mixed,
            ..self
        }
    }

    pub(crate) fn is_general(self) -> bool {
        self.variant == self.variant.general()
    }

    pub(crate) fn joined(self) -> Self {
        Self {
            variant: self.variant.joined(),
            ..self
        }
    }

    pub(crate) fn program(self, mode: RunBufferMode) -> ShapeProgram {
        let Self {
            tier,
            variant,
            turns,
            depth,
            ..
        } = self;
        let constants = [
            ("SHAPE_KIND_FIXED", variant.kind().map_or(-1.0, f64::from)),
            ("SHAPE_KINDS", f64::from(variant.kinds)),
            ("BRUSH_KIND_FIXED", variant.brush.map_or(-1.0, f64::from)),
            ("SHAPE_SOLID", f64::from(u8::from(variant.solid))),
            (
                "SHAPE_CLIPPED",
                f64::from(u8::from(variant.clip != SegmentClip::Untested)),
            ),
            ("SHAPE_ROUNDED_CLIP", f64::from(u8::from(variant.rounded()))),
            (
                "SHAPE_INTERIOR",
                f64::from(u8::from(variant.interior && !variant.rounded())),
            ),
            ("SHAPE_DITHER", f64::from(u8::from(variant.dither))),
            ("SHAPE_TURNS", turns.constant()),
            ("SHAPE_TIER", f64::from(tier as u8)),
            ("SHAPE_BANDS", f64::from(u8::from(mode.storage))),
            ("SHAPE_TRIG_FILLED", f64::from(u8::from(mode.trig_fill))),
            ("SHAPE_FLAT", f64::from(u8::from(variant.ablation.material))),
            ("SHAPE_DISCARD", f64::from(u8::from(variant.ablation.fill))),
        ];
        let (vertex_entry, fragment_entry) = if depth == ShapeDepth::Interior {
            ("vs_record_interior", "fs_interior")
        } else {
            variant.entries(turns == ShapeTurns::None)
        };
        ShapeProgram {
            vertex_entry,
            fragment_entry,
            constants,
        }
    }

    /// The pipeline `segment` of a run at `placement` draws with. `clipped`
    /// says the variant tests the run's clip: a turned run's, since an
    /// unturned one's clip is its paint's scissor (see [`super::clip_scissor`]).
    pub(crate) fn of_run(
        segment: &RecordSegment,
        placement: &crate::scene::Placement,
        tier: RunTier,
        ablation: ShapeAblation,
        turns: ShapeTurns,
        (depth, clip): (PassDepth, SegmentClip),
        viewport: ViewportUniformParams,
    ) -> Self {
        let blend_mode = supported_blend_mode(segment.blend);
        let laid = depth.is_on()
            && blend_mode == BlendMode::SrcOver
            && !placement.paints()
            && clip != SegmentClip::Rounded
            && !ablation.material
            && !ablation.fill;
        Self {
            blend_mode,
            tier,
            variant: ShapeVariant::of_segment(
                segment,
                clip,
                ablation,
                laid,
                (
                    turns == ShapeTurns::None,
                    turns == ShapeTurns::None
                        && placement.snap_anchor.is_some()
                        && viewport.offset.iter().all(|value| value.fract() == 0.0),
                ),
            ),
            turns,
            depth: if depth.is_on() {
                ShapeDepth::Tested
            } else {
                ShapeDepth::Off
            },
        }
    }
}

/// The entry points and pipeline constants of the shape program a key names.
pub(crate) struct ShapeProgram {
    pub(crate) vertex_entry: &'static str,
    pub(crate) fragment_entry: &'static str,
    pub(crate) constants: [(&'static str, f64); 14],
}

pub(crate) fn create_shape_pipeline(
    device: &wgpu::Device,
    cache: Option<&wgpu::PipelineCache>,
    surface_format: wgpu::TextureFormat,
    shader: &SharedShader,
    key: ShapePipelineKey,
    mode: RunBufferMode,
) -> wgpu::RenderPipeline {
    let ShapePipelineKey {
        blend_mode,
        tier,
        variant,
        turns,
        depth,
    } = key;
    let ShapeProgram {
        vertex_entry,
        fragment_entry,
        constants,
    } = key.program(mode);
    let blend = (depth != ShapeDepth::Interior).then(|| blend_state_for_mode(blend_mode));
    let instance_layout = record_vertex_layouts().map(Some);
    let stages = shader.stages((vertex_entry, fragment_entry), &constants);

    create_render_pipeline_logged(
        device,
        cache,
        &format!(
            "shape blend={blend_mode:?} tier={tier:?} variant={variant:?} turns={turns:?} depth={depth:?}"
        ),
        wgpu::RenderPipelineDescriptor {
            label: Some("Shape Pipeline"),
            layout: Some(shader.layout()),
            vertex: wgpu::VertexState {
                module: &stages.vertex,
                entry_point: Some(vertex_entry),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: stages.constants,
                    ..wgpu::PipelineCompilationOptions::default()
                },
                buffers: &instance_layout,
            },
            fragment: Some(wgpu::FragmentState {
                module: &stages.fragment,
                entry_point: Some(fragment_entry),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: stages.constants,
                    ..wgpu::PipelineCompilationOptions::default()
                },
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: depth.stencil_state(),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        },
    )
}

#[cfg(test)]
#[path = "tests/shape_variant_tests.rs"]
mod tests;
