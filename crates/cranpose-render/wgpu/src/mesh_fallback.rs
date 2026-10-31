//! Shape draws whose pipeline has not compiled yet, drawn as triangles.
//!
//! A shape pipeline takes 30 to 150 ms to compile on a Mali-G76, and the
//! first screen of an app a device never ran needs about a dozen of them,
//! so that screen would wait about 250 ms for them. Until a draw's own
//! pipeline lands, its records are laid out on the CPU as triangles, every
//! edge feathered over a pixel for its antialiasing, and drawn through one
//! pipeline of position and colour, which the renderer builds as it starts.
//! The triangles follow what the shape stage draws: fills and strokes of
//! rects and rounded rects, lines, slices of path fills and arc bands, under
//! their placement's offset, scale, turn, clip rect and paint, with a
//! gradient taken at each vertex. A rounded clip clips as its rect.

use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use cranpose_ui_graphics::{
    BRUSH_KIND_LINEAR, BRUSH_KIND_RADIAL, BRUSH_KIND_SWEEP, BrushRecord, FRAGMENT_KIND_ARC,
    FRAGMENT_KIND_FILL, FRAGMENT_KIND_LINE, FRAGMENT_KIND_STROKE, FRAGMENT_KIND_TRAPEZOID,
    GradientStopRecord, ShapeRecordBody, ShapeRecordCurve, whole,
};

use crate::frame_graph::BufferUpload;

/// One corner of a fallback triangle: its clip position and depth, and its
/// premultiplied colour in the 8 bits a channel of the target keeps. A
/// launch's first frames draw every shape this way, and the gauntlet's
/// largest pass at tier 12 laid out 186,000 corners: the frame upload rings
/// grew to hold them, and a Mali driver counts such a peak for the rest of a
/// run. At 16 bytes a corner instead of 32, with the triangles' storage freed
/// once a frame draws none, a first start of the gauntlet on a Mate 20 X
/// held 168 MB instead of 176 (eight launches each; second starts, which
/// draw no fallback, unchanged).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct MeshVertex {
    position: [f32; 3],
    color: [u8; 4],
}

/// The fallback pipeline. The web's frames build their shape pipelines, or
/// stand one in, and its build leaves this out.
#[cfg(not(target_arch = "wasm32"))]
mod pipeline {
    use std::sync::Arc;

    use super::MeshVertex;
    use crate::{fixed_pipeline::FixedPipelineJob, render::PassDepth};

    const MESH_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4];

    const MESH_WGSL: &str = r"
struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

struct Shaded {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_mesh(vertex: Vertex) -> Shaded {
    return Shaded(vec4<f32>(vertex.position, 1.0), vertex.color);
}

@fragment
fn fs_mesh(shaded: Shaded) -> @location(0) vec4<f32> {
    return shaded.color;
}
";

    /// The creation of the fallback pipeline for targets of `format`, in a
    /// pass with the depth pre-pass's buffer when `depth`.
    pub(crate) fn mesh_pipeline_job(
        device: &Arc<wgpu::Device>,
        cache: Option<wgpu::PipelineCache>,
        format: wgpu::TextureFormat,
        depth: PassDepth,
    ) -> FixedPipelineJob {
        let device = Arc::clone(device);
        Box::new(move || {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Mesh Fallback Shader"),
                source: wgpu::ShaderSource::Wgsl(MESH_WGSL.into()),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Mesh Fallback Layout"),
                bind_group_layouts: &[],
                immediate_size: 0,
            });
            let tag = match depth {
                PassDepth::On => "mesh/fallback/depth",
                PassDepth::Off => "mesh/fallback",
            };
            crate::render::create_render_pipeline_logged(
                &device,
                cache.as_ref(),
                tag,
                wgpu::RenderPipelineDescriptor {
                    label: Some("Mesh Fallback Pipeline"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vs_mesh"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: size_of::<MeshVertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &MESH_ATTRIBUTES,
                        })],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some("fs_mesh"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleStrip,
                        strip_index_format: Some(wgpu::IndexFormat::Uint16),
                        ..wgpu::PrimitiveState::default()
                    },
                    depth_stencil: depth.overlay_state(),
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                },
            )
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use pipeline::mesh_pipeline_job;

/// A record's layer alpha and colour filter, applied as the shape stage's
/// `paint` applies them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MeshPaint {
    pub(crate) alpha: f32,
    pub(crate) filter: Option<([[f32; 4]; 4], [f32; 4])>,
}

/// Where a placement puts its records' device pixels, and how it paints
/// them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MeshPlacement {
    pub(crate) offset: [f32; 2],
    pub(crate) scale: f32,
    pub(crate) canonicalize: bool,
    /// The device clip as x, y, width and height.
    pub(crate) clip: Option<[f32; 4]>,
    /// The turn (row-major 2x2) and translation into the target's space.
    pub(crate) turn: Option<([f32; 4], [f32; 2])>,
    pub(crate) paint: Option<MeshPaint>,
}

/// The target a draw lands in: where its device space starts, its size,
/// and the pass order its records' depth counts from in a pass with the
/// depth pre-pass's buffer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MeshTarget {
    pub(crate) offset: [f32; 2],
    pub(crate) size: [f32; 2],
    pub(crate) depth_base: Option<f32>,
}

/// The tables a draw's records index into.
#[derive(Clone, Copy)]
pub(crate) struct MeshRecords<'a> {
    pub(crate) bodies: &'a [ShapeRecordBody],
    pub(crate) curves: &'a [ShapeRecordCurve],
    pub(crate) brushes: &'a [BrushRecord],
    pub(crate) stops: &'a [GradientStopRecord],
}

/// The index that ends a triangle strip; the next index starts another.
const RESTART: u16 = u16::MAX;

/// The vertices one run of indices reaches past its first: those a 16-bit
/// index other than [`RESTART`] names.
const CHUNK_VERTICES: usize = RESTART as usize;

/// A run of a pass's indices that count from one of its vertices: the
/// vertex buffer binds from that vertex while they draw.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MeshChunk {
    first_index: u32,
    first_vertex: u32,
}

/// The fallback triangles of one pass, as triangle strips of 16-bit
/// indices, each strip ended by [`RESTART`]: a band between two rings takes
/// two indices a step and a convex ring one a corner, where separate
/// triangles of 32-bit indices took 24 and 12 bytes. A sharp corner takes
/// one point. The gauntlet's first frames at tier 12 drew up to 135,000
/// corners in one pass, uploaded each frame while the shape pipelines
/// compile; with strips a first start on a Mate 20 X held 166.7 MB instead
/// of 171.5 (four launches each).
#[derive(Default)]
pub(crate) struct MeshBuilder {
    vertices: Vec<MeshVertex>,
    indices: Vec<u16>,
    chunks: Vec<MeshChunk>,
    /// Corners waiting to be clipped, three a triangle.
    clipped: Vec<Corner>,
    /// The rings of the record being laid out.
    points: Vec<Point>,
    rings: Vec<RingSpan>,
}

/// A ring among a record's points, all at one coverage.
#[derive(Clone, Copy, Debug)]
struct RingSpan {
    start: usize,
    end: usize,
    coverage: f32,
}

impl RingSpan {
    fn len(&self) -> usize {
        self.end - self.start
    }
}

/// How a record's rings make triangles: a ring's convex inside, or the band
/// between two rings of as many points, closed or open.
#[derive(Clone, Copy, Debug)]
enum Join {
    Fan(usize),
    Strip(usize, usize, bool),
}

impl Join {
    /// Calls `emit` with the join's triangle strip, as indices into the
    /// record's points: a convex ring zigzags from its first corner across
    /// to its last, a band alternates between its rings.
    fn strip(self, rings: &[RingSpan], mut emit: impl FnMut(usize)) {
        match self {
            Self::Fan(index) => {
                let ring = &rings[index];
                zigzag(ring.len(), |corner| emit(ring.start + corner));
            }
            Self::Strip(a, b, closed) => {
                let (a, b) = (&rings[a], &rings[b]);
                let count = a.len().min(b.len());
                let steps = if closed && count > 0 {
                    count + 1
                } else {
                    count
                };
                for step in 0..steps {
                    emit(a.start + step % count);
                    emit(b.start + step % count);
                }
            }
        }
    }

    /// Calls `emit` with each triangle of the strip [`Self::strip`] gives.
    fn triangles(self, rings: &[RingSpan], mut emit: impl FnMut([usize; 3])) {
        let mut last = [0usize; 2];
        let mut seen = 0usize;
        self.strip(rings, |point| {
            if seen >= 2 {
                emit([last[0], last[1], point]);
            }
            last = [last[1], point];
            seen += 1;
        });
    }
}

/// Calls `emit` with the corners of a convex polygon of `count` corners in
/// the order of a triangle strip that covers it: from the first, across to
/// the last, and on, inward from both ends.
fn zigzag(count: usize, mut emit: impl FnMut(usize)) {
    let (mut low, mut high) = (0, count);
    while low < high {
        emit(low);
        low += 1;
        if low < high {
            high -= 1;
            emit(high);
        }
    }
}

/// Where a record's points lie against a clip rect.
enum ClipRelation {
    Inside,
    Crossing,
    Outside,
}

fn clip_relation(points: &[Point], clip: [f32; 4]) -> ClipRelation {
    let (right, bottom) = (clip[0] + clip[2], clip[1] + clip[3]);
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for at in points {
        bounds = [
            bounds[0].min(at[0]),
            bounds[1].min(at[1]),
            bounds[2].max(at[0]),
            bounds[3].max(at[1]),
        ];
    }
    if bounds[0] >= clip[0] && bounds[1] >= clip[1] && bounds[2] <= right && bounds[3] <= bottom {
        ClipRelation::Inside
    } else if bounds[2] < clip[0] || bounds[3] < clip[1] || bounds[0] > right || bounds[1] > bottom
    {
        ClipRelation::Outside
    } else {
        ClipRelation::Crossing
    }
}

impl MeshBuilder {
    pub(crate) fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.chunks.clear();
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub(crate) fn vertices(&self) -> &[MeshVertex] {
        &self.vertices
    }

    /// The indices, an even number of them, so their bytes fill whole
    /// words.
    pub(crate) fn indices(&self) -> &[u16] {
        &self.indices
    }

    pub(crate) fn chunks(&self) -> &[MeshChunk] {
        &self.chunks
    }

    /// Readies the indices of `count` more vertices: in the open chunk
    /// while its indices reach them, else in a new chunk from the next
    /// vertex.
    fn reserve(&mut self, count: usize) {
        let first = self.chunks.last().map(|chunk| chunk.first_vertex as usize);
        if first.is_none_or(|first| self.vertices.len() + count - first > CHUNK_VERTICES) {
            self.chunks.push(MeshChunk {
                first_index: self.indices.len() as u32,
                first_vertex: self.vertices.len() as u32,
            });
        }
    }

    /// The index of the next vertex in the open chunk.
    fn next_index(&self) -> u16 {
        let first = self.chunks.last().map_or(0, |chunk| chunk.first_vertex);
        (self.vertices.len() as u32 - first) as u16
    }

    /// Appends the triangles of `records` from `tables`, each placed by
    /// `placement`, and returns the range of indices that draws them.
    pub(crate) fn push_records(
        &mut self,
        tables: MeshRecords<'_>,
        records: Range<u32>,
        placement: impl Fn(&ShapeRecordBody) -> Option<MeshPlacement>,
        target: MeshTarget,
    ) -> Range<u32> {
        let start = self.indices.len() as u32;
        for index in records {
            let (Some(body), Some(curve)) = (
                tables.bodies.get(index as usize),
                tables.curves.get(index as usize),
            ) else {
                continue;
            };
            let Some(placement) = placement(body) else {
                continue;
            };
            let depth = target.depth_base.map_or(0.0, |base| {
                (1.0 - (base + index as f32 + 1.0) * DEPTH_STEP).max(0.0)
            });
            let shade = Shade::of(body, curve, &placement, tables);
            let mut record = Record {
                builder: self,
                shade,
                placement,
                target,
                depth,
            };
            record.push(body, curve);
        }
        if self.indices.len() % 2 == 1 {
            self.indices.push(RESTART);
        }
        start..self.indices.len() as u32
    }
}

/// The shape stage's step between two records' depths.
const DEPTH_STEP: f32 = 1.0 / 1_048_576.0;
const INFINITE_GRADIENT_POINT: f32 = 1.0e30;

type Point = [f32; 2];

/// A ring point with the share of the pixel the shape covers there.
#[derive(Clone, Copy, Debug)]
struct Corner {
    at: Point,
    coverage: f32,
}

/// How a record colours a point: one colour, or a gradient.
enum Shade<'a> {
    Solid([f32; 4]),
    Gradient {
        kind: u32,
        /// Linear: start and end. Radial: centre and radius in `[2]`.
        /// Sweep: centre.
        points: [f32; 4],
        stops: &'a [GradientStopRecord],
    },
}

impl<'a> Shade<'a> {
    fn of(
        body: &ShapeRecordBody,
        curve: &ShapeRecordCurve,
        placement: &MeshPlacement,
        tables: MeshRecords<'a>,
    ) -> Self {
        let Some(brush) = body
            .brush
            .checked_sub(1)
            .and_then(|index| tables.brushes.get(index as usize))
        else {
            return Self::Solid(body.color);
        };
        let start = brush.stop_start as usize;
        let stops = tables
            .stops
            .get(start..start + brush.stop_count as usize)
            .unwrap_or_default();
        let scale = placement.scale;
        let rect =
            if cranpose_ui_graphics::record_fragment_kind(body.flags) == FRAGMENT_KIND_TRAPEZOID {
                // A slice resolves its brush against the fill's rect, which it
                // carries in its radii.
                [
                    (curve.radii[0] + placement.offset[0]) * scale,
                    (curve.radii[1] + placement.offset[1]) * scale,
                    curve.radii[2] * scale,
                    curve.radii[3] * scale,
                ]
            } else {
                record_edges(body, placement)
            };
        let resolve = |value: f32, origin: f32, extent: f32| {
            let point = if value.abs() < INFINITE_GRADIENT_POINT {
                origin + value
            } else if value > 0.0 {
                origin + extent
            } else {
                origin
            };
            canonical(point, placement.canonicalize)
        };
        let params = brush.params.map(|value| value * scale);
        let points = match brush.kind {
            BRUSH_KIND_LINEAR => [
                resolve(params[0], rect[0], rect[2]),
                resolve(params[1], rect[1], rect[3]),
                resolve(params[2], rect[0], rect[2]),
                resolve(params[3], rect[1], rect[3]),
            ],
            BRUSH_KIND_RADIAL => [
                resolve(params[0], rect[0], rect[2]),
                resolve(params[1], rect[1], rect[3]),
                params[2],
                0.0,
            ],
            _ => [
                resolve(params[0], rect[0], rect[2]),
                resolve(params[1], rect[1], rect[3]),
                0.0,
                0.0,
            ],
        };
        Self::Gradient {
            kind: brush.kind,
            points,
            stops,
        }
    }

    fn at(&self, point: Point) -> [f32; 4] {
        match self {
            Self::Solid(color) => *color,
            Self::Gradient {
                kind,
                points,
                stops,
            } => {
                let t = match *kind {
                    BRUSH_KIND_LINEAR => {
                        let axis = [points[2] - points[0], points[3] - points[1]];
                        let length = axis[0] * axis[0] + axis[1] * axis[1];
                        if length > 0.0 {
                            ((point[0] - points[0]) * axis[0] + (point[1] - points[1]) * axis[1])
                                / length
                        } else {
                            0.0
                        }
                    }
                    BRUSH_KIND_RADIAL => {
                        let distance = (point[0] - points[0]).hypot(point[1] - points[1]);
                        if points[2] > 0.0 {
                            distance / points[2]
                        } else {
                            0.0
                        }
                    }
                    BRUSH_KIND_SWEEP => {
                        let angle = (point[1] - points[1]).atan2(point[0] - points[0]);
                        angle.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU
                    }
                    _ => 0.0,
                };
                stop_color(stops, t)
            }
        }
    }
}

/// The stops' colour at `t`, clamped to the first and last.
fn stop_color(stops: &[GradientStopRecord], t: f32) -> [f32; 4] {
    let Some(first) = stops.first() else {
        return [0.0; 4];
    };
    let t = t.clamp(0.0, 1.0);
    if t <= first.position[0] {
        return first.color;
    }
    for pair in stops.windows(2) {
        let (from, to) = (&pair[0], &pair[1]);
        if t <= to.position[0] {
            let span = to.position[0] - from.position[0];
            let along = if span > 0.0 {
                (t - from.position[0]) / span
            } else {
                1.0
            };
            return std::array::from_fn(|channel| {
                from.color[channel] + (to.color[channel] - from.color[channel]) * along
            });
        }
    }
    stops.last().map_or([0.0; 4], |last| last.color)
}

/// `value` on the 1/16 device pixel grid when the placement snaps.
fn canonical(value: f32, canonicalize: bool) -> f32 {
    if canonicalize {
        value.signum() * whole::floor(value.abs() * 16.0 + 0.5) / 16.0
    } else {
        value
    }
}

/// The device rect a fill or stroke rasterizes, as x, y, width and height:
/// a stroke's outer half included.
fn record_edges(body: &ShapeRecordBody, placement: &MeshPlacement) -> [f32; 4] {
    let kind = cranpose_ui_graphics::record_fragment_kind(body.flags);
    let half = if kind == FRAGMENT_KIND_STROKE {
        body.stroke_width * 0.5
    } else {
        0.0
    };
    let scale = placement.scale;
    let left = canonical(
        (body.rect[0] - half + placement.offset[0]) * scale,
        placement.canonicalize,
    );
    let top = canonical(
        (body.rect[1] - half + placement.offset[1]) * scale,
        placement.canonicalize,
    );
    let right = canonical(
        (body.rect[0] + body.rect[2] + half + placement.offset[0]) * scale,
        placement.canonicalize,
    );
    let bottom = canonical(
        (body.rect[1] + body.rect[3] + half + placement.offset[1]) * scale,
        placement.canonicalize,
    );
    [left, top, right - left, bottom - top]
}

/// One record being laid out.
struct Record<'b, 's> {
    builder: &'b mut MeshBuilder,
    shade: Shade<'s>,
    placement: MeshPlacement,
    target: MeshTarget,
    depth: f32,
}

impl Record<'_, '_> {
    fn push(&mut self, body: &ShapeRecordBody, curve: &ShapeRecordCurve) {
        match cranpose_ui_graphics::record_fragment_kind(body.flags) {
            FRAGMENT_KIND_FILL => self.fill(body, curve),
            FRAGMENT_KIND_STROKE => self.stroke(body, curve),
            FRAGMENT_KIND_LINE => self.line(body, curve),
            FRAGMENT_KIND_TRAPEZOID => self.trapezoid(body, curve),
            FRAGMENT_KIND_ARC => self.arc(body, curve),
            _ => {}
        }
    }

    /// The corner radii in device pixels, top-left, top-right,
    /// bottom-right, bottom-left, held to half the shorter side.
    fn radii(&self, body: &ShapeRecordBody, curve: &ShapeRecordCurve) -> [f32; 4] {
        let limit = (body.rect[2].min(body.rect[3]) * 0.5).max(0.0);
        curve
            .radii
            .map(|radius| radius.clamp(0.0, limit) * self.placement.scale)
    }

    fn fill(&mut self, body: &ShapeRecordBody, curve: &ShapeRecordCurve) {
        let rect = record_edges(body, &self.placement);
        let radii = self.radii(body, curve);
        let steps = corner_steps(&radii);
        let (inset, coverage) = thin_inset(rect);
        let core = self.ring(coverage, |points| {
            rounded_ring(points, rect, radii, -inset, steps);
        });
        let outer = self.ring(0.0, |points| {
            rounded_ring(points, rect, radii, 0.5, steps);
        });
        self.commit(&[Join::Fan(core), Join::Strip(core, outer, true)]);
    }

    fn stroke(&mut self, body: &ShapeRecordBody, curve: &ShapeRecordCurve) {
        // The rect reaches the stroke's outer edge; its inner edge lies a
        // stroke width inside, and the outline half a width inside.
        let rect = record_edges(body, &self.placement);
        let width = (body.stroke_width * self.placement.scale).max(0.0);
        let half = width * 0.5;
        let outline = self.radii(body, curve);
        let outer_radii = outline.map(|radius| if radius > 0.0 { radius + half } else { 0.0 });
        let steps = corner_steps(&outer_radii);
        if width >= 1.0 {
            let inner_rect = [
                rect[0] + width,
                rect[1] + width,
                (rect[2] - 2.0 * width).max(0.0),
                (rect[3] - 2.0 * width).max(0.0),
            ];
            let inner_radii = outline.map(|radius| (radius - half).max(0.0));
            let outer_edge = self.ring(0.0, |points| {
                rounded_ring(points, rect, outer_radii, 0.5, steps);
            });
            let outer_core = self.ring(1.0, |points| {
                rounded_ring(points, rect, outer_radii, -0.5, steps);
            });
            let inner_core = self.ring(1.0, |points| {
                rounded_ring(points, inner_rect, inner_radii, 0.5, steps);
            });
            let inner_edge = self.ring(0.0, |points| {
                rounded_ring(points, inner_rect, inner_radii, -0.5, steps);
            });
            self.commit(&[
                Join::Strip(outer_edge, outer_core, true),
                Join::Strip(outer_core, inner_core, true),
                Join::Strip(inner_core, inner_edge, true),
            ]);
        } else {
            // A stroke thinner than a pixel covers its share of each pixel
            // along its outline.
            let line_rect = [
                rect[0] + half,
                rect[1] + half,
                (rect[2] - width).max(0.0),
                (rect[3] - width).max(0.0),
            ];
            let outer = self.ring(0.0, |points| {
                rounded_ring(points, line_rect, outline, 0.5, steps);
            });
            let middle = self.ring(width, |points| {
                rounded_ring(points, line_rect, outline, 0.0, steps);
            });
            let inner = self.ring(0.0, |points| {
                rounded_ring(points, line_rect, outline, -0.5, steps);
            });
            self.commit(&[
                Join::Strip(outer, middle, true),
                Join::Strip(middle, inner, true),
            ]);
        }
    }

    fn line(&mut self, body: &ShapeRecordBody, curve: &ShapeRecordCurve) {
        let scale = self.placement.scale;
        let offset = self.placement.offset;
        let center = [
            ((body.arc_geometry[0] + body.arc_geometry[2]) * 0.5 + offset[0]) * scale,
            ((body.arc_geometry[1] + body.arc_geometry[3]) * 0.5 + offset[1]) * scale,
        ];
        let direction = [curve.arc_normalized[0], curve.arc_normalized[1]];
        let normal = [-direction[1], direction[0]];
        let half_length = curve.arc_normalized[2] * scale;
        let half_width = (body.stroke_width * 0.5 * scale).max(0.0);
        // Butt caps end at the ends; round and square ones reach half the
        // width past them.
        let cap_reach = if cranpose_ui_graphics::record_cap_bits(body.flags) == 0 {
            0.0
        } else {
            half_width
        };
        let reach = half_length + cap_reach;
        let quad = |points: &mut Vec<Point>, along: f32, across: f32| {
            for (a, b) in [
                (-along, -across),
                (along, -across),
                (along, across),
                (-along, across),
            ] {
                points.push([
                    center[0] + direction[0] * a + normal[0] * b,
                    center[1] + direction[1] * a + normal[1] * b,
                ]);
            }
        };
        let coverage = (half_width * 2.0).min(1.0);
        let core = self.ring(coverage, |points| {
            quad(points, (reach - 0.5).max(0.0), (half_width - 0.5).max(0.0));
        });
        let outer = self.ring(0.0, |points| quad(points, reach + 0.5, half_width + 0.5));
        self.commit(&[Join::Fan(core), Join::Strip(core, outer, true)]);
    }

    fn trapezoid(&mut self, body: &ShapeRecordBody, curve: &ShapeRecordCurve) {
        let scale = self.placement.scale;
        let offset = self.placement.offset;
        let left = (curve.arc_normalized[0] + offset[0]) * scale;
        let right = (curve.arc_normalized[1] + offset[0]) * scale;
        let top = [
            (body.arc_geometry[0] + offset[1]) * scale,
            (body.arc_geometry[1] + offset[1]) * scale,
        ];
        let bottom = [
            (body.arc_geometry[2] + offset[1]) * scale,
            (body.arc_geometry[3] + offset[1]) * scale,
        ];
        let high = [top[0].min(bottom[0]), top[1].min(bottom[1])];
        let low = [top[0].max(bottom[0]), top[1].max(bottom[1])];
        // Slices of one fill share their sides: only the top and bottom
        // edges are feathered.
        let inset = |side: usize| ((low[side] - high[side]) * 0.5).min(0.5);
        let core_top = [[left, high[0] + inset(0)], [right, high[1] + inset(1)]];
        let core_bottom = [[right, low[1] - inset(1)], [left, low[0] - inset(0)]];
        let coverage = (low[0] - high[0]).max(low[1] - high[1]).min(1.0);
        let top_edge = self.ring(0.0, |points| {
            points.extend_from_slice(&[[left, high[0] - 0.5], [right, high[1] - 0.5]]);
        });
        let core = self.ring(coverage, |points| {
            points.extend_from_slice(&core_top);
            points.extend_from_slice(&core_bottom);
        });
        let bottom_core = self.ring(coverage, |points| points.extend_from_slice(&core_bottom));
        let bottom_edge = self.ring(0.0, |points| {
            points.extend_from_slice(&[[right, low[1] + 0.5], [left, low[0] + 0.5]]);
        });
        self.commit(&[
            Join::Fan(core),
            Join::Strip(top_edge, core, false),
            Join::Strip(bottom_core, bottom_edge, false),
        ]);
    }

    fn arc(&mut self, body: &ShapeRecordBody, curve: &ShapeRecordCurve) {
        if cranpose_ui_graphics::record_is_degenerate_arc(body.flags) {
            return;
        }
        let scale = self.placement.scale;
        let offset = self.placement.offset;
        let center = [
            (body.arc_geometry[0] + offset[0]) * scale,
            (body.arc_geometry[1] + offset[1]) * scale,
        ];
        let inner = (body.arc_geometry[2] * scale).max(0.0);
        let outer = (body.arc_geometry[3] * scale).max(inner);
        let start = curve.arc_normalized[0];
        let sweep = curve.arc_normalized[1].clamp(0.0, std::f32::consts::TAU);
        if sweep <= 0.0 || outer <= 0.0 {
            return;
        }
        let closed = sweep >= std::f32::consts::TAU - 1.0e-4;
        let steps = (whole::ceil(sweep * outer / 4.0) as usize).clamp(2, 96);
        let count = if closed { steps } else { steps + 1 };
        let arc_ring = move |points: &mut Vec<Point>, radius: f32| {
            let radius = radius.max(0.0);
            points.extend((0..count).map(|step| {
                let angle = start + sweep * step as f32 / steps as f32;
                [
                    center[0] + angle.cos() * radius,
                    center[1] + angle.sin() * radius,
                ]
            }));
        };
        let width = outer - inner;
        let (core_out, core_in, coverage) = if width >= 1.0 {
            (outer - 0.5, inner + 0.5, 1.0)
        } else {
            let middle = (outer + inner) * 0.5;
            (middle, middle, width)
        };
        let outer_edge = self.ring(0.0, |points| arc_ring(points, outer + 0.5));
        let outer_core = self.ring(coverage, |points| arc_ring(points, core_out));
        let inner_core = self.ring(coverage, |points| arc_ring(points, core_in));
        let inner_edge = self.ring(0.0, |points| arc_ring(points, inner - 0.5));
        self.commit(&[
            Join::Strip(outer_edge, outer_core, closed),
            Join::Strip(outer_core, inner_core, closed),
            Join::Strip(inner_core, inner_edge, closed),
        ]);
    }

    /// Appends a ring of points, which `fill` writes, at `coverage`, and
    /// returns its index for [`Self::commit`].
    fn ring(&mut self, coverage: f32, fill: impl FnOnce(&mut Vec<Point>)) -> usize {
        let start = self.builder.points.len();
        fill(&mut self.builder.points);
        self.builder.rings.push(RingSpan {
            start,
            end: self.builder.points.len(),
            coverage,
        });
        self.builder.rings.len() - 1
    }

    /// Draws the rings appended since the record began, joined as `joins`
    /// say: each ring point one vertex, unless the placement's clip cuts
    /// the record, whose triangles are then clipped one at a time.
    fn commit(&mut self, joins: &[Join]) {
        let points = std::mem::take(&mut self.builder.points);
        let rings = std::mem::take(&mut self.builder.rings);
        let clip = self
            .placement
            .clip
            .filter(|_| self.placement.turn.is_none());
        match clip.map(|clip| clip_relation(&points, clip)) {
            None | Some(ClipRelation::Inside) => self.commit_shared(&points, &rings, joins),
            Some(ClipRelation::Crossing) => self.commit_clipped(&points, &rings, joins),
            Some(ClipRelation::Outside) => {}
        }
        self.builder.points = points;
        self.builder.points.clear();
        self.builder.rings = rings;
        self.builder.rings.clear();
    }

    fn commit_shared(&mut self, points: &[Point], rings: &[RingSpan], joins: &[Join]) {
        self.builder.reserve(points.len());
        let base = self.builder.next_index();
        for ring in rings {
            for at in &points[ring.start..ring.end] {
                let vertex = self.vertex(Corner {
                    at: *at,
                    coverage: ring.coverage,
                });
                self.builder.vertices.push(vertex);
            }
        }
        let indices = &mut self.builder.indices;
        for join in joins {
            join.strip(rings, |point| indices.push(base + point as u16));
            indices.push(RESTART);
        }
    }

    fn commit_clipped(&mut self, points: &[Point], rings: &[RingSpan], joins: &[Join]) {
        let corner = |point: usize| {
            let coverage = rings
                .iter()
                .find(|ring| (ring.start..ring.end).contains(&point))
                .map_or(0.0, |ring| ring.coverage);
            Corner {
                at: points[point],
                coverage,
            }
        };
        for join in joins {
            join.triangles(rings, |triangle| {
                self.clipped_triangle(triangle.map(corner));
            });
        }
    }

    /// One triangle cut to the placement's clip, as a strip of the convex
    /// polygon left.
    fn clipped_triangle(&mut self, corners: [Corner; 3]) {
        let Some(clip) = self.placement.clip else {
            return;
        };
        let mut polygon = std::mem::take(&mut self.builder.clipped);
        polygon.clear();
        polygon.extend_from_slice(&corners);
        clip_polygon(&mut polygon, clip);
        if polygon.len() >= 3 {
            self.builder.reserve(polygon.len());
            let base = self.builder.next_index();
            for corner in &polygon {
                let vertex = self.vertex(*corner);
                self.builder.vertices.push(vertex);
            }
            let indices = &mut self.builder.indices;
            zigzag(polygon.len(), |corner| indices.push(base + corner as u16));
            indices.push(RESTART);
        }
        self.builder.clipped = polygon;
    }

    fn vertex(&self, corner: Corner) -> MeshVertex {
        let mut color = paint(self.shade.at(corner.at), self.placement.paint);
        color[3] *= corner.coverage.clamp(0.0, 1.0);
        let premultiplied = [
            color[0] * color[3],
            color[1] * color[3],
            color[2] * color[3],
            color[3],
        ];
        let at = match self.placement.turn {
            Some((turn, translation)) => [
                turn[0] * corner.at[0] + turn[1] * corner.at[1] + translation[0],
                turn[2] * corner.at[0] + turn[3] * corner.at[1] + translation[1],
            ],
            None => corner.at,
        };
        let size = self.target.size;
        let x = ((at[0] - self.target.offset[0]) / size[0]) * 2.0 - 1.0;
        let y = 1.0 - ((at[1] - self.target.offset[1]) / size[1]) * 2.0;
        MeshVertex {
            position: [x, y, self.depth],
            color: premultiplied.map(|value| (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8),
        }
    }
}

/// The colour `color` paints under `paint`: quantized to 8 bits, faded by
/// the layer alpha and filtered, as the shape stage paints it.
fn paint(color: [f32; 4], paint: Option<MeshPaint>) -> [f32; 4] {
    let Some(paint) = paint else {
        return color;
    };
    let mut painted = color.map(|value| whole::floor(value.clamp(0.0, 1.0) * 255.0 + 0.5) / 255.0);
    painted[3] = (painted[3] * paint.alpha).clamp(0.0, 1.0);
    if let Some((matrix, offset)) = paint.filter {
        let input = painted;
        painted = std::array::from_fn(|row| {
            let filtered: f32 = (0..4)
                .map(|column| matrix[column][row] * input[column])
                .sum();
            (filtered + offset[row]).clamp(0.0, 1.0)
        });
    }
    painted
}

/// How far a fill's covered core lies inside its edges, and the coverage
/// of a fill thinner than a pixel, which covers its share of each pixel.
fn thin_inset(rect: [f32; 4]) -> (f32, f32) {
    let thinnest = rect[2].min(rect[3]);
    if thinnest >= 1.0 {
        (0.5, 1.0)
    } else {
        ((thinnest * 0.5).max(0.0), thinnest.max(0.0))
    }
}

/// The steps each corner's arc takes: enough that the chords of the
/// largest stay within about a tenth of a pixel of its circle, and none at
/// a sharp corner, which every ring of a record keeps sharp, so the rings
/// keep as many points as each other.
fn corner_steps(radii: &[f32; 4]) -> [usize; 4] {
    let largest = radii.iter().copied().fold(0.0f32, f32::max);
    let segments = (whole::ceil(largest.sqrt() * 1.2) as usize).clamp(1, 12);
    radii.map(|radius| if radius > 0.0 { segments } else { 0 })
}

/// The outline of `rect` (x, y, width, height) with `radii` (top-left,
/// top-right, bottom-right, bottom-left), moved out by `grow` (in when
/// negative), clockwise from the top-left corner's start, each corner in
/// its `steps`: one point where it has none.
fn rounded_ring(
    ring: &mut Vec<Point>,
    rect: [f32; 4],
    radii: [f32; 4],
    grow: f32,
    steps: [usize; 4],
) {
    let left = rect[0] - grow;
    let top = rect[1] - grow;
    let right = (rect[0] + rect[2] + grow).max(left);
    let bottom = (rect[1] + rect[3] + grow).max(top);
    let limit = ((right - left).min(bottom - top) * 0.5).max(0.0);
    let corners = [
        ([left, top], 1.0f32, 1.0f32, std::f32::consts::PI),
        ([right, top], -1.0, 1.0, 1.5 * std::f32::consts::PI),
        ([right, bottom], -1.0, -1.0, 0.0),
        ([left, bottom], 1.0, -1.0, 0.5 * std::f32::consts::PI),
    ];
    for (corner, (at, x_in, y_in, start)) in corners.into_iter().enumerate() {
        let radius = if radii[corner] > 0.0 {
            (radii[corner] + grow).clamp(0.0, limit)
        } else {
            0.0
        };
        let center = [at[0] + x_in * radius, at[1] + y_in * radius];
        let segments = steps[corner];
        for step in 0..=segments {
            let angle = start + std::f32::consts::FRAC_PI_2 * step as f32 / segments.max(1) as f32;
            ring.push([
                center[0] + angle.cos() * radius,
                center[1] + angle.sin() * radius,
            ]);
        }
    }
}

/// Clips `polygon` to `clip` (x, y, width and height), carrying each
/// corner's coverage along the edges it cuts.
fn clip_polygon(polygon: &mut Vec<Corner>, clip: [f32; 4]) {
    let edges: [(usize, f32, bool); 4] = [
        (0, clip[0], true),
        (0, clip[0] + clip[2], false),
        (1, clip[1], true),
        (1, clip[1] + clip[3], false),
    ];
    let mut output = Vec::with_capacity(polygon.len() + 4);
    for (axis, bound, low) in edges {
        output.clear();
        let inside = |corner: &Corner| {
            if low {
                corner.at[axis] >= bound
            } else {
                corner.at[axis] <= bound
            }
        };
        for index in 0..polygon.len() {
            let current = polygon[index];
            let next = polygon[(index + 1) % polygon.len()];
            match (inside(&current), inside(&next)) {
                (true, true) => output.push(next),
                (true, false) => output.push(cross(current, next, axis, bound)),
                (false, true) => {
                    output.push(cross(current, next, axis, bound));
                    output.push(next);
                }
                (false, false) => {}
            }
        }
        std::mem::swap(polygon, &mut output);
        if polygon.is_empty() {
            return;
        }
    }
}

/// Where the edge from `a` to `b` crosses `bound` on `axis`.
fn cross(a: Corner, b: Corner, axis: usize, bound: f32) -> Corner {
    let span = b.at[axis] - a.at[axis];
    let t = if span.abs() > f32::EPSILON {
        (bound - a.at[axis]) / span
    } else {
        0.0
    };
    Corner {
        at: [
            a.at[0] + (b.at[0] - a.at[0]) * t,
            a.at[1] + (b.at[1] - a.at[1]) * t,
        ],
        coverage: a.coverage + (b.coverage - a.coverage) * t,
    }
}

/// A pass's fallback triangles and, for each of its run draws, the indices
/// that draw it while its shape pipeline compiles.
#[derive(Default)]
pub(crate) struct PassMeshes {
    pub(crate) builder: MeshBuilder,
    /// By run draw, as the pass's run draws hold them, up to the last
    /// that takes a range.
    pub(crate) ranges: Vec<Option<Range<u32>>>,
    /// Whether a pass of the current frame drew fallback triangles.
    drew: bool,
}

impl PassMeshes {
    pub(crate) fn clear(&mut self) {
        self.builder.clear();
        self.ranges.clear();
    }

    /// Opens a frame. The triangles draw only while shape pipelines
    /// compile, so the storage a frame without any leaves is freed: a
    /// launch grows it to megabytes.
    pub(crate) fn begin_frame(&mut self) {
        if !std::mem::take(&mut self.drew) {
            *self = Self::default();
        }
    }

    /// Records that the run draw at `draw` draws the fallback `indices`.
    pub(crate) fn set(&mut self, draw: usize, indices: Range<u32>) {
        self.drew = true;
        if self.ranges.len() <= draw {
            self.ranges.resize(draw + 1, None);
        }
        self.ranges[draw] = Some(indices);
    }
}

/// The buffers a pass's fallback triangles draw from.
pub(crate) struct MeshBuffers {
    pub(crate) vertices: BufferUpload,
    pub(crate) indices: BufferUpload,
}

/// What a batch's run draws draw with while their pipelines compile: the
/// index range of each, as the batch holds its draws, the pass's chunks
/// and the buffers.
#[derive(Clone, Copy)]
pub(crate) struct RunMeshes<'a> {
    pub(crate) ranges: &'a [Option<Range<u32>>],
    pub(crate) chunks: &'a [MeshChunk],
    pub(crate) buffers: &'a MeshBuffers,
}

impl RunMeshes<'_> {
    /// The ranges of the draws in `draws` of those these hold; draws past
    /// the last that takes a range take none.
    pub(crate) fn of(self, draws: Range<usize>) -> Self {
        let end = draws.end.min(self.ranges.len());
        Self {
            ranges: self.ranges.get(draws.start..end).unwrap_or_default(),
            ..self
        }
    }

    /// Draws the indices in `range` into `pass`, which has the fallback
    /// pipeline set: one draw for each chunk the range reaches, with the
    /// vertices bound from the chunk's first.
    pub(crate) fn draw(&self, pass: &mut crate::draw_pass::TrackedPass<'_, '_>, range: Range<u32>) {
        pass.set_index_buffer(self.buffers.indices.slice(), wgpu::IndexFormat::Uint16);
        for (position, chunk) in self.chunks.iter().enumerate() {
            let end = self
                .chunks
                .get(position + 1)
                .map_or(u32::MAX, |next| next.first_index);
            let drawn = range.start.max(chunk.first_index)..range.end.min(end);
            if drawn.is_empty() {
                continue;
            }
            let vertices = &self.buffers.vertices;
            let from = u64::from(chunk.first_vertex) * size_of::<MeshVertex>() as u64;
            pass.set_vertex_buffer(
                0,
                vertices
                    .buffer
                    .slice(vertices.offset + from..vertices.offset + vertices.len),
            );
            pass.draw_indexed(drawn, 0, 0..1);
        }
    }
}
