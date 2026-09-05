use std::{
    cell::OnceCell,
    hash::{Hash, Hasher},
    ops::Range,
    sync::{Arc, OnceLock},
};

use bytemuck::{Pod, Zeroable};

use crate::{
    ArcGeometry, BlendMode, Brush, Color, CornerRadii, DrawPrimitive, FxHasher, Point, Rect,
    RenderHash, ShapeRecords, Stroke, StrokeCap, StrokeJoin, TAU, TileMode, arc_band,
    arc_trig_cache::ArcTrigCache,
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
        if self.kind() == RECORD_KIND_ARC {
            FRAGMENT_KIND_ARC
        } else if self.is_stroked() {
            FRAGMENT_KIND_STROKE
        } else {
            FRAGMENT_KIND_FILL
        }
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
        Rect {
            x: self.rect[0],
            y: self.rect[1],
            width: self.rect[2],
            height: self.rect[3],
        }
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
        last.kinds |= opened.kinds;
        last.band_class = last.band_class.max(opened.band_class);
        return;
    }
    tables.segments.push(opened)
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

    fn push_shape_primitive(
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
            ShapeRecord {
                rect: rect_row(rect),
                radii: [0.0; 4],
                color,
                stroke_width: stroke.map_or(0.0, |stroke| stroke.width),
                flags: pack_flags(RECORD_KIND_RECT, stroke, blend, StrokeCap::Butt),
                brush: handle,
                reserved: 0,
                arc: [0.0; 4],
                arc_band: [0.0; 4],
                arc_normalized: [0.0; 4],
            },
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
        let mut arc = [0.0; 4];
        let mut arc_band = [0.0; 4];
        let mut arc_normalized = [0.0; 4];
        let mut bucket = None;
        if let Some(ring) = stroked_circle_ring(rect, radii, stroke)
            && let band = BandRing::of_geometry(&ring)
            && let Some(ring_bucket) =
                band_bucket_for(&ring, &band, expand_rect(rect, ring.half_thickness()))
        {
            flags |= ARC_BANDED_BIT;
            arc = [
                ring.center.x,
                ring.center.y,
                ring.mid_radius(),
                ring.inner_radius,
            ];
            arc_band = [0.0, TAU, ring.inner_radius, ring.outer_radius];
            arc_normalized = [0.0, TAU, band.range_start, band.range];
            bucket = Some(ring_bucket);
        }
        self.push_shape(
            ShapeRecord {
                rect: rect_row(rect),
                radii: [
                    radii.top_left,
                    radii.top_right,
                    radii.bottom_right,
                    radii.bottom_left,
                ],
                color,
                stroke_width: stroke.map_or(0.0, |stroke| stroke.width),
                flags,
                brush: handle,
                reserved: 0,
                arc,
                arc_band,
                arc_normalized,
            },
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
            ShapeRecord {
                rect: rect_row(rect),
                radii,
                color,
                stroke_width: args.stroke.map_or(0.0, |stroke| stroke.width),
                flags,
                brush: handle,
                reserved: 0,
                arc: [args.center.x, args.center.y, args.radius, args.inner_radius],
                arc_band: [
                    args.start_angle,
                    args.sweep_angle,
                    geometry.inner_radius,
                    geometry.outer_radius,
                ],
                arc_normalized: [
                    geometry.start_angle,
                    geometry.sweep_angle,
                    ring.range_start,
                    ring.range,
                ],
            },
            bucket,
        )
    }

    /// Appends `record`, stamped with `band_bucket` as its band class when
    /// it draws as a band, and extends the open segment or opens one. A
    /// segment is one draw at its largest band class's strip, in record
    /// order; it takes a record of another class while the quads that
    /// strip leaves pinned stay within [`SEGMENT_WASTE_QUADS`], and is cut
    /// past that. The tables are taken once per record.
    #[inline]
    fn push_shape(&mut self, mut record: ShapeRecord, band_bucket: Option<usize>) -> Rect {
        let coverage = expand_rect(record.stored_rect(), record.half_stroke());
        self.include_bounds(coverage);
        let index = self.tables.shapes.len() as u32;
        let blend = record.blend_mode();
        let gradient = record.is_gradient();
        let kind_bit = 1u8 << record.fragment_kind();
        let band_class = band_bucket.unwrap_or(0) as u8;
        record.flags |= u32::from(band_class) << BAND_CLASS_SHIFT;
        let extend = self.note_segment_key(RecordLane::Shapes, blend, gradient)
            && self.segment_takes_class(band_class);
        if !extend {
            self.segment_waste = 0;
        }
        let tables = self.tables_mut();
        tables.shapes.push(record);
        extend_segment_in(
            tables,
            extend,
            RecordSegment {
                lane: RecordLane::Shapes,
                start: index,
                count: 1,
                blend,
                gradient,
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
        self.segments_in(&segments)
            .flat_map(|segment| -> Box<dyn Iterator<Item = Rect> + '_> {
                match segment.lane {
                    RecordLane::Shapes => Box::new(
                        self.shapes
                            .tables
                            .shapes
                            .iter()
                            .skip(segment.start as usize)
                            .take(segment.count as usize)
                            .map(|record| record.coverage_rect()),
                    ),
                    RecordLane::Others => Box::new(
                        self.content.others[segment.range()]
                            .iter()
                            .filter_map(primitive_coverage_rect),
                    ),
                    RecordLane::Content => Box::new(std::iter::empty()),
                }
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

    fn segment_primitives(
        &self,
        segment: &RecordSegment,
        markers: bool,
    ) -> Box<dyn Iterator<Item = DrawPrimitive> + '_> {
        match segment.lane {
            RecordLane::Shapes => Box::new(
                segment
                    .range()
                    .map(move |index| self.materialize_shape(index)),
            ),
            RecordLane::Others => Box::new(self.content.others[segment.range()].iter().cloned()),
            RecordLane::Content if markers => Box::new(std::iter::repeat_n(
                DrawPrimitive::Content,
                segment.count as usize,
            )),
            RecordLane::Content => Box::new(std::iter::empty()),
        }
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

/// The band a primitive's arc arguments describe, normalised as the
/// fragment stage draws it.
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
mod tests {
    use super::*;
    use crate::{
        DrawScope, DrawScopeDefault, DrawTextStyle, ImageBitmap, ImageSampling, Size, TextPrimitive,
    };

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn solid() -> Brush {
        Brush::Solid(Color(0.1, 0.2, 0.3, 0.4))
    }

    fn linear_explicit() -> Brush {
        Brush::LinearGradient {
            colors: vec![Color::RED, Color::GREEN, Color::BLUE],
            stops: Some(vec![0.0, 0.25, 1.0]),
            start: Point::new(1.0, 2.0),
            end: Point::new(3.0, 4.0),
            tile_mode: TileMode::Repeated,
        }
    }

    fn linear_mismatched_stops() -> Brush {
        Brush::LinearGradient {
            colors: vec![Color::RED, Color::BLUE],
            stops: Some(vec![0.5]),
            start: Point::new(0.0, 0.0),
            end: Point::new(0.0, 10.0),
            tile_mode: TileMode::Clamp,
        }
    }

    fn radial() -> Brush {
        Brush::RadialGradient {
            colors: vec![Color::WHITE, Color::BLACK],
            stops: None,
            center: Point::new(5.0, 6.0),
            radius: 7.0,
            tile_mode: TileMode::Mirror,
        }
    }

    fn sweep() -> Brush {
        Brush::SweepGradient {
            colors: vec![Color::RED],
            stops: Some(vec![]),
            center: Point::new(8.0, 9.0),
        }
    }

    fn image() -> ImageBitmap {
        ImageBitmap::from_rgba8(1, 1, vec![255, 0, 0, 255]).expect("a one-pixel image")
    }

    fn text() -> DrawPrimitive {
        DrawPrimitive::Text(Box::new(TextPrimitive {
            rect: rect(1.0, 1.0, 20.0, 10.0),
            text: "hi".into(),
            style: DrawTextStyle::default(),
            color: Color::WHITE,
        }))
    }

    fn every_primitive() -> Vec<DrawPrimitive> {
        let stroke = Stroke {
            width: 3.0,
            cap: StrokeCap::Round,
            join: StrokeJoin::Bevel,
        };
        vec![
            DrawPrimitive::Rect {
                rect: rect(0.0, 0.0, 10.0, 10.0),
                brush: solid(),
                stroke: None,
            },
            DrawPrimitive::Rect {
                rect: rect(1.0, 2.0, 3.0, 4.0),
                brush: linear_explicit(),
                stroke: Some(stroke),
            },
            DrawPrimitive::RoundRect {
                rect: rect(5.0, 5.0, 20.0, 10.0),
                brush: radial(),
                radii: CornerRadii {
                    top_left: 1.0,
                    top_right: 2.0,
                    bottom_right: 3.0,
                    bottom_left: 4.0,
                },
                stroke: Some(Stroke::new(1.0)),
            },
            DrawPrimitive::Arc {
                rect: rect(-1.0, -1.0, 2.0, 2.0),
                brush: sweep(),
                center: Point::new(0.0, 0.0),
                radius: 5.0,
                start_angle: 1.0,
                sweep_angle: -2.0,
                stroke: Some(stroke),
                inner_radius: 0.0,
            },
            DrawPrimitive::Arc {
                rect: rect(-5.0, -5.0, 10.0, 10.0),
                brush: linear_mismatched_stops(),
                center: Point::new(0.0, 0.0),
                radius: 5.0,
                start_angle: 0.0,
                sweep_angle: crate::TAU,
                stroke: None,
                inner_radius: 2.0,
            },
            DrawPrimitive::Arc {
                rect: rect(0.0, 0.0, 0.0, 0.0),
                brush: solid(),
                center: Point::new(0.0, 0.0),
                radius: 5.0,
                start_angle: 0.0,
                sweep_angle: 0.0,
                stroke: None,
                inner_radius: 0.0,
            },
            DrawPrimitive::Blend {
                primitive: Box::new(DrawPrimitive::Rect {
                    rect: rect(0.0, 0.0, 1.0, 1.0),
                    brush: solid(),
                    stroke: None,
                }),
                blend_mode: BlendMode::Plus,
            },
            DrawPrimitive::Blend {
                primitive: Box::new(DrawPrimitive::Blend {
                    primitive: Box::new(DrawPrimitive::Rect {
                        rect: rect(0.0, 0.0, 1.0, 1.0),
                        brush: solid(),
                        stroke: None,
                    }),
                    blend_mode: BlendMode::Xor,
                }),
                blend_mode: BlendMode::Luminosity,
            },
            DrawPrimitive::Blend {
                primitive: Box::new(DrawPrimitive::Image {
                    rect: rect(0.0, 0.0, 1.0, 1.0),
                    image: image(),
                    alpha: 0.5,
                    color_filter: None,
                    sampling: ImageSampling::Linear,
                    src_rect: None,
                }),
                blend_mode: BlendMode::Screen,
            },
            DrawPrimitive::Image {
                rect: rect(2.0, 2.0, 4.0, 4.0),
                image: image(),
                alpha: 1.0,
                color_filter: None,
                sampling: ImageSampling::Nearest,
                src_rect: Some(rect(0.0, 0.0, 1.0, 1.0)),
            },
            text(),
            DrawPrimitive::Shadow(crate::ShadowPrimitive::Drop {
                shape: Box::new(DrawPrimitive::Rect {
                    rect: rect(0.0, 0.0, 1.0, 1.0),
                    brush: solid(),
                    stroke: None,
                }),
                cutout: None,
                blur_radius: 2.0,
                blend_mode: BlendMode::SrcOver,
            }),
            DrawPrimitive::Content,
            DrawPrimitive::Rect {
                rect: rect(9.0, 9.0, 1.0, 1.0),
                brush: solid(),
                stroke: None,
            },
        ]
    }

    fn scan_summary(primitives: &[DrawPrimitive]) -> RecordingSummary {
        let mut summary = RecordingSummary::default();
        for primitive in primitives {
            summary.note(primitive);
        }
        summary
    }

    #[test]
    fn every_primitive_round_trips_through_the_record_byte_for_byte() {
        let primitives = every_primitive();
        let recording = CommandRecording::from_primitives(primitives.clone());
        assert_eq!(recording.into_primitives_with_markers(), primitives);
    }

    #[test]
    fn shapes_and_blended_shapes_are_records_everything_else_is_not() {
        let recording = CommandRecording::from_primitives(every_primitive());
        assert_eq!(
            recording.shapes().len(),
            8,
            "six shapes, one blended, one after content"
        );
        assert_eq!(
            recording.others().len(),
            5,
            "a nested blend, a blended image, an image, a text and a shadow"
        );
        assert_eq!(recording.content_markers(), 1);
        assert_eq!(recording.len(), 14);
    }

    #[test]
    fn the_scope_records_the_arc_bounds_the_primitive_used_to_carry() {
        let center = Point::new(50.0, 40.0);
        let stroke = Stroke::new(4.0);
        let mut scope = DrawScopeDefault::new(Size::new(100.0, 100.0));
        scope.draw_arc(solid(), center, 30.0, 0.5, -1.5, stroke);
        scope.draw_annular_sector(radial(), center, 10.0, 20.0, 0.0, 2.0);
        scope.draw_arc(solid(), center, 30.0, 0.5, 0.0, stroke);
        let (band_inner, band_outer, cap) = arc_band(30.0, 0.0, Some(stroke));
        let stroked_bounds =
            ArcGeometry::new(center, band_inner, band_outer, 0.5, -1.5, cap).bounds();
        let sector_bounds =
            ArcGeometry::new(center, 10.0, 20.0, 0.0, 2.0, StrokeCap::Butt).bounds();
        assert_eq!(
            scope.into_primitives(),
            vec![
                DrawPrimitive::Arc {
                    rect: stroked_bounds,
                    brush: solid(),
                    center,
                    radius: 30.0,
                    start_angle: 0.5,
                    sweep_angle: -1.5,
                    stroke: Some(stroke),
                    inner_radius: 0.0,
                },
                DrawPrimitive::Arc {
                    rect: sector_bounds,
                    brush: radial(),
                    center,
                    radius: 20.0,
                    start_angle: 0.0,
                    sweep_angle: 2.0,
                    stroke: None,
                    inner_radius: 10.0,
                },
            ],
            "a zero sweep records nothing, as the scope always did"
        );
    }

    #[test]
    fn the_arc_record_carries_the_normalised_band_the_fragment_stage_reads() {
        let recording = CommandRecording::from_primitives(vec![DrawPrimitive::Arc {
            rect: rect(0.0, 0.0, 1.0, 1.0),
            brush: solid(),
            center: Point::new(3.0, 4.0),
            radius: 10.0,
            start_angle: 1.0,
            sweep_angle: -2.0,
            stroke: Some(Stroke::new(4.0).with_cap(StrokeCap::Square)),
            inner_radius: 0.0,
        }]);
        let record = recording.shapes().get(0).unwrap();
        let geometry = record.arc_geometry().expect("an arc");
        let expected = ArcGeometry::new(
            Point::new(3.0, 4.0),
            8.0,
            12.0,
            1.0,
            -2.0,
            StrokeCap::Square,
        );
        assert_eq!(geometry, expected);
        assert_eq!(
            record.radii,
            arc_trig(&expected),
            "the trig row the fragment stage reads is computed once, when recorded"
        );
        assert_eq!(
            record.radii[1],
            (expected.start_angle + expected.sweep_angle * 0.5).cos()
        );
        let ring = BandRing::of_geometry(&expected);
        assert_eq!(
            record.arc_normalized[2..],
            [ring.range_start, ring.range],
            "the strip's padded sweep the vertex stage reads is computed once, when recorded"
        );
        assert!(ring.range_start < expected.start_angle && ring.range > 2.0);
        assert!(!record.is_degenerate_arc());
        assert!(!record.has_loose_rect());
        assert_eq!(record.rect_value(), rect(0.0, 0.0, 1.0, 1.0));
        let degenerate = CommandRecording::from_primitives(vec![DrawPrimitive::Arc {
            rect: rect(0.0, 0.0, 1.0, 1.0),
            brush: solid(),
            center: Point::new(3.0, 4.0),
            radius: 10.0,
            start_angle: 1.0,
            sweep_angle: 0.0,
            stroke: None,
            inner_radius: 0.0,
        }]);
        assert!(degenerate.shapes().get(0).unwrap().is_degenerate_arc());
    }

    #[test]
    fn segments_cut_on_blend_and_brush_class_and_lane_never_on_kind() {
        let mut recording = CommandRecorder::default();
        recording.push_rect(rect(0.0, 0.0, 1.0, 1.0), &solid(), None, BlendMode::SrcOver);
        recording.push_round_rect(
            rect(0.0, 0.0, 1.0, 1.0),
            &solid(),
            CornerRadii::uniform(1.0),
            Some(Stroke::new(1.0)),
            BlendMode::SrcOver,
        );
        recording.push_arc(
            rect(0.0, 0.0, 1.0, 1.0),
            &ArcRecordArgs {
                brush: &solid(),
                center: Point::new(0.0, 0.0),
                radius: 5.0,
                start_angle: 0.0,
                sweep_angle: 1.0,
                stroke: None,
                inner_radius: 1.0,
                blend_mode: BlendMode::SrcOver,
            },
        );
        recording.push_rect(
            rect(0.0, 0.0, 1.0, 1.0),
            &radial(),
            None,
            BlendMode::SrcOver,
        );
        recording.push_rect(rect(0.0, 0.0, 1.0, 1.0), &solid(), None, BlendMode::Plus);
        recording.push_other(text());
        recording.push_other(text());
        recording.push_content();
        recording.push_rect(rect(0.0, 0.0, 1.0, 1.0), &solid(), None, BlendMode::SrcOver);
        let recording = recording.finish();
        let lanes: Vec<(RecordLane, u32, u32, BlendMode, bool, u8)> = recording
            .segments()
            .iter()
            .map(|segment| {
                (
                    segment.lane,
                    segment.start,
                    segment.count,
                    segment.blend,
                    segment.gradient,
                    segment.kinds,
                )
            })
            .collect();
        assert_eq!(
            lanes,
            vec![
                (RecordLane::Shapes, 0, 3, BlendMode::SrcOver, false, 0b111),
                (RecordLane::Shapes, 3, 1, BlendMode::SrcOver, true, 0b1),
                (RecordLane::Shapes, 4, 1, BlendMode::Plus, false, 0b1),
                (RecordLane::Others, 0, 2, BlendMode::SrcOver, false, 0),
                (RecordLane::Content, 0, 1, BlendMode::SrcOver, false, 0),
                (RecordLane::Shapes, 5, 1, BlendMode::SrcOver, false, 0b1),
            ]
        );
        assert_eq!(recording.segments()[0].uniform_kind(), None);
        assert_eq!(
            recording.segments()[1].uniform_kind(),
            Some(FRAGMENT_KIND_FILL)
        );
    }

    #[test]
    fn the_content_split_follows_the_last_marker() {
        let recording = CommandRecording::from_primitives(vec![
            DrawPrimitive::Rect {
                rect: rect(1.0, 0.0, 1.0, 1.0),
                brush: solid(),
                stroke: None,
            },
            DrawPrimitive::Content,
            DrawPrimitive::Rect {
                rect: rect(2.0, 0.0, 1.0, 1.0),
                brush: solid(),
                stroke: None,
            },
            DrawPrimitive::Content,
            DrawPrimitive::Rect {
                rect: rect(3.0, 0.0, 1.0, 1.0),
                brush: solid(),
                stroke: None,
            },
        ]);
        let xs = |segments: Range<u32>| -> Vec<f32> {
            recording
                .primitives(segments)
                .map(|primitive| match primitive {
                    DrawPrimitive::Rect { rect, .. } => rect.x,
                    other => panic!("unexpected {other:?}"),
                })
                .collect()
        };
        assert_eq!(xs(recording.content_split(true)), [1.0, 2.0]);
        assert_eq!(xs(recording.content_split(false)), [3.0]);
        assert_eq!(xs(recording.all_segments()), [1.0, 2.0, 3.0]);
        let unsplit = CommandRecording::from_primitives(vec![DrawPrimitive::Rect {
            rect: rect(4.0, 0.0, 1.0, 1.0),
            brush: solid(),
            stroke: None,
        }]);
        assert!(unsplit.is_empty_in(&unsplit.content_split(true)));
        assert_eq!(unsplit.content_split(false), unsplit.all_segments());
        assert_eq!(unsplit.len_in(&unsplit.all_segments()), 1);
    }

    #[test]
    fn summary_and_bounds_are_what_a_scan_of_the_primitives_finds() {
        let primitives = every_primitive();
        let recording = CommandRecording::from_primitives(primitives.clone());
        assert_eq!(recording.summary(), scan_summary(&primitives));
        let expected_bounds = primitives
            .iter()
            .filter_map(primitive_coverage_rect)
            .reduce(|a, b| a.union(b));
        assert_eq!(recording.bounds(), expected_bounds);
        assert_eq!(
            recording.summary_in(&recording.content_split(false)),
            scan_summary(&primitives[primitives.len() - 1..])
        );
        let rects: Vec<Rect> = recording.coverage_rects(recording.all_segments()).collect();
        let expected: Vec<Rect> = primitives
            .iter()
            .filter_map(primitive_coverage_rect)
            .collect();
        assert_eq!(rects, expected);
    }

    #[test]
    fn a_scope_arc_keeps_the_disc_and_derives_the_tight_bounds() {
        let center = Point::new(50.0, 40.0);
        let mut scope = DrawScopeDefault::new(Size::new(100.0, 100.0));
        scope.draw_arc(solid(), center, 30.0, 0.5, 1.0, Stroke::new(4.0));
        let recording = scope.finish();
        let record = recording.shapes().get(0).unwrap();
        assert!(record.has_loose_rect());
        let tight = ArcGeometry::new(center, 28.0, 32.0, 0.5, 1.0, StrokeCap::Butt).bounds();
        assert_eq!(record.rect_value(), tight);
        assert_eq!(record.coverage_rect(), expand_rect(tight, 2.0));
        let stored = record.stored_rect();
        assert!(stored.x <= tight.x && stored.y <= tight.y);
        assert!(stored.x + stored.width >= tight.x + tight.width);
        assert!(stored.y + stored.height >= tight.y + tight.height);
        let bounds = recording.bounds().expect("one arc gives bounds");
        assert_eq!(bounds, expand_rect(stored, 2.0));
    }

    #[test]
    fn a_shadow_only_recording_summarises_as_shadow() {
        let recording = CommandRecording::from_primitives(vec![DrawPrimitive::Shadow(
            crate::ShadowPrimitive::Drop {
                shape: Box::new(DrawPrimitive::Rect {
                    rect: rect(0.0, 0.0, 1.0, 1.0),
                    brush: solid(),
                    stroke: None,
                }),
                cutout: None,
                blur_radius: 1.0,
                blend_mode: BlendMode::SrcOver,
            },
        )]);
        assert_eq!(
            recording.summary(),
            RecordingSummary {
                has_shadow: true,
                ..RecordingSummary::default()
            }
        );
        assert_eq!(recording.bounds(), None);
    }

    #[test]
    fn the_fingerprint_sees_every_stop_the_order_and_the_blend() {
        let base = || CommandRecording::from_primitives(every_primitive());
        assert_eq!(base().fingerprint(), base().fingerprint());
        let mut primitives = every_primitive();
        let DrawPrimitive::Rect { brush, .. } = &mut primitives[1] else {
            unreachable!()
        };
        let Brush::LinearGradient { colors, .. } = brush else {
            unreachable!()
        };
        colors[1] = Color::WHITE;
        let recoloured_stop = CommandRecording::from_primitives(primitives);
        assert_ne!(recoloured_stop.fingerprint(), base().fingerprint());
        assert_eq!(
            recoloured_stop.shapes(),
            base().shapes(),
            "the record itself is unchanged by a stop colour, which is why the fingerprint must cover the stops"
        );
        let mut primitives = every_primitive();
        let DrawPrimitive::Rect { brush, .. } = &mut primitives[1] else {
            unreachable!()
        };
        let Brush::LinearGradient { stops, .. } = brush else {
            unreachable!()
        };
        *stops = Some(vec![0.0, 0.5, 1.0]);
        assert_ne!(
            CommandRecording::from_primitives(primitives).fingerprint(),
            base().fingerprint()
        );
        let mut primitives = every_primitive();
        primitives.swap(0, 1);
        assert_ne!(
            CommandRecording::from_primitives(primitives).fingerprint(),
            base().fingerprint()
        );
        let mut primitives = every_primitive();
        let DrawPrimitive::Blend { blend_mode, .. } = &mut primitives[6] else {
            unreachable!()
        };
        *blend_mode = BlendMode::Screen;
        assert_ne!(
            CommandRecording::from_primitives(primitives).fingerprint(),
            base().fingerprint()
        );
        let mut primitives = every_primitive();
        primitives.pop();
        assert_ne!(
            CommandRecording::from_primitives(primitives).fingerprint(),
            base().fingerprint()
        );
    }

    #[test]
    fn clearing_keeps_the_capacity_and_forgets_the_content() {
        let recording = CommandRecording::from_primitives(every_primitive());
        let capacity = recording.shape_capacity();
        let fingerprint = recording.fingerprint();
        let recording = CommandRecorder::reusing(recording).finish();
        assert!(recording.is_empty());
        assert_eq!(recording.shape_capacity(), capacity);
        assert_eq!(recording.segments().len(), 0);
        assert_eq!(recording.bounds(), None);
        assert_eq!(recording.summary(), RecordingSummary::default());
        assert_ne!(recording.fingerprint(), fingerprint);
        assert_eq!(
            recording.fingerprint(),
            CommandRecording::default().fingerprint()
        );
    }

    #[test]
    fn publishing_and_unique_reuse_move_the_shape_columns() {
        let mut recorder = CommandRecorder::default();
        assert!(recorder.is_empty());
        recorder.reserve_shapes(512);
        recorder.push_primitive(every_primitive().remove(0));
        recorder.push_content();
        assert_eq!(recorder.len(), 2);
        assert_eq!(recorder.content_markers(), 1);
        let body_pointer = recorder.shapes.tables.shapes.bodies().as_ptr();
        let published = recorder.finish();
        assert_eq!(published.shapes().bodies().as_ptr(), body_pointer);
        let capacity = published.shape_capacity();
        let mut reused = CommandRecorder::reusing(published);
        assert!(reused.is_empty());
        assert_eq!(reused.content_markers(), 0);
        reused.push_primitive(every_primitive().remove(0));
        let published = reused.finish();
        assert_eq!(published.len(), 1);
        assert_eq!(published.shape_capacity(), capacity);
        assert_eq!(published.shapes().bodies().as_ptr(), body_pointer);
    }

    #[test]
    fn the_record_is_seven_rows() {
        assert_eq!(std::mem::size_of::<ShapeRecord>(), 112);
        assert_eq!(std::mem::size_of::<BrushRecord>(), 48);
        assert_eq!(std::mem::size_of::<GradientStopRecord>(), 32);
    }
}

#[cfg(test)]
mod band_tests {
    use super::*;
    use crate::{DrawScope, DrawScopeDefault, Size};

    #[test]
    fn wide_arcs_are_banded_by_sweep_and_radius_and_narrow_ones_stay_quads() {
        let mut scope = DrawScopeDefault::new(Size::new(400.0, 400.0));
        let brush = Brush::Solid(Color::WHITE);
        let center = Point::new(200.0, 200.0);
        scope.draw_annular_sector(brush.clone(), center, 4.0, 8.0, 0.0, 1.0);
        scope.draw_annular_sector(brush.clone(), center, 10.0, 20.0, 0.0, 1.0);
        scope.draw_arc(brush.clone(), center, 50.0, 0.0, 1.0, Stroke::new(3.0));
        scope.draw_arc(brush.clone(), center, 200.0, 0.0, 1.0, Stroke::new(3.0));
        scope.draw_arc(brush.clone(), center, 500.0, 0.0, 1.0, Stroke::new(3.0));
        scope.draw_annular_sector(brush.clone(), center, 0.0, 40.0, 0.0, 3.0);
        scope.draw_annular_sector(brush, center, 30.0, 40.0, 0.0, 0.05);
        let recording = scope.finish();
        let banded: Vec<bool> = recording
            .shapes()
            .iter()
            .map(|record| record.is_banded())
            .collect();
        assert_eq!(
            banded,
            [false, true, true, true, true, false, true],
            "a disc stays a quad: its strip would be the disc and more; a sliver's \
             strip beats the disc the quad path would draw"
        );
        let segments: Vec<u32> = recording
            .shapes()
            .iter()
            .map(|record| record.band_segments())
            .collect();
        assert_eq!(
            segments[1..5],
            [4, 4, 8, 16],
            "a band takes the segments its padded sweep needs at the ring step of its radius"
        );
        assert_eq!(
            segments[6], 2,
            "the wide band needs two segments to cover its padded sweep"
        );
        let classes: Vec<usize> = recording
            .shapes()
            .iter()
            .map(|record| record.band_class())
            .collect();
        assert_eq!(classes, [0, 2, 2, 3, 4, 0, 1]);
        let segment_classes: Vec<u8> = recording
            .tables()
            .segments
            .iter()
            .map(|segment| segment.band_class)
            .collect();
        assert_eq!(
            segment_classes,
            [4],
            "a few records of mixed classes share one segment at the largest \
             class, so one draw keeps record order"
        );
        assert_eq!(band_bucket(1), 0);
        assert_eq!(band_bucket(64), ARC_BUCKETS - 1);
    }

    #[test]
    fn a_strip_pattern_stays_within_its_vertices_including_a_single_segment() {
        for segments in ARC_BUCKET_SEGMENTS {
            let indices: Vec<u32> = strip_index_pattern(segments).collect();
            assert_eq!(indices.len() as u32, strip_indices(segments));
            assert_eq!(
                indices.iter().max().copied(),
                Some(strip_vertices(segments) - 1)
            );
        }
        let ring = BandRing::new(20.0, 22.0, 0.0, 0.01);
        assert_eq!(ring.segments(), BAND_MIN_SEGMENTS);
        assert_eq!(band_bucket(BAND_MIN_SEGMENTS), 0);
    }

    #[test]
    fn a_segment_is_cut_where_its_largest_class_would_collapse_more_than_a_draw_is_worth() {
        let mut scope = DrawScopeDefault::new(Size::new(2000.0, 2000.0));
        let brush = Brush::Solid(Color::WHITE);
        let quad = Rect {
            x: 1.0,
            y: 1.0,
            width: 4.0,
            height: 4.0,
        };
        let rects = 40;
        for _ in 0..rects {
            scope.draw_rect_at(quad, brush.clone());
        }
        scope.draw_arc(
            brush.clone(),
            Point::new(500.0, 500.0),
            400.0,
            0.0,
            TAU,
            Stroke::new(3.0),
        );
        for _ in 0..rects {
            scope.draw_rect_at(quad, brush.clone());
        }
        let recording = scope.finish();
        let ring = recording.shapes().get(rects).unwrap();
        assert!(ring.is_banded());
        let ring_quads = ring.band_segments();
        assert!(ring_quads > 1);
        let segments: Vec<(u32, u8)> = recording
            .tables()
            .segments
            .iter()
            .map(|segment| (segment.count, segment.band_class))
            .collect();
        let after = SEGMENT_WASTE_QUADS / (ring_quads - 1);
        assert_eq!(
            segments,
            [
                (rects as u32, 0),
                (1 + after, ring.band_class() as u8),
                (rects as u32 - after, 0)
            ],
            "the ring would collapse {rects} quads times {} vertices each, more than a draw is \
             worth, so it opens a segment; the rects after it join until their own collapse \
             passes the budget",
            ring_quads - 1
        );
    }

    #[test]
    fn a_stroked_circle_is_a_band_and_a_stroked_pill_is_not() {
        let mut scope = DrawScopeDefault::new(Size::new(400.0, 400.0));
        let brush = Brush::Solid(Color::WHITE);
        let square = Rect {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 100.0,
        };
        scope.draw_round_rect_at_stroked(
            square,
            brush.clone(),
            CornerRadii::uniform(50.0),
            Stroke::new(4.0),
        );
        scope.draw_round_rect_at_stroked(
            square,
            brush.clone(),
            CornerRadii::uniform(20.0),
            Stroke::new(4.0),
        );
        scope.draw_round_rect_at(square, brush.clone(), CornerRadii::uniform(50.0));
        scope.draw_round_rect_at_stroked(
            Rect {
                x: 10.0,
                y: 10.0,
                width: 100.0,
                height: 60.0,
            },
            brush,
            CornerRadii::uniform(30.0),
            Stroke::new(4.0),
        );
        let recording = scope.finish();
        let banded: Vec<bool> = recording
            .shapes()
            .iter()
            .map(|record| record.is_banded())
            .collect();
        assert_eq!(banded, [true, false, false, false]);
        let ring = recording.shapes().get(0).unwrap();
        assert_eq!(ring.kind(), RECORD_KIND_ROUND_RECT);
        assert_eq!(ring.fragment_kind(), FRAGMENT_KIND_STROKE);
        assert_eq!(ring.arc, [60.0, 60.0, 50.0, 48.0]);
        assert_eq!(ring.arc_band, [0.0, TAU, 48.0, 52.0]);
        assert_eq!(ring.band_segments(), 16);
        assert_eq!(ring.band_class(), 4);
    }

    #[test]
    fn the_tables_are_shared_until_recorded_into_again() {
        let recording = CommandRecording::from_primitives(vec![DrawPrimitive::Rect {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            brush: Brush::Solid(Color::WHITE),
            stroke: None,
        }]);
        let held = Arc::clone(recording.shape_recorder());
        let recording = CommandRecorder::reusing(recording).finish();
        assert!(!Arc::ptr_eq(&held, recording.shape_recorder()));
        assert_eq!(held.tables().shapes.len(), 1);
        assert!(recording.is_empty());
        assert_ne!(held.tables(), CommandRecording::default().tables());
    }
}
