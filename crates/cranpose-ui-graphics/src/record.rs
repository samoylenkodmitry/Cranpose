use std::{cell::OnceCell, hash::Hasher, ops::Range};

use bytemuck::{Pod, Zeroable};

use crate::{
    ArcGeometry, BlendMode, Brush, Color, CornerRadii, DrawPrimitive, FxHasher, Point, Rect,
    RenderHash, Stroke, StrokeCap, StrokeJoin, TileMode, arc_band,
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
const NO_SEGMENT_KEY: u32 = u32::MAX;
const TWO_BITS: u32 = 0b11;
const BLEND_MASK: u32 = 0xff;

/// The brush kinds of [`BrushRecord::kind`].
pub const BRUSH_KIND_LINEAR: u32 = 1;
/// See [`BRUSH_KIND_LINEAR`].
pub const BRUSH_KIND_RADIAL: u32 = 2;
/// See [`BRUSH_KIND_LINEAR`].
pub const BRUSH_KIND_SWEEP: u32 = 3;

/// One recorded shape, in the draw command's local space, laid out as the
/// GPU reads it. Every value the app passed is kept verbatim, so the record
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
    /// bottom-left. Arcs: unused; the vertex stage derives the band's trig.
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
    /// Arcs: the normalised start angle and sweep.
    pub arc_normalized: [f32; 4],
}

impl ShapeRecord {
    pub fn kind(&self) -> u32 {
        (self.flags >> KIND_SHIFT) & TWO_BITS
    }

    pub fn is_stroked(&self) -> bool {
        self.flags & STROKED_BIT != 0
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
    /// [`CommandRecording::explicit_stops`], when it gave any; `explicit_len`
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
}

impl RecordSegment {
    pub fn range(&self) -> Range<usize> {
        self.start as usize..(self.start + self.count) as usize
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

/// Everything one draw command recorded, written once by the draw scope in
/// the form the renderer keeps: POD shape records with their brush and
/// stop tables, the primitives that are not shapes, the coalesced segments
/// a pass draws, the bounds and the content summary, each produced while
/// recording, and a fingerprint over all of it computed the first time a
/// cache asks.
#[derive(Clone, Debug)]
pub struct CommandRecording {
    shapes: Vec<ShapeRecord>,
    brushes: Vec<BrushRecord>,
    stops: Vec<GradientStopRecord>,
    explicit_stops: Vec<f32>,
    others: Vec<DrawPrimitive>,
    segments: Vec<RecordSegment>,
    last_segment_key: u32,
    min: [f32; 2],
    max: [f32; 2],
    summary: RecordingSummary,
    content_markers: u32,
    fingerprint: OnceCell<u64>,
}

impl Default for CommandRecording {
    fn default() -> Self {
        Self {
            shapes: Vec::new(),
            brushes: Vec::new(),
            stops: Vec::new(),
            explicit_stops: Vec::new(),
            others: Vec::new(),
            segments: Vec::new(),
            last_segment_key: NO_SEGMENT_KEY,
            min: [f32::INFINITY; 2],
            max: [f32::NEG_INFINITY; 2],
            summary: RecordingSummary::default(),
            content_markers: 0,
            fingerprint: OnceCell::new(),
        }
    }
}

impl PartialEq for CommandRecording {
    fn eq(&self, other: &Self) -> bool {
        self.shapes == other.shapes
            && self.brushes == other.brushes
            && self.stops == other.stops
            && self.explicit_stops == other.explicit_stops
            && self.others == other.others
            && self.segments == other.segments
            && self.content_markers == other.content_markers
    }
}

impl CommandRecording {
    pub fn from_primitives(primitives: impl IntoIterator<Item = DrawPrimitive>) -> Self {
        let mut recording = Self::default();
        for primitive in primitives {
            recording.push_primitive(primitive);
        }
        recording
    }

    /// Empties the recording, keeping every buffer's capacity for the next
    /// recording into it.
    pub fn clear(&mut self) {
        self.shapes.clear();
        self.brushes.clear();
        self.stops.clear();
        self.explicit_stops.clear();
        self.others.clear();
        self.segments.clear();
        self.last_segment_key = NO_SEGMENT_KEY;
        self.min = [f32::INFINITY; 2];
        self.max = [f32::NEG_INFINITY; 2];
        self.summary = RecordingSummary::default();
        self.content_markers = 0;
        self.fingerprint.take();
    }

    pub fn reserve_shapes(&mut self, additional: usize) {
        self.shapes.reserve(additional);
    }

    pub fn shape_capacity(&self) -> usize {
        self.shapes.capacity()
    }

    /// The heap the POD tables hold, capacity included.
    pub fn pod_heap_bytes(&self) -> usize {
        self.shapes.capacity() * std::mem::size_of::<ShapeRecord>()
            + self.brushes.capacity() * std::mem::size_of::<BrushRecord>()
            + self.stops.capacity() * std::mem::size_of::<GradientStopRecord>()
            + self.explicit_stops.capacity() * std::mem::size_of::<f32>()
            + self.segments.capacity() * std::mem::size_of::<RecordSegment>()
    }

    /// The entries inside `segments` that draw, content markers left out.
    pub fn len_in(&self, segments: &Range<u32>) -> usize {
        self.segments_in(segments)
            .filter(|segment| segment.lane != RecordLane::Content)
            .map(|segment| segment.count as usize)
            .sum()
    }

    pub fn shapes(&self) -> &[ShapeRecord] {
        &self.shapes
    }

    pub fn brushes(&self) -> &[BrushRecord] {
        &self.brushes
    }

    pub fn stops(&self) -> &[GradientStopRecord] {
        &self.stops
    }

    pub fn others(&self) -> &[DrawPrimitive] {
        &self.others
    }

    pub fn segments(&self) -> &[RecordSegment] {
        &self.segments
    }

    /// Every segment, the range a placement without a content split draws.
    pub fn all_segments(&self) -> Range<u32> {
        0..self.segments.len() as u32
    }

    /// A rect containing every entry's coverage rect: exact for rects and
    /// the other lanes, the disc around the band for a scope-recorded arc.
    pub fn bounds(&self) -> Option<Rect> {
        (self.min[0] <= self.max[0] && self.min[1] <= self.max[1]).then(|| Rect {
            x: self.min[0],
            y: self.min[1],
            width: self.max[0] - self.min[0],
            height: self.max[1] - self.min[1],
        })
    }

    pub fn summary(&self) -> RecordingSummary {
        self.summary
    }

    pub fn content_markers(&self) -> u32 {
        self.content_markers
    }

    /// Entries of every lane, content markers included.
    pub fn len(&self) -> usize {
        self.shapes.len() + self.others.len() + self.content_markers as usize
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
            hasher.write(bytemuck::cast_slice(&self.shapes));
            hasher.write(bytemuck::cast_slice(&self.brushes));
            hasher.write(bytemuck::cast_slice(&self.stops));
            hasher.write(bytemuck::cast_slice(&self.explicit_stops));
            for primitive in &self.others {
                hasher.write_u64(primitive.render_hash());
            }
            for segment in &self.segments {
                hasher.write_u8(segment.lane as u8);
                hasher.write_u32(segment.start);
                hasher.write_u32(segment.count);
            }
            hasher.write_u32(self.content_markers);
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
        self.segments[segments.start as usize..segments.end as usize].iter()
    }

    /// The summary of the entries inside `segments` only.
    pub fn summary_in(&self, segments: &Range<u32>) -> RecordingSummary {
        if *segments == self.all_segments() {
            return self.summary;
        }
        let mut summary = RecordingSummary::default();
        for segment in self.segments_in(segments) {
            match segment.lane {
                RecordLane::Shapes if segment.count > 0 => summary.has_non_shadow = true,
                RecordLane::Others => {
                    for primitive in &self.others[segment.range()] {
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
            .segments
            .iter()
            .rposition(|segment| segment.lane == RecordLane::Content);
        match (last_marker, behind) {
            (Some(index), true) => 0..index as u32,
            (Some(index), false) => index as u32 + 1..self.segments.len() as u32,
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
                        self.shapes[segment.range()]
                            .iter()
                            .map(ShapeRecord::coverage_rect),
                    ),
                    RecordLane::Others => Box::new(
                        self.others[segment.range()]
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
        self.segments
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
            RecordLane::Others => Box::new(self.others[segment.range()].iter().cloned()),
            RecordLane::Content if markers => Box::new(std::iter::repeat_n(
                DrawPrimitive::Content,
                segment.count as usize,
            )),
            RecordLane::Content => Box::new(std::iter::empty()),
        }
    }

    /// The exact [`DrawPrimitive`] the record was made from.
    pub fn materialize_shape(&self, index: usize) -> DrawPrimitive {
        let record = &self.shapes[index];
        let rect = record.rect_value();
        let brush = self.brush_of(record);
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
        let brush = &self.brushes[record.brush as usize - 1];
        let colors = self.stops
            [brush.stop_start as usize..(brush.stop_start + brush.stop_count) as usize]
            .iter()
            .map(|stop| Color(stop.color[0], stop.color[1], stop.color[2], stop.color[3]))
            .collect();
        let stops = (brush.explicit_len != NO_EXPLICIT_STOPS).then(|| {
            self.explicit_stops[brush.explicit_start as usize
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

    pub fn push_content(&mut self) {
        self.content_markers += 1;
        self.last_segment_key = NO_SEGMENT_KEY;
        self.segments.push(RecordSegment {
            lane: RecordLane::Content,
            start: 0,
            count: 1,
            blend: BlendMode::SrcOver,
            gradient: false,
        });
    }

    /// Records a primitive the way the draw scope would have: shapes and
    /// blended shapes become records, everything else joins the others lane.
    pub fn push_primitive(&mut self, primitive: DrawPrimitive) {
        match primitive {
            DrawPrimitive::Content => self.push_content(),
            DrawPrimitive::Blend {
                primitive,
                blend_mode,
            } => {
                if let Some(inner) = self.push_shape_primitive(*primitive, blend_mode) {
                    self.push_other(DrawPrimitive::Blend {
                        primitive: Box::new(inner),
                        blend_mode,
                    });
                }
            }
            other => {
                if let Some(other) = self.push_shape_primitive(other, BlendMode::SrcOver) {
                    self.push_other(other);
                }
            }
        }
    }

    /// Records a rect, rounded rect or arc under `blend_mode`; hands any
    /// other primitive back untouched.
    fn push_shape_primitive(
        &mut self,
        primitive: DrawPrimitive,
        blend_mode: BlendMode,
    ) -> Option<DrawPrimitive> {
        match primitive {
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
                ArcRecordArgs {
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
            other => return Some(other),
        }
        None
    }

    /// Records an image, text, shadow or nested blend.
    pub fn push_other(&mut self, primitive: DrawPrimitive) {
        self.summary.note(&primitive);
        if let Some(rect) = primitive_coverage_rect(&primitive) {
            self.include_bounds(rect);
        }
        let index = self.others.len() as u32;
        self.others.push(primitive);
        self.extend_segment(RecordLane::Others, index, BlendMode::SrcOver, false);
    }

    pub fn push_rect(
        &mut self,
        rect: Rect,
        brush: &Brush,
        stroke: Option<Stroke>,
        blend: BlendMode,
    ) {
        let (handle, color) = self.intern_brush(brush);
        self.push_shape(ShapeRecord {
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
        });
    }

    pub fn push_round_rect(
        &mut self,
        rect: Rect,
        brush: &Brush,
        radii: CornerRadii,
        stroke: Option<Stroke>,
        blend: BlendMode,
    ) {
        let (handle, color) = self.intern_brush(brush);
        self.push_shape(ShapeRecord {
            rect: rect_row(rect),
            radii: [
                radii.top_left,
                radii.top_right,
                radii.bottom_right,
                radii.bottom_left,
            ],
            color,
            stroke_width: stroke.map_or(0.0, |stroke| stroke.width),
            flags: pack_flags(RECORD_KIND_ROUND_RECT, stroke, blend, StrokeCap::Butt),
            brush: handle,
            reserved: 0,
            arc: [0.0; 4],
            arc_band: [0.0; 4],
            arc_normalized: [0.0; 4],
        });
    }

    /// Records an arc band or annular sector with the rect the primitive
    /// carries.
    pub fn push_arc(&mut self, rect: Rect, args: ArcRecordArgs<'_>) {
        let geometry = normalized_band(&args);
        self.push_arc_band(args, geometry, Some(rect));
    }

    /// Records an arc the draw scope drew, whose band the scope already
    /// normalised: the record keeps the disc around the band as its rect
    /// and derives the primitive's tight bounds only when asked.
    pub fn push_scope_arc(&mut self, args: ArcRecordArgs<'_>, geometry: ArcGeometry) {
        self.push_arc_band(args, geometry, None);
    }

    fn push_arc_band(
        &mut self,
        args: ArcRecordArgs<'_>,
        geometry: ArcGeometry,
        rect: Option<Rect>,
    ) {
        let (handle, color) = self.intern_brush(args.brush);
        let mut flags = pack_flags(RECORD_KIND_ARC, args.stroke, args.blend_mode, geometry.cap);
        if geometry.is_degenerate() {
            flags |= ARC_DEGENERATE_BIT;
        }
        let rect = rect.unwrap_or_else(|| {
            flags |= ARC_RECT_LOOSE_BIT;
            band_disc(&geometry)
        });
        self.push_shape(ShapeRecord {
            rect: rect_row(rect),
            radii: [0.0; 4],
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
            arc_normalized: [geometry.start_angle, geometry.sweep_angle, 0.0, 0.0],
        });
    }

    fn push_shape(&mut self, record: ShapeRecord) {
        self.summary.has_non_shadow = true;
        self.include_bounds(expand_rect(record.stored_rect(), record.half_stroke()));
        let index = self.shapes.len() as u32;
        let blend = record.blend_mode();
        let gradient = record.is_gradient();
        self.shapes.push(record);
        self.extend_segment(RecordLane::Shapes, index, blend, gradient);
    }

    fn extend_segment(&mut self, lane: RecordLane, index: u32, blend: BlendMode, gradient: bool) {
        let key = ((lane as u32) << 16) | ((blend as u32) << 1) | gradient as u32;
        if key == self.last_segment_key {
            let last = self.segments.last_mut().expect("a keyed segment exists");
            last.count += 1;
            return;
        }
        self.last_segment_key = key;
        self.segments.push(RecordSegment {
            lane,
            start: index,
            count: 1,
            blend,
            gradient,
        });
    }

    fn include_bounds(&mut self, rect: Rect) {
        self.min[0] = self.min[0].min(rect.x);
        self.min[1] = self.min[1].min(rect.y);
        self.max[0] = self.max[0].max(rect.x + rect.width);
        self.max[1] = self.max[1].max(rect.y + rect.height);
    }

    fn intern_brush(&mut self, brush: &Brush) -> (u32, [f32; 4]) {
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
        let stop_start = self.stops.len() as u32;
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
            self.stops.push(GradientStopRecord {
                color: [color.0, color.1, color.2, color.3],
                position: [position, 0.0, 0.0, 0.0],
            });
        }
        let (explicit_start, explicit_len) = match stops {
            Some(values) => {
                let start = self.explicit_stops.len() as u32;
                self.explicit_stops.extend_from_slice(values);
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
        self.brushes.push(record);
        let first = colors.first().copied().unwrap_or(Color(0.0, 0.0, 0.0, 0.0));
        (
            self.brushes.len() as u32,
            [first.0, first.1, first.2, first.3],
        )
    }

    /// Folds `other`'s summary into this recording's, for callers that
    /// combine recordings.
    pub fn merge_summary(&mut self, other: RecordingSummary) {
        self.summary.merge(other);
    }
}

/// The arc as the app drew it, the arguments of
/// [`CommandRecording::push_arc`].
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
    if geometry.sweep_angle >= crate::TAU && geometry.start_angle == 0.0 {
        return [0.0, -1.0, 0.0, -1.0];
    }
    let half_sweep = geometry.sweep_angle.clamp(0.0, crate::TAU) * 0.5;
    let (mid_sin, mid_cos) = (geometry.start_angle + half_sweep).sin_cos();
    let (half_sin, half_cos) = half_sweep.sin_cos();
    [mid_sin, mid_cos, half_sin.max(0.0), half_cos]
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
        let record = recording.shapes()[0];
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
            record.radii, [0.0; 4],
            "the trig row is the vertex stage's, not the recording's"
        );
        assert_eq!(
            arc_trig(&expected)[1],
            (expected.start_angle + expected.sweep_angle * 0.5).cos()
        );
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
        assert!(degenerate.shapes()[0].is_degenerate_arc());
    }

    #[test]
    fn segments_cut_on_blend_and_brush_class_and_lane_never_on_kind() {
        let mut recording = CommandRecording::default();
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
            ArcRecordArgs {
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
        let lanes: Vec<(RecordLane, u32, u32, BlendMode, bool)> = recording
            .segments()
            .iter()
            .map(|segment| {
                (
                    segment.lane,
                    segment.start,
                    segment.count,
                    segment.blend,
                    segment.gradient,
                )
            })
            .collect();
        assert_eq!(
            lanes,
            vec![
                (RecordLane::Shapes, 0, 3, BlendMode::SrcOver, false),
                (RecordLane::Shapes, 3, 1, BlendMode::SrcOver, true),
                (RecordLane::Shapes, 4, 1, BlendMode::Plus, false),
                (RecordLane::Others, 0, 2, BlendMode::SrcOver, false),
                (RecordLane::Content, 0, 1, BlendMode::SrcOver, false),
                (RecordLane::Shapes, 5, 1, BlendMode::SrcOver, false),
            ]
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
        let record = recording.shapes()[0];
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
        let mut recording = CommandRecording::from_primitives(every_primitive());
        let capacity = recording.shape_capacity();
        let fingerprint = recording.fingerprint();
        recording.clear();
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
    fn the_record_is_seven_rows() {
        assert_eq!(std::mem::size_of::<ShapeRecord>(), 112);
        assert_eq!(std::mem::size_of::<BrushRecord>(), 48);
        assert_eq!(std::mem::size_of::<GradientStopRecord>(), 32);
    }
}
