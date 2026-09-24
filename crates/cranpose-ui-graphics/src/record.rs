use std::{
    cell::OnceCell,
    hash::{Hash, Hasher},
    ops::Range,
    sync::{Arc, OnceLock},
};

use bytemuck::{Pod, Zeroable};

use crate::{
    ArcGeometry, BlendMode, Brush, Color, CornerRadii, DrawPrimitive, FxHasher, Point, Rect,
    RenderHash, ShapeRecordBody, ShapeRecordCurve, ShapeRecords, Stroke, StrokeCap, StrokeJoin,
    TAU, TileMode, arc_band, arc_trig_cache::ArcTrigCache,
};

/// The kind bits of [`ShapeRecord::flags`]: a plain rect.
pub const RECORD_KIND_RECT: u32 = 0;
/// The kind bits of [`ShapeRecord::flags`]: a rounded rect.
pub const RECORD_KIND_ROUND_RECT: u32 = 1;
/// The kind bits of [`ShapeRecord::flags`]: an arc band or annular sector.
pub const RECORD_KIND_ARC: u32 = 2;

const KIND_SHIFT: u32 = 0;
const STROKED_BIT: u32 = 1 << 2;
const CAP_SHIFT: u32 = 3;
const JOIN_SHIFT: u32 = 5;
const BLEND_SHIFT: u32 = 8;
const BAND_CAP_SHIFT: u32 = 16;
const ARC_DEGENERATE_BIT: u32 = 1 << 18;
const ARC_RECT_LOOSE_BIT: u32 = 1 << 19;
const ARC_BANDED_BIT: u32 = 1 << 20;
const BAND_CLASS_SHIFT: u32 = 21;
const BAND_CLASS_MASK: u32 = 0b111;
const NO_SEGMENT_KEY: u32 = u32::MAX;

/// How many segment-count buckets band-drawn arcs fall into: one per
/// power-of-two segment count of [`ARC_BUCKET_SEGMENTS`].
pub const ARC_BUCKETS: usize = 7;
/// The strip segments a bucket's arcs are drawn with.
pub const ARC_BUCKET_SEGMENTS: [u32; ARC_BUCKETS] = [1, 2, 4, 8, 16, 32, 64];
/// The segments a full ring takes with an outer radius up to each of
/// [`ARC_RING_RADII`], and above the last: the angular step a band at that
/// radius keeps whatever its sweep, so its chords overshoot the circle by
/// the same few pixels and a short arc takes only the segments its sweep
/// needs.
pub const ARC_RING_SEGMENTS: [u32; 4] = [8, 16, 32, 64];
/// The outer radius, in the command's units, above which a full ring moves
/// to the next of [`ARC_RING_SEGMENTS`].
pub const ARC_RING_RADII: [f32; 3] = [24.0, 96.0, 384.0];
const ARC_RING_SEGMENTS_PER_RADIAN: [f32; ARC_RING_SEGMENTS.len()] = ring_segments_per_radian();

const fn ring_segments_per_radian() -> [f32; ARC_RING_SEGMENTS.len()] {
    let mut out = [0.0; ARC_RING_SEGMENTS.len()];
    let mut index = 0;
    while index < ARC_RING_SEGMENTS.len() {
        out[index] = ARC_RING_SEGMENTS[index] as f32 / TAU;
        index += 1;
    }
    out
}
/// Bands narrower than this radius draw as their quad: the pixels a band
/// would save cost less than its vertices.
pub const ARC_BAND_MIN_RADIUS: f32 = 11.0;
/// A band whose inner radius is within the strip's margin of the centre is
/// a disc: its strip would be the whole disc's quad and more.
pub const ARC_BAND_MIN_INNER_RADIUS: f32 = 1.0;
/// The pixels a band's strip extends past the ring on each side, so the
/// edge the fragment stage anti-aliases lies inside the strip. `band_position`
/// in `shape.wgsl` pads by the same amount.
pub const BAND_MARGIN: f32 = 1.0;
/// Device-pixel padding for an arc's oriented quad: half-pixel antialias
/// support plus a sixteenth pixel for rasterization rounding.
pub const BAND_QUAD_MARGIN: f32 = 0.5 + 1.0 / 16.0;
/// The radians a band's strip extends past each end of its sweep beyond
/// the angle the ring's padded half-width subtends at its padded inner
/// radius, which covers every cap and the margin; float slack only.
/// `band_position` pads by the same amount.
pub const BAND_ANGULAR_PAD: f32 = 0.001;
/// The vertices of one quad: its four corners, which its two triangles
/// share through the strip index pattern.
pub const QUAD_VERTICES: u32 = 4;
/// The indices one quad's two triangles take.
pub const QUAD_INDICES: u32 = 6;
/// The fewest strip segments a band draws with. A short band's strip and a
/// rectangular shape both use four vertices in the same segment class.
pub const BAND_MIN_SEGMENTS: u32 = 1;

/// The vertices a strip of `segments` quads shares: an inner and an
/// outer vertex at each of its `segments + 1` boundaries.
pub const fn strip_vertices(segments: u32) -> u32 {
    segments * 2 + 2
}

/// The indices a strip of `segments` quads draws with.
pub const fn strip_indices(segments: u32) -> u32 {
    segments * QUAD_INDICES
}

/// The index pattern of one record's strip of `segments` quads over its
/// [`strip_vertices`]: vertex `2b` sits at boundary `b` on the inner
/// radius and `2b + 1` on the outer, and quad `j` is the triangles
/// `(2j, 2j + 1, 2j + 2)` and `(2j + 2, 2j + 1, 2j + 3)`. A quad-drawn
/// record is the pattern's first quad over the rect's corners: vertex
/// `2x + y` is corner `(x, y)`.
pub fn strip_index_pattern(segments: u32) -> impl Iterator<Item = u32> {
    (0..segments).flat_map(|quad| {
        let base = quad * 2;
        [base, base + 1, base + 2, base + 2, base + 1, base + 3]
    })
}
/// What one strip vertex costs against fill, in pixels of the command's
/// units. A tiling GPU writes every vertex's varyings, sixteen vectors
/// here, to memory before it shades a pixel, and reads the record the
/// vertex came from; a pixel reads and writes a few bytes and runs the
/// distance field once. A band of many segments over few pixels is dearer
/// than the quad it replaces.
pub const BAND_VERTEX_PIXELS: f32 = 32.0;
/// The strip's outer vertices ride out past the padded ring so its chords
/// circumscribe the circle; the estimate allows for that and the margin.
const BAND_STRIP_OVERSHOOT: f32 = 1.25;
/// The quads a segment may leave pinned before it is cut: a segment
/// draws every record at its largest band class, so a quad among rings
/// or a ring among quads costs pinned vertices, while a cut costs a
/// draw call, which on a low-end GPU is worth about this many quads.
const SEGMENT_WASTE_QUADS: u32 = 512;

/// The ring a band's strip is built around, in the command's units: what
/// `band_position` derives from a record, derived here from the geometry the
/// record is made of or from the record itself, with the padded sweep
/// the strip covers computed once.
struct BandRing {
    mid: f32,
    ring_half: f32,
    range_start: f32,
    range: f32,
    segments_per_radian: f32,
}

impl BandRing {
    fn of_geometry(geometry: &ArcGeometry) -> Self {
        Self::new(
            geometry.inner_radius,
            geometry.outer_radius,
            geometry.start_angle,
            geometry.sweep_angle,
        )
    }

    fn new(inner: f32, outer: f32, start: f32, sweep: f32) -> Self {
        let mid = (outer + inner) * 0.5;
        let ring_half = ((outer - inner) * 0.5).max(0.0) + BAND_MARGIN;
        let (range_start, range) = Self::padded_range(mid, ring_half, start, sweep);
        Self {
            mid,
            ring_half,
            range_start,
            range,
            segments_per_radian: Self::segments_per_radian(outer),
        }
    }

    /// The segments per radian a full ring takes at `outer_radius`: the
    /// angular step every band at that radius keeps.
    #[inline]
    fn segments_per_radian(outer_radius: f32) -> f32 {
        let bucket = usize::from(outer_radius > ARC_RING_RADII[0])
            + usize::from(outer_radius > ARC_RING_RADII[1])
            + usize::from(outer_radius > ARC_RING_RADII[2]);
        ARC_RING_SEGMENTS_PER_RADIAN[bucket]
    }

    /// Where the strip starts and the padded sweep it covers, which the
    /// record carries for the vertex stage: the angular pad is bounded
    /// above (atan(x) is at most x), so no transcendental per arc, and a
    /// sweep the pad closes is the full circle.
    fn padded_range(mid: f32, ring_half: f32, start: f32, sweep: f32) -> (f32, f32) {
        let inner_padded = mid - ring_half;
        if inner_padded <= 0.0 {
            return (0.0, TAU);
        }
        let pad = ring_half / inner_padded + BAND_ANGULAR_PAD;
        let padded = sweep + pad + pad;
        if padded < TAU {
            (start - pad, padded)
        } else {
            (0.0, TAU)
        }
    }

    /// The fewest of [`ARC_BUCKET_SEGMENTS`] whose step over the padded
    /// sweep stays within the ring step at this radius.
    #[inline]
    fn segments(&self) -> u32 {
        let exact = self.range * self.segments_per_radian;
        if exact <= BAND_MIN_SEGMENTS as f32 {
            return BAND_MIN_SEGMENTS;
        }
        let floor = exact as u32;
        let needed = if (floor as f32) < exact {
            floor + 1
        } else {
            floor
        };
        needed
            .max(BAND_MIN_SEGMENTS)
            .next_power_of_two()
            .min(ARC_BUCKET_SEGMENTS[ARC_BUCKETS - 1])
    }

    /// The strip's cost in pixels: the padded ring's area over the padded
    /// sweep, plus what its `segments` quads' vertices cost.
    fn strip_pixels(&self, segments: u32) -> f32 {
        self.range * self.mid * (self.ring_half + self.ring_half) * BAND_STRIP_OVERSHOOT
            + vertex_pixels(strip_vertices(segments))
    }
}

fn vertex_pixels(vertices: u32) -> f32 {
    vertices as f32 * BAND_VERTEX_PIXELS
}

/// The bucket of [`ARC_BUCKET_SEGMENTS`] a strip of `segments` draws from.
pub fn band_bucket(segments: u32) -> usize {
    segments.trailing_zeros() as usize
}

/// The strip segments every record of band class `class` is drawn at.
pub fn band_class_segments(class: u8) -> u32 {
    ARC_BUCKET_SEGMENTS[class as usize]
}

/// The bucket an arc's band draws from when its strip costs less than
/// `rect`, the quad it would otherwise draw: the pixels each rasterizes
/// plus what their vertices cost a tiling GPU. `None` when the quad is
/// cheaper.
#[inline]
fn band_bucket_for(geometry: &ArcGeometry, ring: &BandRing, rect: Rect) -> Option<usize> {
    if geometry.is_degenerate()
        || geometry.outer_radius < ARC_BAND_MIN_RADIUS
        || geometry.inner_radius <= ARC_BAND_MIN_INNER_RADIUS
    {
        return None;
    }
    let segments = ring.segments();
    (ring.strip_pixels(segments) < rect.width * rect.height + vertex_pixels(QUAD_VERTICES))
        .then(|| band_bucket(segments))
}

/// Whether an arc's band strip costs less than `rect`, the quad it would
/// otherwise draw; see [`ShapeRecord::is_banded`].
pub fn band_pays(geometry: &ArcGeometry, rect: Rect) -> bool {
    band_bucket_for(geometry, &BandRing::of_geometry(geometry), rect).is_some()
}

/// The fragment program's shape kinds: a filled rect or round rect, a
/// stroked one, and an arc band.
pub const FRAGMENT_KIND_FILL: u32 = 0;
pub const FRAGMENT_KIND_STROKE: u32 = 1;
pub const FRAGMENT_KIND_ARC: u32 = 2;

const TWO_BITS: u32 = 0b11;
const BLEND_MASK: u32 = 0xff;

/// The brush kinds of [`BrushRecord::kind`].
pub const BRUSH_KIND_LINEAR: u32 = 1;
/// See [`BRUSH_KIND_LINEAR`].
pub const BRUSH_KIND_RADIAL: u32 = 2;
/// See [`BRUSH_KIND_LINEAR`].
pub const BRUSH_KIND_SWEEP: u32 = 3;

/// One complete recorded shape in the draw command's local space.
/// Every value the app passed is kept verbatim, so the record
/// materialises back into the exact [`DrawPrimitive`] the call described,
/// and the derived arc values the fragment stage needs sit beside them.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct ShapeRecord {
    /// The primitive's rect: the app's rect for rects; for arcs the band's
    /// tight bounds, or, when [`Self::has_loose_rect`], the disc around the
    /// band that the tight bounds are derived from on demand.
    pub rect: [f32; 4],
    /// Rects: the corner radii, top-left, top-right, bottom-right,
    /// bottom-left. Arcs: the band's trig, see [`arc_trig`].
    pub radii: [f32; 4],
    /// The solid colour, or the first stop of a gradient brush.
    pub color: [f32; 4],
    /// The stroke width when [`Self::is_stroked`].
    pub stroke_width: f32,
    /// Kind, stroke, cap, join, blend mode and arc facts, packed; read them
    /// through the accessors.
    pub flags: u32,
    /// `0` for a solid brush, otherwise one plus the index into the
    /// recording's brush table.
    pub brush: u32,
    /// Keeps the record at a whole number of 16-byte rows.
    pub reserved: u32,
    /// Arcs: centre x, centre y, radius and inner radius as the app drew them.
    pub arc: [f32; 4],
    /// Arcs: start angle and sweep as the app drew them, then the normalised
    /// band's inner and outer radius.
    pub arc_band: [f32; 4],
    /// Arcs: the normalised start angle and sweep, then where the band's
    /// strip starts and the padded sweep it covers.
    pub arc_normalized: [f32; 4],
}

impl ShapeRecord {
    pub fn kind(&self) -> u32 {
        (self.flags >> KIND_SHIFT) & TWO_BITS
    }

    pub fn is_stroked(&self) -> bool {
        self.flags & STROKED_BIT != 0
    }

    /// Which coverage program the fragment stage runs for this record.
    pub fn fragment_kind(&self) -> u32 {
        fragment_kind(self.flags)
    }

    pub fn stroke(&self) -> Option<Stroke> {
        self.is_stroked().then(|| Stroke {
            width: self.stroke_width,
            cap: STROKE_CAPS[((self.flags >> CAP_SHIFT) & TWO_BITS) as usize],
            join: STROKE_JOINS[((self.flags >> JOIN_SHIFT) & TWO_BITS) as usize],
        })
    }

    pub fn blend_mode(&self) -> BlendMode {
        BlendMode::ALL[((self.flags >> BLEND_SHIFT) & BLEND_MASK) as usize]
    }

    /// The cap of the normalised arc band.
    pub fn band_cap(&self) -> StrokeCap {
        STROKE_CAPS[((self.flags >> BAND_CAP_SHIFT) & TWO_BITS) as usize]
    }

    /// An arc whose band draws nothing: zero sweep, zero width or a
    /// non-finite input. The GPU path collapses it; the primitive still
    /// materialises as the app drew it.
    pub fn is_degenerate_arc(&self) -> bool {
        self.flags & ARC_DEGENERATE_BIT != 0
    }

    pub fn is_gradient(&self) -> bool {
        self.brush != 0
    }

    /// An arc wide enough to draw as a band strip from the vertex stage;
    /// its band class sits in its flags.
    pub fn is_banded(&self) -> bool {
        self.flags & ARC_BANDED_BIT != 0
    }

    /// The band class this record was filed in: the index into
    /// [`ARC_BUCKET_SEGMENTS`] of the strip segments it draws with when
    /// [`Self::is_banded`], zero otherwise.
    pub fn band_class(&self) -> usize {
        ((self.flags >> BAND_CLASS_SHIFT) & BAND_CLASS_MASK) as usize
    }

    /// The strip segments this record draws with when [`Self::is_banded`].
    pub fn band_segments(&self) -> u32 {
        ARC_BUCKET_SEGMENTS[self.band_class()]
    }

    /// An arc recorded by the draw scope: its rect is the disc around the
    /// band, and the tight cap-aware bounds the primitive carries are
    /// derived when asked for, never on the recording path.
    pub fn has_loose_rect(&self) -> bool {
        self.flags & ARC_RECT_LOOSE_BIT != 0
    }

    /// The rect as stored: loose for a scope-recorded arc.
    pub fn stored_rect(&self) -> Rect {
        row_rect(self.rect)
    }

    /// The rect the materialised primitive carries: the tight band bounds
    /// for a scope-recorded arc, the stored rect otherwise.
    pub fn rect_value(&self) -> Rect {
        match self.arc_geometry() {
            Some(geometry) if self.has_loose_rect() => geometry.bounds(),
            _ => self.stored_rect(),
        }
    }

    /// The rect this record's pixels can reach: its rect grown by half the
    /// stroke width.
    pub fn coverage_rect(&self) -> Rect {
        expand_rect(self.rect_value(), self.half_stroke())
    }

    fn half_stroke(&self) -> f32 {
        if self.is_stroked() {
            self.stroke_width * 0.5
        } else {
            0.0
        }
    }

    /// The normalised band the fragment stage draws; `None` for rects.
    pub fn arc_geometry(&self) -> Option<ArcGeometry> {
        (self.kind() == RECORD_KIND_ARC).then(|| ArcGeometry {
            center: Point::new(self.arc[0], self.arc[1]),
            inner_radius: self.arc_band[2],
            outer_radius: self.arc_band[3],
            start_angle: self.arc_normalized[0],
            sweep_angle: self.arc_normalized[1],
            cap: self.band_cap(),
        })
    }
}

fn fragment_kind(flags: u32) -> u32 {
    if (flags >> KIND_SHIFT) & TWO_BITS == RECORD_KIND_ARC {
        FRAGMENT_KIND_ARC
    } else if flags & STROKED_BIT != 0 {
        FRAGMENT_KIND_STROKE
    } else {
        FRAGMENT_KIND_FILL
    }
}

const STROKE_CAPS: [StrokeCap; 3] = [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square];
const STROKE_JOINS: [StrokeJoin; 3] = [StrokeJoin::Miter, StrokeJoin::Round, StrokeJoin::Bevel];
const TILE_MODES: [TileMode; 4] = [
    TileMode::Clamp,
    TileMode::Repeated,
    TileMode::Mirror,
    TileMode::Decal,
];

#[inline]
fn pack_flags(kind: u32, stroke: Option<Stroke>, blend: BlendMode, band_cap: StrokeCap) -> u32 {
    let mut flags = (kind << KIND_SHIFT) | ((blend as u32) << BLEND_SHIFT);
    if let Some(stroke) = stroke {
        flags |=
            STROKED_BIT | ((stroke.cap as u32) << CAP_SHIFT) | ((stroke.join as u32) << JOIN_SHIFT);
    }
    flags | ((band_cap as u32) << BAND_CAP_SHIFT)
}

/// A gradient brush of a recording, addressed by [`ShapeRecord::brush`].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct BrushRecord {
    /// One of the `BRUSH_KIND_*` constants.
    pub kind: u32,
    /// The [`TileMode`] as its declaration index.
    pub tile_mode: u32,
    /// The brush's stops in [`CommandRecording::stops`].
    pub stop_start: u32,
    pub stop_count: u32,
    /// Linear: start x, start y, end x, end y. Radial: centre x, centre y,
    /// radius, 0. Sweep: centre x, centre y, 0, 0.
    pub params: [f32; 4],
    /// The app's explicit stop positions in
    /// [`RecordTables::explicit_stops`], when it gave any; `explicit_len`
    /// is `u32::MAX` when it gave none.
    pub explicit_start: u32,
    pub explicit_len: u32,
    pub reserved: [u32; 2],
}

const NO_EXPLICIT_STOPS: u32 = u32::MAX;

/// One gradient stop in the layout the shader reads.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct GradientStopRecord {
    pub color: [f32; 4],
    /// The position in `x`; the rest keeps the 16-byte row.
    pub position: [f32; 4],
}

/// Which lane a segment's entries live in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecordLane {
    /// Entries in [`CommandRecording::shapes`].
    Shapes,
    /// Entries in [`CommandRecording::others`]: images, text, shadows and
    /// nested blends.
    Others,
    /// One `draw_content` marker; carries no entry.
    Content,
}

/// A run of consecutive entries of one lane that a single draw can take:
/// shapes sharing a blend mode and a brush class, or a run of other
/// primitives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RecordSegment {
    pub lane: RecordLane,
    pub start: u32,
    pub count: u32,
    pub blend: BlendMode,
    pub gradient: bool,
    /// One bit per `BRUSH_KIND_*` present in the segment; bit 0 stands for
    /// the records drawn without a brush.
    pub brushes: u8,
    /// One bit per `FRAGMENT_KIND_*` present in the segment.
    pub kinds: u8,
    /// The band class every record of the segment is drawn at: the index
    /// into [`ARC_BUCKET_SEGMENTS`] of the strip each record is instanced
    /// over, so one draw covers the segment in record order.
    pub band_class: u8,
}

impl RecordSegment {
    pub fn range(&self) -> Range<usize> {
        self.start as usize..(self.start + self.count) as usize
    }

    /// The one kind every record of the segment has, if they agree.
    pub fn uniform_kind(&self) -> Option<u32> {
        (self.kinds.count_ones() == 1).then(|| self.kinds.trailing_zeros())
    }

    /// The one brush kind every record of the segment paints with, if they
    /// agree; zero when none of them has a brush.
    pub fn uniform_brush(&self) -> Option<u32> {
        (self.brushes.count_ones() == 1).then(|| self.brushes.trailing_zeros())
    }
}

/// The POD half of a recording: what the GPU reads, shared with the
/// renderer behind an `Arc` so a packet carries it without a copy.
#[derive(Clone, Debug, Default)]
pub struct RecordTables {
    pub shapes: ShapeRecords,
    pub brushes: Vec<BrushRecord>,
    pub stops: Vec<GradientStopRecord>,
    pub explicit_stops: Vec<f32>,
    pub segments: Vec<RecordSegment>,
    fingerprint: OnceLock<u64>,
}

impl PartialEq for RecordTables {
    fn eq(&self, other: &Self) -> bool {
        self.shapes == other.shapes
            && self.brushes == other.brushes
            && self.stops == other.stops
            && self.explicit_stops == other.explicit_stops
            && self.segments == other.segments
    }
}

impl RecordTables {
    fn clear(&mut self) {
        self.shapes.clear();
        self.brushes.clear();
        self.stops.clear();
        self.explicit_stops.clear();
        self.segments.clear();
        self.fingerprint.take();
    }

    /// Empty tables with this one's capacities, so a recording that starts
    /// while a scene still holds the last one grows nothing.
    fn with_capacity_of(&self) -> Self {
        Self {
            shapes: ShapeRecords::with_capacity(self.shapes.capacity()),
            brushes: Vec::with_capacity(self.brushes.capacity()),
            stops: Vec::with_capacity(self.stops.capacity()),
            explicit_stops: Vec::with_capacity(self.explicit_stops.capacity()),
            segments: Vec::with_capacity(self.segments.capacity()),
            fingerprint: OnceLock::new(),
        }
    }

    /// A hash of everything the GPU reads and of the segments, computed
    /// the first time a cache asks and kept while the tables stand.
    pub fn fingerprint(&self) -> u64 {
        *self.fingerprint.get_or_init(|| {
            let mut hasher = FxHasher::default();
            hasher.write(bytemuck::cast_slice(self.shapes.bodies()));
            hasher.write(bytemuck::cast_slice(self.shapes.curves()));
            hasher.write(self.shapes.source_bytes());
            hasher.write(bytemuck::cast_slice(&self.brushes));
            hasher.write(bytemuck::cast_slice(&self.stops));
            hasher.write(bytemuck::cast_slice(&self.explicit_stops));
            self.segments.hash(&mut hasher);
            hasher.finish()
        })
    }

    /// The heap the tables hold, capacity included.
    pub fn heap_bytes(&self) -> usize {
        self.shapes.heap_bytes()
            + self.brushes.capacity() * std::mem::size_of::<BrushRecord>()
            + self.stops.capacity() * std::mem::size_of::<GradientStopRecord>()
            + self.explicit_stops.capacity() * std::mem::size_of::<f32>()
            + self.segments.capacity() * std::mem::size_of::<RecordSegment>()
    }
}

/// What a recording contains, answered once while recording so no consumer
/// rescans it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordingSummary {
    /// Any text, including inside a blend: glyph masks want rigid snapping.
    pub has_text: bool,
    pub has_shadow: bool,
    /// Any primitive besides a shadow.
    pub has_non_shadow: bool,
    /// Any image or text, including inside a blend: content that resamples
    /// badly on a fractionally offset surface.
    pub has_pixel_sensitive: bool,
}

impl RecordingSummary {
    fn note(&mut self, primitive: &DrawPrimitive) {
        if matches!(primitive, DrawPrimitive::Shadow(_)) {
            self.has_shadow = true;
            return;
        }
        if matches!(primitive, DrawPrimitive::Content) {
            return;
        }
        self.has_non_shadow = true;
        match unwrap_blend(primitive) {
            DrawPrimitive::Text(_) => {
                self.has_text = true;
                self.has_pixel_sensitive = true;
            }
            DrawPrimitive::Image { .. } => self.has_pixel_sensitive = true,
            _ => {}
        }
    }

    fn merge(&mut self, other: Self) {
        self.has_text |= other.has_text;
        self.has_shadow |= other.has_shadow;
        self.has_non_shadow |= other.has_non_shadow;
        self.has_pixel_sensitive |= other.has_pixel_sensitive;
    }
}

fn unwrap_blend(mut primitive: &DrawPrimitive) -> &DrawPrimitive {
    while let DrawPrimitive::Blend {
        primitive: inner, ..
    } = primitive
    {
        primitive = inner;
    }
    primitive
}

/// `rect` grown by `margin` on every side.
pub fn expand_rect(rect: Rect, margin: f32) -> Rect {
    Rect {
        x: rect.x - margin,
        y: rect.y - margin,
        width: rect.width + margin * 2.0,
        height: rect.height + margin * 2.0,
    }
}

/// The rect a primitive's pixels can reach, or `None` when it has none of
/// its own (a content marker or a shadow).
pub fn primitive_coverage_rect(primitive: &DrawPrimitive) -> Option<Rect> {
    match primitive {
        DrawPrimitive::Blend { primitive, .. } => primitive_coverage_rect(primitive),
        DrawPrimitive::Rect { rect, stroke, .. }
        | DrawPrimitive::RoundRect { rect, stroke, .. }
        | DrawPrimitive::Arc { rect, stroke, .. } => {
            let half_stroke = stroke.as_ref().map_or(0.0, |stroke| stroke.width * 0.5);
            Some(expand_rect(*rect, half_stroke))
        }
        DrawPrimitive::Image { rect, .. } => Some(*rect),
        DrawPrimitive::Text(text) => Some(text.rect),
        DrawPrimitive::Content | DrawPrimitive::Shadow(_) => None,
    }
}

/// The POD half of a recording as it is written: the shape records with
/// their brush and stop tables and coalesced segments, and the bounds
/// those records reach. Plain data throughout, so a scene carries one
/// across threads; [`CommandRecording`] wraps it with the other lane.
#[derive(Clone, Debug)]
pub struct ShapeRecorder {
    tables: RecordTables,
    arc_trig: ArcTrigCache,
    last_segment_key: u32,
    segment_waste: u32,
    min: [f32; 2],
    max: [f32; 2],
}

impl Default for ShapeRecorder {
    fn default() -> Self {
        Self {
            tables: RecordTables::default(),
            arc_trig: ArcTrigCache::default(),
            last_segment_key: NO_SEGMENT_KEY,
            segment_waste: 0,
            min: [f32::INFINITY; 2],
            max: [f32::NEG_INFINITY; 2],
        }
    }
}

impl PartialEq for ShapeRecorder {
    fn eq(&self, other: &Self) -> bool {
        self.tables == other.tables
    }
}

/// What [`ShapeRecorder::push_primitive`] did with a primitive.
pub enum Recorded {
    /// A shape was recorded; its coverage rect.
    Shape(Rect),
    /// The primitive is not a shape and is handed back.
    Other(DrawPrimitive),
}

#[inline]
fn extend_segment_in(tables: &mut RecordTables, extend: bool, opened: RecordSegment) {
    if extend {
        let last = tables.segments.last_mut().expect("a keyed segment exists");
        last.count += 1;
        last.brushes |= opened.brushes;
        last.kinds |= opened.kinds;
        last.band_class = last.band_class.max(opened.band_class);
        return;
    }
    tables.segments.push(opened);
}

impl ShapeRecorder {
    /// The owned shape columns and their brush, stop and segment tables.
    pub fn tables(&self) -> &RecordTables {
        &self.tables
    }

    fn tables_mut(&mut self) -> &mut RecordTables {
        let tables = &mut self.tables;
        tables.fingerprint.take();
        tables
    }

    pub fn is_empty(&self) -> bool {
        self.tables.shapes.is_empty()
    }

    /// Every segment, the range a run of the whole recording draws.
    pub fn all_segments(&self) -> Range<u32> {
        0..self.tables.segments.len() as u32
    }

    /// The rect every recorded shape's pixels lie within; `None` when
    /// nothing was recorded.
    pub fn bounds(&self) -> Option<Rect> {
        (self.min[0] <= self.max[0] && self.min[1] <= self.max[1]).then(|| Rect {
            x: self.min[0],
            y: self.min[1],
            width: self.max[0] - self.min[0],
            height: self.max[1] - self.min[1],
        })
    }

    /// The tables' fingerprint; see [`RecordTables::fingerprint`].
    pub fn fingerprint(&self) -> u64 {
        self.tables.fingerprint()
    }

    /// Empties the recorder while retaining its table capacities.
    pub fn clear(&mut self) {
        self.tables.clear();
        self.last_segment_key = NO_SEGMENT_KEY;
        self.segment_waste = 0;
        self.min = [f32::INFINITY; 2];
        self.max = [f32::NEG_INFINITY; 2];
    }

    /// Records a rect, rounded rect or arc, blended or not; hands any
    /// other primitive back untouched.
    pub fn push_primitive(&mut self, primitive: DrawPrimitive) -> Recorded {
        match primitive {
            DrawPrimitive::Blend {
                primitive,
                blend_mode,
            } => match self.push_shape_primitive(*primitive, blend_mode) {
                Recorded::Other(inner) => Recorded::Other(DrawPrimitive::Blend {
                    primitive: Box::new(inner),
                    blend_mode,
                }),
                recorded => recorded,
            },
            other => self.push_shape_primitive(other, BlendMode::SrcOver),
        }
    }

    /// Records a rect, rounded rect or arc with the supplied blend mode.
    /// Returns any other primitive untouched, including blend wrappers.
    pub fn push_shape_primitive(
        &mut self,
        primitive: DrawPrimitive,
        blend_mode: BlendMode,
    ) -> Recorded {
        Recorded::Shape(match primitive {
            DrawPrimitive::Rect {
                rect,
                brush,
                stroke,
            } => self.push_rect(rect, &brush, stroke, blend_mode),
            DrawPrimitive::RoundRect {
                rect,
                brush,
                radii,
                stroke,
            } => self.push_round_rect(rect, &brush, radii, stroke, blend_mode),
            DrawPrimitive::Arc {
                rect,
                brush,
                center,
                radius,
                start_angle,
                sweep_angle,
                stroke,
                inner_radius,
            } => self.push_arc(
                rect,
                &ArcRecordArgs {
                    brush: &brush,
                    center,
                    radius,
                    start_angle,
                    sweep_angle,
                    stroke,
                    inner_radius,
                    blend_mode,
                },
            ),
            other => return Recorded::Other(other),
        })
    }

    fn push_content_segment(&mut self) {
        self.last_segment_key = NO_SEGMENT_KEY;
        self.tables_mut().segments.push(RecordSegment {
            lane: RecordLane::Content,
            start: 0,
            count: 1,
            blend: BlendMode::SrcOver,
            gradient: false,
            brushes: 0,
            kinds: 0,
            band_class: 0,
        });
    }

    #[inline]
    pub fn push_rect(
        &mut self,
        rect: Rect,
        brush: &Brush,
        stroke: Option<Stroke>,
        blend: BlendMode,
    ) -> Rect {
        let (handle, color) = self.intern_brush(brush);
        self.push_shape(
            ShapeRecordBody {
                rect: rect_row(rect),
                color,
                stroke_width: stroke.map_or(0.0, |stroke| stroke.width),
                flags: pack_flags(RECORD_KIND_RECT, stroke, blend, StrokeCap::Butt),
                brush: handle,
                placement: 0,
                arc_geometry: [0.0; 4],
            },
            ShapeRecordCurve {
                radii: [0.0; 4],
                arc_normalized: [0.0; 4],
            },
            [0.0; 4],
            blend,
            None,
        )
    }

    pub fn push_round_rect(
        &mut self,
        rect: Rect,
        brush: &Brush,
        radii: CornerRadii,
        stroke: Option<Stroke>,
        blend: BlendMode,
    ) -> Rect {
        let (handle, color) = self.intern_brush(brush);
        let mut flags = pack_flags(RECORD_KIND_ROUND_RECT, stroke, blend, StrokeCap::Butt);
        let mut arc_geometry = [0.0; 4];
        let mut source = [0.0; 4];
        let mut arc_normalized = [0.0; 4];
        let mut bucket = None;
        if let Some(ring) = stroked_circle_ring(rect, radii, stroke)
            && let band = BandRing::of_geometry(&ring)
            && let Some(ring_bucket) =
                band_bucket_for(&ring, &band, expand_rect(rect, ring.half_thickness()))
        {
            flags |= ARC_BANDED_BIT;
            arc_geometry = [
                ring.center.x,
                ring.center.y,
                ring.inner_radius,
                ring.outer_radius,
            ];
            source = [ring.mid_radius(), ring.inner_radius, 0.0, TAU];
            arc_normalized = [0.0, TAU, band.range_start, band.range];
            bucket = Some(ring_bucket);
        }
        self.push_shape(
            ShapeRecordBody {
                rect: rect_row(rect),
                color,
                stroke_width: stroke.map_or(0.0, |stroke| stroke.width),
                flags,
                brush: handle,
                placement: 0,
                arc_geometry,
            },
            ShapeRecordCurve {
                radii: [
                    radii.top_left,
                    radii.top_right,
                    radii.bottom_right,
                    radii.bottom_left,
                ],
                arc_normalized,
            },
            source,
            blend,
            bucket,
        )
    }

    /// Records an arc band or annular sector with the rect the primitive
    /// carries.
    pub fn push_arc(&mut self, rect: Rect, args: &ArcRecordArgs<'_>) -> Rect {
        let geometry = normalized_band(args);
        self.push_arc_band(args, &geometry, Some(rect))
    }

    /// Records an arc the draw scope drew, whose band the scope already
    /// normalised: the record keeps the disc around the band as its rect
    /// and derives the primitive's tight bounds only when asked.
    #[inline]
    pub fn push_scope_arc(&mut self, args: &ArcRecordArgs<'_>, geometry: &ArcGeometry) -> Rect {
        self.push_arc_band(args, geometry, None)
    }

    #[inline]
    fn push_arc_band(
        &mut self,
        args: &ArcRecordArgs<'_>,
        geometry: &ArcGeometry,
        rect: Option<Rect>,
    ) -> Rect {
        let (handle, color) = self.intern_brush(args.brush);
        let mut flags = pack_flags(RECORD_KIND_ARC, args.stroke, args.blend_mode, geometry.cap);
        if geometry.is_degenerate() {
            flags |= ARC_DEGENERATE_BIT;
        }
        let rect = rect.unwrap_or_else(|| {
            flags |= ARC_RECT_LOOSE_BIT;
            band_disc(geometry)
        });
        let ring = BandRing::of_geometry(geometry);
        let bucket = band_bucket_for(geometry, &ring, rect);
        if bucket.is_some() {
            flags |= ARC_BANDED_BIT;
        }
        let radii = self.arc_trig.resolve(geometry);
        self.push_shape(
            ShapeRecordBody {
                rect: rect_row(rect),
                color,
                stroke_width: args.stroke.map_or(0.0, |stroke| stroke.width),
                flags,
                brush: handle,
                placement: 0,
                arc_geometry: [
                    args.center.x,
                    args.center.y,
                    geometry.inner_radius,
                    geometry.outer_radius,
                ],
            },
            ShapeRecordCurve {
                radii,
                arc_normalized: [
                    geometry.start_angle,
                    geometry.sweep_angle,
                    ring.range_start,
                    ring.range,
                ],
            },
            [
                args.radius,
                args.inner_radius,
                args.start_angle,
                args.sweep_angle,
            ],
            args.blend_mode,
            bucket,
        )
    }

    #[inline(always)]
    fn push_shape(
        &mut self,
        mut body: ShapeRecordBody,
        curve: ShapeRecordCurve,
        source: [f32; 4],
        blend: BlendMode,
        band_bucket: Option<usize>,
    ) -> Rect {
        let half_stroke = if body.flags & STROKED_BIT != 0 {
            body.stroke_width * 0.5
        } else {
            0.0
        };
        let coverage = expand_rect(row_rect(body.rect), half_stroke);
        self.include_bounds(coverage);
        let index = self.tables.shapes.len() as u32;
        let gradient = body.brush != 0;
        let brush_bit = 1u8
            << match body.brush {
                0 => 0,
                index => self.tables.brushes[index as usize - 1].kind,
            };
        let kind_bit = 1u8 << fragment_kind(body.flags);
        let band_class = band_bucket.unwrap_or(0) as u8;
        body.flags |= u32::from(band_class) << BAND_CLASS_SHIFT;
        let extend = self.note_segment_key(RecordLane::Shapes, blend, gradient)
            && self.segment_takes_class(band_class);
        if !extend {
            self.segment_waste = 0;
        }
        let tables = self.tables_mut();
        tables.shapes.push(body, curve, source);
        extend_segment_in(
            tables,
            extend,
            RecordSegment {
                lane: RecordLane::Shapes,
                start: index,
                count: 1,
                blend,
                gradient,
                brushes: brush_bit,
                kinds: kind_bit,
                band_class,
            },
        );
        coverage
    }

    fn extend_segment(
        &mut self,
        lane: RecordLane,
        index: u32,
        blend: BlendMode,
        gradient: bool,
        kind_bit: u8,
    ) {
        let extend = self.note_segment_key(lane, blend, gradient);
        if !extend {
            self.segment_waste = 0;
        }
        extend_segment_in(
            self.tables_mut(),
            extend,
            RecordSegment {
                lane,
                start: index,
                count: 1,
                blend,
                gradient,
                brushes: 0,
                kinds: kind_bit,
                band_class: 0,
            },
        );
    }

    /// Whether the open segment takes a record of `band_class` without
    /// leaving more than [`SEGMENT_WASTE_QUADS`] pinned: a larger class
    /// raises every earlier record's budget, a smaller one collapses its
    /// own surplus. Accounts the waste when it does.
    #[inline]
    fn segment_takes_class(&mut self, band_class: u8) -> bool {
        let last = self.tables.segments.last().expect("a keyed segment exists");
        let held = ARC_BUCKET_SEGMENTS[last.band_class as usize];
        let wanted = ARC_BUCKET_SEGMENTS[band_class as usize];
        let waste = if wanted > held {
            last.count * (wanted - held)
        } else {
            held - wanted
        };
        if self.segment_waste + waste > SEGMENT_WASTE_QUADS {
            return false;
        }
        self.segment_waste += waste;
        true
    }

    /// Whether the next record continues the open segment, and makes its
    /// key the open one.
    #[inline]
    fn note_segment_key(&mut self, lane: RecordLane, blend: BlendMode, gradient: bool) -> bool {
        let key = ((lane as u32) << 16) | ((blend as u32) << 1) | gradient as u32;
        let extend = key == self.last_segment_key;
        self.last_segment_key = key;
        extend
    }

    /// Widens the bounds to `rect`; plain compares, not the NaN-ordering
    /// `f32::min`, which armv7 lowers to a libm call per edge.
    #[inline]
    fn include_bounds(&mut self, rect: Rect) {
        let right = rect.x + rect.width;
        let bottom = rect.y + rect.height;
        if rect.x < self.min[0] {
            self.min[0] = rect.x;
        }
        if rect.y < self.min[1] {
            self.min[1] = rect.y;
        }
        if right > self.max[0] {
            self.max[0] = right;
        }
        if bottom > self.max[1] {
            self.max[1] = bottom;
        }
    }

    /// The brush handle a record carries and its colour row: the colour
    /// itself for a solid brush, the first stop for a gradient, which is
    /// interned into the brush and stop tables.
    #[inline]
    fn intern_brush(&mut self, brush: &Brush) -> (u32, [f32; 4]) {
        match brush {
            Brush::Solid(color) => (0, [color.0, color.1, color.2, color.3]),
            gradient => self.intern_gradient(gradient),
        }
    }

    #[cold]
    #[inline(never)]
    fn intern_gradient(&mut self, brush: &Brush) -> (u32, [f32; 4]) {
        let (kind, tile_mode, params, colors, stops) = match brush {
            Brush::Solid(color) => return (0, [color.0, color.1, color.2, color.3]),
            Brush::LinearGradient {
                colors,
                stops,
                start,
                end,
                tile_mode,
            } => (
                BRUSH_KIND_LINEAR,
                *tile_mode,
                [start.x, start.y, end.x, end.y],
                colors,
                stops,
            ),
            Brush::RadialGradient {
                colors,
                stops,
                center,
                radius,
                tile_mode,
            } => (
                BRUSH_KIND_RADIAL,
                *tile_mode,
                [center.x, center.y, *radius, 0.0],
                colors,
                stops,
            ),
            Brush::SweepGradient {
                colors,
                stops,
                center,
            } => (
                BRUSH_KIND_SWEEP,
                TileMode::Clamp,
                [center.x, center.y, 0.0, 0.0],
                colors,
                stops,
            ),
        };
        let tables = self.tables_mut();
        let stop_start = tables.stops.len() as u32;
        let count = colors.len();
        let positions = stops.as_deref().filter(|values| values.len() == count);
        for (index, color) in colors.iter().enumerate() {
            let position = positions.map_or_else(
                || {
                    if count <= 1 {
                        0.0
                    } else {
                        index as f32 / (count - 1) as f32
                    }
                },
                |values| values[index],
            );
            tables.stops.push(GradientStopRecord {
                color: [color.0, color.1, color.2, color.3],
                position: [position, 0.0, 0.0, 0.0],
            });
        }
        let (explicit_start, explicit_len) = match stops {
            Some(values) => {
                let start = tables.explicit_stops.len() as u32;
                tables.explicit_stops.extend_from_slice(values);
                (start, values.len() as u32)
            }
            None => (0, NO_EXPLICIT_STOPS),
        };
        let record = BrushRecord {
            kind,
            tile_mode: tile_mode as u32,
            stop_start,
            stop_count: count as u32,
            params,
            explicit_start,
            explicit_len,
            reserved: [0; 2],
        };
        tables.brushes.push(record);
        let handle = tables.brushes.len() as u32;
        let first = colors.first().copied().unwrap_or(Color(0.0, 0.0, 0.0, 0.0));
        (handle, [first.0, first.1, first.2, first.3])
    }
}

/// A completed draw command with shared shape data, ordered segments and
/// non-shape primitives. Bounds and content metadata are recorded while drawing;
/// the complete fingerprint is computed when a cache first requests it.
/// Use [`CommandRecorder`] to build a command and [`Self::into_recorder`] to edit it.
#[derive(Clone, Debug)]
pub struct CommandRecording {
    shapes: Arc<ShapeRecorder>,
    content: RecordingContent,
    fingerprint: OnceCell<u64>,
}

#[derive(Clone, Debug)]
struct RecordingContent {
    others: Vec<DrawPrimitive>,
    min: [f32; 2],
    max: [f32; 2],
    summary: RecordingSummary,
    content_markers: u32,
}

impl Default for RecordingContent {
    fn default() -> Self {
        Self {
            others: Vec::new(),
            min: [f32::INFINITY; 2],
            max: [f32::NEG_INFINITY; 2],
            summary: RecordingSummary::default(),
            content_markers: 0,
        }
    }
}

impl Default for CommandRecording {
    fn default() -> Self {
        CommandRecorder::default().finish()
    }
}

impl PartialEq for CommandRecording {
    fn eq(&self, other: &Self) -> bool {
        self.shapes == other.shapes
            && self.content.others == other.content.others
            && self.content.content_markers == other.content.content_markers
    }
}

impl CommandRecording {
    /// Returns owned command data for further recording.
    /// Shape data is copied only when a retained reader still shares it.
    pub fn into_recorder(self) -> CommandRecorder {
        CommandRecorder {
            shapes: Arc::unwrap_or_clone(self.shapes),
            content: self.content,
        }
    }

    pub fn from_primitives(primitives: impl IntoIterator<Item = DrawPrimitive>) -> Self {
        CommandRecorder::from_primitives(primitives).finish()
    }

    pub fn shape_capacity(&self) -> usize {
        self.shapes.tables.shapes.capacity()
    }

    /// The heap the POD tables hold, capacity included.
    pub fn pod_heap_bytes(&self) -> usize {
        self.shapes.tables.heap_bytes()
    }

    /// The published shape columns and their brush, stop and segment tables.
    pub fn tables(&self) -> &RecordTables {
        self.shapes.tables()
    }

    /// Shared ownership of the immutable shape recording.
    pub fn shape_recorder(&self) -> &Arc<ShapeRecorder> {
        &self.shapes
    }

    /// The entries inside `segments` that draw, content markers left out.
    pub fn len_in(&self, segments: &Range<u32>) -> usize {
        self.segments_in(segments)
            .filter(|segment| segment.lane != RecordLane::Content)
            .map(|segment| segment.count as usize)
            .sum()
    }

    /// The shape columns, with complete records available on demand.
    pub fn shapes(&self) -> &ShapeRecords {
        &self.shapes.tables.shapes
    }

    pub fn brushes(&self) -> &[BrushRecord] {
        &self.shapes.tables.brushes
    }

    pub fn stops(&self) -> &[GradientStopRecord] {
        &self.shapes.tables.stops
    }

    pub fn others(&self) -> &[DrawPrimitive] {
        &self.content.others
    }

    pub fn segments(&self) -> &[RecordSegment] {
        &self.shapes.tables.segments
    }

    /// Every segment, the range a placement without a content split draws.
    pub fn all_segments(&self) -> Range<u32> {
        0..self.shapes.tables.segments.len() as u32
    }

    /// A rect containing every entry's coverage rect: exact for rects and
    /// the other lanes, the disc around the band for a scope-recorded arc.
    pub fn bounds(&self) -> Option<Rect> {
        let others = (self.content.min[0] <= self.content.max[0]
            && self.content.min[1] <= self.content.max[1])
            .then(|| Rect {
                x: self.content.min[0],
                y: self.content.min[1],
                width: self.content.max[0] - self.content.min[0],
                height: self.content.max[1] - self.content.min[1],
            });
        match (self.shapes.bounds(), others) {
            (Some(shapes), Some(others)) => Some(union_rect(shapes, others)),
            (shapes, others) => shapes.or(others),
        }
    }

    pub fn summary(&self) -> RecordingSummary {
        self.content.summary
    }

    pub fn content_markers(&self) -> u32 {
        self.content.content_markers
    }

    /// Entries of every lane, content markers included.
    pub fn len(&self) -> usize {
        self.shapes.tables.shapes.len()
            + self.content.others.len()
            + self.content.content_markers as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A hash over every entry, table and marker in recorded order, so two
    /// recordings with equal fingerprints draw the same pixels. Computed
    /// the first time it is asked for, so a recording nothing caches never
    /// pays for it.
    pub fn fingerprint(&self) -> u64 {
        *self.fingerprint.get_or_init(|| {
            let mut hasher = FxHasher::default();
            hasher.write_u64(self.shapes.fingerprint());
            for primitive in &self.content.others {
                hasher.write_u64(primitive.render_hash());
            }
            hasher.write_u32(self.content.content_markers);
            hasher.finish()
        })
    }

    /// The segments of `segments` that draw anything.
    pub fn is_empty_in(&self, segments: &Range<u32>) -> bool {
        !self
            .segments_in(segments)
            .any(|segment| segment.lane != RecordLane::Content && segment.count > 0)
    }

    pub fn segments_in(&self, segments: &Range<u32>) -> std::slice::Iter<'_, RecordSegment> {
        self.shapes.tables.segments[segments.start as usize..segments.end as usize].iter()
    }

    /// The summary of the entries inside `segments` only.
    pub fn summary_in(&self, segments: &Range<u32>) -> RecordingSummary {
        if *segments == self.all_segments() {
            return self.content.summary;
        }
        let mut summary = RecordingSummary::default();
        for segment in self.segments_in(segments) {
            match segment.lane {
                RecordLane::Shapes if segment.count > 0 => summary.has_non_shadow = true,
                RecordLane::Others => {
                    for primitive in &self.content.others[segment.range()] {
                        summary.note(primitive);
                    }
                }
                RecordLane::Shapes | RecordLane::Content => {}
            }
        }
        summary
    }

    /// The segments before the last content marker (`behind`) or after it;
    /// with no marker, behind is empty and overlay is everything.
    pub fn content_split(&self, behind: bool) -> Range<u32> {
        let last_marker = self
            .shapes
            .tables
            .segments
            .iter()
            .rposition(|segment| segment.lane == RecordLane::Content);
        match (last_marker, behind) {
            (Some(index), true) => 0..index as u32,
            (Some(index), false) => index as u32 + 1..self.shapes.tables.segments.len() as u32,
            (None, true) => 0..0,
            (None, false) => self.all_segments(),
        }
    }

    /// The coverage rect of every entry inside `segments`.
    pub fn coverage_rects(&self, segments: Range<u32>) -> impl Iterator<Item = Rect> + '_ {
        self.segments_in(&segments).flat_map(move |segment| {
            segment.range().filter_map(move |index| match segment.lane {
                RecordLane::Shapes => self
                    .shapes
                    .tables
                    .shapes
                    .get(index)
                    .map(|record| record.coverage_rect()),
                RecordLane::Others => primitive_coverage_rect(&self.content.others[index]),
                RecordLane::Content => None,
            })
        })
    }

    /// The primitives inside `segments`, materialised in recorded order,
    /// content markers left out.
    pub fn primitives(&self, segments: Range<u32>) -> impl Iterator<Item = DrawPrimitive> + '_ {
        self.segments_in(&segments)
            .flat_map(|segment| self.segment_primitives(segment, false))
    }

    /// Every primitive in recorded order, content markers included.
    pub fn primitives_with_markers(&self) -> impl Iterator<Item = DrawPrimitive> + '_ {
        self.shapes
            .tables
            .segments
            .iter()
            .flat_map(|segment| self.segment_primitives(segment, true))
    }

    pub fn into_primitives_with_markers(self) -> Vec<DrawPrimitive> {
        self.primitives_with_markers().collect()
    }

    fn segment_primitives<'a>(
        &'a self,
        segment: &RecordSegment,
        markers: bool,
    ) -> impl Iterator<Item = DrawPrimitive> + use<'a> {
        let lane = segment.lane;
        segment.range().filter_map(move |index| match lane {
            RecordLane::Shapes => Some(self.materialize_shape(index)),
            RecordLane::Others => Some(self.content.others[index].clone()),
            RecordLane::Content => markers.then_some(DrawPrimitive::Content),
        })
    }

    /// The exact [`DrawPrimitive`] the record was made from.
    pub fn materialize_shape(&self, index: usize) -> DrawPrimitive {
        let record = self
            .shapes
            .tables
            .shapes
            .get(index)
            .expect("recorded shape index");
        let rect = record.rect_value();
        let brush = self.brush_of(&record);
        let stroke = record.stroke();
        let primitive = match record.kind() {
            RECORD_KIND_ROUND_RECT => DrawPrimitive::RoundRect {
                rect,
                brush,
                radii: CornerRadii {
                    top_left: record.radii[0],
                    top_right: record.radii[1],
                    bottom_right: record.radii[2],
                    bottom_left: record.radii[3],
                },
                stroke,
            },
            RECORD_KIND_ARC => DrawPrimitive::Arc {
                rect,
                brush,
                center: Point::new(record.arc[0], record.arc[1]),
                radius: record.arc[2],
                start_angle: record.arc_band[0],
                sweep_angle: record.arc_band[1],
                stroke,
                inner_radius: record.arc[3],
            },
            _ => DrawPrimitive::Rect {
                rect,
                brush,
                stroke,
            },
        };
        let blend_mode = record.blend_mode();
        if blend_mode == BlendMode::SrcOver {
            primitive
        } else {
            DrawPrimitive::Blend {
                primitive: Box::new(primitive),
                blend_mode,
            }
        }
    }

    /// The brush of a record: its solid colour, or the gradient rebuilt
    /// from the tables exactly as the app gave it.
    pub fn brush_of(&self, record: &ShapeRecord) -> Brush {
        if record.brush == 0 {
            return Brush::Solid(Color(
                record.color[0],
                record.color[1],
                record.color[2],
                record.color[3],
            ));
        }
        let tables = &self.shapes.tables;
        let brush = &tables.brushes[record.brush as usize - 1];
        let colors = tables.stops
            [brush.stop_start as usize..(brush.stop_start + brush.stop_count) as usize]
            .iter()
            .map(|stop| Color(stop.color[0], stop.color[1], stop.color[2], stop.color[3]))
            .collect();
        let stops = (brush.explicit_len != NO_EXPLICIT_STOPS).then(|| {
            tables.explicit_stops[brush.explicit_start as usize
                ..(brush.explicit_start + brush.explicit_len) as usize]
                .to_vec()
        });
        let [a, b, c, d] = brush.params;
        let tile_mode = TILE_MODES[brush.tile_mode as usize];
        match brush.kind {
            BRUSH_KIND_RADIAL => Brush::RadialGradient {
                colors,
                stops,
                center: Point::new(a, b),
                radius: c,
                tile_mode,
            },
            BRUSH_KIND_SWEEP => Brush::SweepGradient {
                colors,
                stops,
                center: Point::new(a, b),
            },
            _ => Brush::LinearGradient {
                colors,
                stops,
                start: Point::new(a, b),
                end: Point::new(c, d),
                tile_mode,
            },
        }
    }
}

/// Mutable command data owned exclusively while a draw scope records it.
/// Publishing with [`Self::finish`] shares the completed shape data without copying it.
#[derive(Clone, Debug, Default)]
pub struct CommandRecorder {
    shapes: ShapeRecorder,
    content: RecordingContent,
}

impl CommandRecorder {
    /// Records primitives in order into owned command data.
    pub fn from_primitives(primitives: impl IntoIterator<Item = DrawPrimitive>) -> Self {
        let mut recorder = Self::default();
        for primitive in primitives {
            recorder.push_primitive(primitive);
        }
        recorder
    }

    /// Reuses a completed command's buffer capacities for an empty recording.
    /// Retained readers keep their original shape data.
    pub fn reusing(recording: CommandRecording) -> Self {
        let shapes = Arc::try_unwrap(recording.shapes).unwrap_or_else(|shared| ShapeRecorder {
            tables: shared.tables.with_capacity_of(),
            ..ShapeRecorder::default()
        });
        let mut recorder = Self {
            shapes,
            content: recording.content,
        };
        recorder.clear();
        recorder
    }

    /// Publishes completed command data without copying its shape columns.
    pub fn finish(self) -> CommandRecording {
        CommandRecording {
            shapes: Arc::new(self.shapes),
            content: self.content,
            fingerprint: OnceCell::new(),
        }
    }

    /// Every recorded entry, including content markers.
    pub fn len(&self) -> usize {
        self.shapes.tables.shapes.len()
            + self.content.others.len()
            + self.content.content_markers as usize
    }

    /// Whether this recorder contains no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The number of recorded content markers.
    pub fn content_markers(&self) -> u32 {
        self.content.content_markers
    }

    /// Empties the recording, keeping every buffer's capacity for the next
    /// recording into it.
    pub fn clear(&mut self) {
        self.shapes.clear();
        self.content.others.clear();
        self.content.min = [f32::INFINITY; 2];
        self.content.max = [f32::NEG_INFINITY; 2];
        self.content.summary = RecordingSummary::default();
        self.content.content_markers = 0;
    }

    /// Reserves capacity for additional shape records.
    pub fn reserve_shapes(&mut self, additional: usize) {
        self.shapes.tables_mut().shapes.reserve(additional);
    }

    /// Records a content marker at the current command position.
    pub fn push_content(&mut self) {
        self.content.content_markers += 1;
        self.shapes.push_content_segment();
    }

    /// Records a primitive the way the draw scope would have: shapes and
    /// blended shapes become records, everything else joins the others lane.
    pub fn push_primitive(&mut self, primitive: DrawPrimitive) {
        if matches!(primitive, DrawPrimitive::Content) {
            self.push_content();
            return;
        }
        match self.shapes.push_primitive(primitive) {
            Recorded::Shape(_) => self.note_shape(),
            Recorded::Other(other) => self.push_other(other),
        }
    }

    #[inline]
    fn note_shape(&mut self) {
        self.content.summary.has_non_shadow = true;
    }

    fn include_bounds(&mut self, rect: Rect) {
        self.content.min[0] = self.content.min[0].min(rect.x);
        self.content.min[1] = self.content.min[1].min(rect.y);
        self.content.max[0] = self.content.max[0].max(rect.x + rect.width);
        self.content.max[1] = self.content.max[1].max(rect.y + rect.height);
    }

    /// Records a rectangle with its brush, optional stroke and blend mode.
    pub fn push_rect(
        &mut self,
        rect: Rect,
        brush: &Brush,
        stroke: Option<Stroke>,
        blend: BlendMode,
    ) {
        self.shapes.push_rect(rect, brush, stroke, blend);
        self.note_shape();
    }

    /// Records a rounded rectangle with its corner radii and paint.
    pub fn push_round_rect(
        &mut self,
        rect: Rect,
        brush: &Brush,
        radii: CornerRadii,
        stroke: Option<Stroke>,
        blend: BlendMode,
    ) {
        self.shapes
            .push_round_rect(rect, brush, radii, stroke, blend);
        self.note_shape();
    }

    /// Records an arc band or annular sector with the rect the primitive
    /// carries.
    #[inline]
    pub fn push_arc(&mut self, rect: Rect, args: &ArcRecordArgs<'_>) {
        self.shapes.push_arc(rect, args);
        self.note_shape();
    }

    /// Records an arc the draw scope drew, whose band the scope already
    /// normalised: the record keeps the disc around the band as its rect
    /// and derives the primitive's tight bounds only when asked.
    #[inline]
    pub fn push_scope_arc(&mut self, args: &ArcRecordArgs<'_>, geometry: &ArcGeometry) {
        self.shapes.push_scope_arc(args, geometry);
        self.note_shape();
    }

    /// Records a primitive in the non-shape lane and updates its metadata.
    pub fn push_other(&mut self, primitive: DrawPrimitive) {
        self.content.summary.note(&primitive);
        if let Some(rect) = primitive_coverage_rect(&primitive) {
            self.include_bounds(rect);
        }
        let index = self.content.others.len() as u32;
        self.content.others.push(primitive);
        self.shapes
            .extend_segment(RecordLane::Others, index, BlendMode::SrcOver, false, 0);
    }

    /// Folds `other`'s summary into this recording's, for callers that
    /// combine recordings.
    pub fn merge_summary(&mut self, other: RecordingSummary) {
        self.content.summary.merge(other);
    }
}

/// The arc as the app drew it, the arguments of
/// [`CommandRecorder::push_arc`].
pub struct ArcRecordArgs<'a> {
    pub brush: &'a Brush,
    pub center: Point,
    pub radius: f32,
    pub start_angle: f32,
    pub sweep_angle: f32,
    pub stroke: Option<Stroke>,
    pub inner_radius: f32,
    pub blend_mode: BlendMode,
}

/// The ring a stroked round rect draws when it is a circle: a square
/// whose four radii are half its side, stroked. Its band is the stroke.
fn stroked_circle_ring(
    rect: Rect,
    radii: CornerRadii,
    stroke: Option<Stroke>,
) -> Option<ArcGeometry> {
    const CIRCLE_TOLERANCE: f32 = 0.01;
    let stroke = stroke?;
    if rect.width.to_bits() != rect.height.to_bits() || !stroke.is_visible() {
        return None;
    }
    let half = rect.width * 0.5;
    let radius = radii.top_left;
    if !radius.is_finite()
        || radius <= 0.0
        || (radius - half).abs() > CIRCLE_TOLERANCE
        || radii.top_right.to_bits() != radius.to_bits()
        || radii.bottom_right.to_bits() != radius.to_bits()
        || radii.bottom_left.to_bits() != radius.to_bits()
    {
        return None;
    }
    let half_width = stroke.half_width();
    Some(ArcGeometry::new(
        Point::new(rect.x + half, rect.y + half),
        (half - half_width).max(0.0),
        half + half_width,
        0.0,
        TAU,
        StrokeCap::Round,
    ))
}

fn union_rect(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect {
        x,
        y,
        width: (a.x + a.width).max(b.x + b.width) - x,
        height: (a.y + a.height).max(b.y + b.height) - y,
    }
}

fn rect_row(rect: Rect) -> [f32; 4] {
    [rect.x, rect.y, rect.width, rect.height]
}

fn row_rect(row: [f32; 4]) -> Rect {
    Rect {
        x: row[0],
        y: row[1],
        width: row[2],
        height: row[3],
    }
}

/// The band a primitive's arc arguments describe, normalised as the
/// fragment stage draws it.
#[inline(always)]
pub fn normalized_band(args: &ArcRecordArgs<'_>) -> ArcGeometry {
    let (band_inner, band_outer, cap) = arc_band(args.radius, args.inner_radius, args.stroke);
    ArcGeometry::new(
        args.center,
        band_inner,
        band_outer,
        args.start_angle,
        args.sweep_angle,
        cap,
    )
}

/// The disc around a band: the square of its outer radius plus the cap's
/// reach, which contains the band's tight bounds.
fn band_disc(geometry: &ArcGeometry) -> Rect {
    let reach = geometry.outer_radius + geometry.half_thickness();
    Rect {
        x: geometry.center.x - reach,
        y: geometry.center.y - reach,
        width: reach + reach,
        height: reach + reach,
    }
}

/// The arc row the fragment stage reads: the mid-angle sine and cosine and
/// the half-sweep sine and cosine, with the full circle's sentinel.
pub fn arc_trig(geometry: &ArcGeometry) -> [f32; 4] {
    ArcTrigCache::default().resolve(geometry)
}

#[cfg(test)]
#[path = "tests/record_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/record_band_tests.rs"]
mod band_tests;
