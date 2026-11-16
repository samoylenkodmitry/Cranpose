use std::{cell::OnceCell, hash::Hasher, ops::Range, sync::Arc};

use bytemuck::Zeroable;

use super::{
    ArcRecordArgs, BrushRecord, BrushShading, CAP_SHIFT, ContentMarkers, DrawGroups,
    GradientStopRecord, RECORD_KIND_LINE, RecordLane, RecordSegment, RecordTables, Recorded,
    RecordingSummary, ShapeRecord, ShapeRecorder, brush_of, line_record, materialize_shape,
    pack_flags, parts_primitives_with_markers, parts_segment_primitives, primitive_coverage_rect,
    union_rect,
};
use crate::{
    ArcGeometry, BlendMode, Brush, CornerRadii, DrawGroup, DrawPlacement, DrawPrimitive, FxHasher,
    GroupMarker, LineGeometry, Point, Rect, RenderHash, ShapeRecordBody, ShapeRecordCurve,
    ShapeRecords, Stroke, StrokeCap, Trapezoid,
};

/// A completed draw command with shared shape data, ordered segments and
/// non-shape primitives. Bounds and content metadata are recorded while drawing;
/// the complete fingerprint is computed when a cache first requests it.
/// [`Self::record`] records it anew in place, [`Self::edit`] records more after it.
#[derive(Debug)]
pub struct CommandRecording {
    shapes: Arc<ShapeRecorder>,
    // The shape columns held before `shapes`, while a reader may still hold
    // them: once it lets go, the next recording draws into them in place of
    // new ones.
    spare: Option<Arc<ShapeRecorder>>,
    content: RecordingContent,
    fingerprint: OnceCell<u64>,
}

// A copy shares the shape columns, not the spare: each copy keeps its own.
impl Clone for CommandRecording {
    fn clone(&self) -> Self {
        Self {
            shapes: Arc::clone(&self.shapes),
            spare: None,
            content: self.content.clone(),
            fingerprint: self.fingerprint.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct RecordingContent {
    pub(super) others: crate::rare_list::RareList<DrawPrimitive>,
    pub(super) groups: DrawGroups,
    pub(super) min: [f32; 2],
    pub(super) max: [f32; 2],
    pub(super) shape_count: u32,
    pub(super) segment_count: u32,
    pub(super) summary: RecordingSummary,
    pub(super) content_markers: u32,
}

impl Default for RecordingContent {
    fn default() -> Self {
        Self {
            others: crate::rare_list::RareList::default(),
            groups: DrawGroups::default(),
            min: [f32::INFINITY; 2],
            max: [f32::NEG_INFINITY; 2],
            shape_count: 0,
            segment_count: 0,
            summary: RecordingSummary::default(),
            content_markers: 0,
        }
    }
}

impl Default for CommandRecording {
    fn default() -> Self {
        Self {
            shapes: crate::thread_state::with(|state| {
                Arc::clone(state.no_shapes.get_or_init(Default::default))
            }),
            spare: None,
            content: RecordingContent::default(),
            fingerprint: OnceCell::new(),
        }
    }
}

impl PartialEq for CommandRecording {
    fn eq(&self, other: &Self) -> bool {
        self.shapes == other.shapes
            && self.content.others == other.content.others
            && self.content.groups.list() == other.content.groups.list()
            && self.content.content_markers == other.content.content_markers
    }
}

impl CommandRecording {
    /// Records into this recording anew, in the storage it holds: what it
    /// held is cleared and every buffer keeps its capacity. When a retained
    /// reader still shares the shape columns, the reader keeps them and the
    /// recording takes the columns it held before them, once their reader
    /// let them go, or else new ones with the same room.
    ///
    /// ```
    /// use cranpose_ui_graphics::{Brush, Color, CommandRecording, DrawPrimitive, Rect};
    ///
    /// let mut recording = CommandRecording::default();
    /// let mut recorder = recording.record();
    /// recorder.push_primitive(DrawPrimitive::Rect {
    ///     rect: Rect {
    ///         x: 0.0,
    ///         y: 0.0,
    ///         width: 1.0,
    ///         height: 1.0,
    ///     },
    ///     brush: Brush::Solid(Color::RED),
    ///     stroke: None,
    /// });
    /// recorder.finish();
    /// assert_eq!(recording.len(), 1);
    /// ```
    pub fn record(&mut self) -> CommandRecorder<'_> {
        let mut fresh_shapes = false;
        if Arc::get_mut(&mut self.shapes).is_none() {
            let unshared = self
                .spare
                .take()
                .and_then(|mut spare| Arc::get_mut(&mut spare).is_some().then_some(spare));
            let next = match unshared {
                Some(spare) => spare,
                None => {
                    fresh_shapes = true;
                    Arc::new(ShapeRecorder {
                        tables: self.shapes.tables.with_capacity_of(),
                        ..ShapeRecorder::default()
                    })
                }
            };
            self.spare = Some(std::mem::replace(&mut self.shapes, next));
        }
        let mut recorder = self.recorder(fresh_shapes);
        recorder.clear();
        recorder
    }

    /// Records more into this recording, after what it holds. Shape data is
    /// copied only when a retained reader still shares it.
    pub fn edit(&mut self) -> CommandRecorder<'_> {
        let fresh_shapes = Arc::get_mut(&mut self.shapes).is_none();
        self.recorder(fresh_shapes)
    }

    fn recorder(&mut self, fresh_shapes: bool) -> CommandRecorder<'_> {
        let Self {
            shapes,
            content,
            fingerprint,
            ..
        } = self;
        fingerprint.take();
        CommandRecorder {
            shapes: Arc::make_mut(shapes),
            content,
            fresh_shapes,
        }
    }

    /// Whether a reader other than this recording still holds its shape
    /// columns, so recording anew into it takes new ones.
    pub fn shares_shapes(&self) -> bool {
        Arc::strong_count(&self.shapes) > 1 || Arc::weak_count(&self.shapes) > 0
    }

    /// Creates a completed recording from `primitives`, in the given order.
    pub fn from_primitives(primitives: impl IntoIterator<Item = DrawPrimitive>) -> Self {
        let mut recording = Self::default();
        let mut recorder = recording.record();
        for primitive in primitives {
            recorder.push_primitive(primitive);
        }
        recorder.finish();
        recording
    }

    /// Number of shape records the recording has room for before it allocates.
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
        if *segments == self.all_segments() {
            return self.content.shape_count as usize + self.content.others.len();
        }
        self.segments_in(segments)
            .filter(|segment| segment.lane.draws())
            .map(|segment| segment.count as usize)
            .sum()
    }

    /// The shape records inside `segments`.
    pub fn shape_count_in(&self, segments: &Range<u32>) -> u32 {
        if *segments == self.all_segments() {
            return self.content.shape_count;
        }
        self.segments_in(segments)
            .filter(|segment| segment.lane == RecordLane::Shapes)
            .map(|segment| segment.count)
            .sum()
    }

    /// Where every recorded shape's pixels lie; `None` when none was recorded.
    pub fn shape_bounds(&self) -> Option<Rect> {
        self.shapes.bounds()
    }

    /// The shape columns, with complete records available on demand.
    pub fn shapes(&self) -> &ShapeRecords {
        &self.shapes.tables.shapes
    }

    /// The gradient brush table that shape records index into.
    pub fn brushes(&self) -> &[BrushRecord] {
        self.shapes.tables.brushes()
    }

    /// The gradient stop table that brush records index into.
    pub fn stops(&self) -> &[GradientStopRecord] {
        self.shapes.tables.stops()
    }

    /// The primitives that are not shapes, such as images, text and shadows, in recorded order.
    pub fn others(&self) -> &[DrawPrimitive] {
        &self.content.others
    }

    /// The groups that [`RecordLane::GroupStart`] segments open, in recorded order.
    pub fn groups(&self) -> &[DrawGroup] {
        self.content.groups.list()
    }

    /// The segments in recorded order.
    pub fn segments(&self) -> &[RecordSegment] {
        &self.shapes.tables.segments
    }

    /// Every segment, the range a placement without a content split draws.
    pub fn all_segments(&self) -> Range<u32> {
        0..self.content.segment_count
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

    /// What the recording contains, answered while it was recorded.
    pub fn summary(&self) -> RecordingSummary {
        self.content.summary
    }

    /// Number of `draw_content` markers recorded.
    pub fn content_markers(&self) -> u32 {
        self.content.content_markers
    }

    /// Entries of every lane, content markers included.
    pub fn len(&self) -> usize {
        self.content.shape_count as usize
            + self.content.others.len()
            + self.content.content_markers as usize
    }

    /// Whether the recording has no entries and no content markers.
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
            for primitive in self.content.others.iter() {
                hasher.write_u64(primitive.render_hash());
            }
            for group in self.content.groups.list() {
                crate::render_hash::hash_draw_group(group, &mut hasher);
            }
            hasher.write_u32(self.content.content_markers);
            hasher.finish()
        })
    }

    /// The segments of `segments` that draw anything.
    pub fn is_empty_in(&self, segments: &Range<u32>) -> bool {
        if *segments == self.all_segments() {
            return self.content.shape_count == 0 && self.content.others.is_empty();
        }
        !self
            .segments_in(segments)
            .any(|segment| segment.lane.draws() && segment.count > 0)
    }

    /// Iterates over the segments in the index range `segments`.
    pub fn segments_in(&self, segments: &Range<u32>) -> std::slice::Iter<'_, RecordSegment> {
        self.shapes.segments_in(segments)
    }

    /// The summary of the entries inside `segments` only.
    pub fn summary_in(&self, segments: &Range<u32>) -> RecordingSummary {
        if *segments == self.all_segments() {
            return self.content.summary;
        }
        let mut summary = RecordingSummary::default();
        for segment in self.segments_in(segments) {
            match segment.lane {
                RecordLane::Shapes if segment.count > 0 => {
                    summary.has_non_shadow = true;
                    summary.has_shapes = true;
                }
                RecordLane::Others => {
                    summary.has_others = true;
                    for primitive in &self.content.others[segment.range()] {
                        summary.note(primitive);
                    }
                }
                RecordLane::Content => summary.has_content_markers = true,
                RecordLane::GroupStart | RecordLane::GroupEnd => summary.has_groups = true,
                RecordLane::Shapes => {}
            }
        }
        summary
    }

    /// The segments before the last content marker (`Behind`) or after it
    /// (`Overlay`); with no marker, behind is empty and overlay is everything.
    pub fn content_split(&self, placement: DrawPlacement) -> Range<u32> {
        let last_marker = if self.content.content_markers == 0 {
            None
        } else {
            self.shapes
                .tables
                .segments
                .iter()
                .rposition(|segment| segment.lane == RecordLane::Content)
        };
        match (last_marker, placement) {
            (Some(index), DrawPlacement::Behind) => 0..index as u32,
            (Some(index), DrawPlacement::Overlay) => index as u32 + 1..self.content.segment_count,
            (None, DrawPlacement::Behind) => 0..0,
            (None, DrawPlacement::Overlay) => self.all_segments(),
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
                RecordLane::Content | RecordLane::GroupStart | RecordLane::GroupEnd => None,
            })
        })
    }

    /// The primitives inside `segments`, materialised in recorded order,
    /// content markers left out.
    pub fn primitives(&self, segments: Range<u32>) -> impl Iterator<Item = DrawPrimitive> + '_ {
        self.segments_in(&segments)
            .flat_map(|segment| self.segment_primitives(segment, ContentMarkers::Skipped))
    }

    /// Runs `visit` on each primitive [`Self::primitives`] yields for
    /// `segments`, in order. It reads a segment's lane once for all its
    /// entries, where the iterator matches it for each.
    pub fn for_each_primitive(&self, segments: Range<u32>, mut visit: impl FnMut(DrawPrimitive)) {
        for segment in self.segments_in(&segments) {
            let entries = segment.range();
            match segment.lane {
                RecordLane::Shapes => {
                    entries.for_each(|index| visit(self.materialize_shape(index)));
                }
                RecordLane::Others => {
                    entries.for_each(|index| visit(self.content.others[index].clone()));
                }
                RecordLane::Content => {}
                RecordLane::GroupStart => entries.for_each(|index| {
                    visit(DrawPrimitive::Group(GroupMarker::Start(
                        self.content.groups.list()[index],
                    )));
                }),
                RecordLane::GroupEnd => {
                    entries.for_each(|_| visit(DrawPrimitive::Group(GroupMarker::End)));
                }
            }
        }
    }

    /// Every primitive in recorded order, content markers included.
    pub fn primitives_with_markers(&self) -> impl Iterator<Item = DrawPrimitive> + '_ {
        parts_primitives_with_markers(&self.shapes, &self.content)
    }

    /// Consumes the recording and returns every primitive in recorded order, content markers
    /// included.
    pub fn into_primitives_with_markers(self) -> Vec<DrawPrimitive> {
        self.primitives_with_markers().collect()
    }

    fn segment_primitives<'a>(
        &'a self,
        segment: &RecordSegment,
        markers: ContentMarkers,
    ) -> impl Iterator<Item = DrawPrimitive> + use<'a> {
        parts_segment_primitives(&self.shapes, &self.content, segment, markers)
    }

    /// The exact [`DrawPrimitive`] the record was made from.
    pub fn materialize_shape(&self, index: usize) -> DrawPrimitive {
        materialize_shape(&self.shapes, index)
    }

    /// The brush of a record: its solid colour, or the gradient rebuilt
    /// from the tables exactly as the app gave it.
    pub fn brush_of(&self, record: &ShapeRecord) -> Brush {
        brush_of(&self.shapes, record)
    }
}

/// Records draw commands into a [`CommandRecording`] it borrows, in place:
/// [`CommandRecording::record`] lends the recording's shape columns and
/// content, and [`Self::finish`] completes them there, so a recording kept
/// from an earlier frame is drawn again in the storage it already has.
#[derive(Debug)]
pub struct CommandRecorder<'a> {
    pub(super) shapes: &'a mut ShapeRecorder,
    content: &'a mut RecordingContent,
    /// Whether the shape columns are new rather than an earlier
    /// recording's, whose room [`Self::finish`] then trims.
    fresh_shapes: bool,
}

impl<'a> CommandRecorder<'a> {
    /// Completes the recording: closes the groups still open and notes its
    /// bounds and counts. A recording into new shape columns gives back the
    /// room its growth left: drawing it again then refills that allocation.
    pub fn finish(self) {
        self.finish_parts();
    }

    /// [`Self::finish`], then every primitive in recorded order, content
    /// markers included.
    pub fn into_primitives(self) -> Vec<DrawPrimitive> {
        let (shapes, content) = self.finish_parts();
        parts_primitives_with_markers(shapes, content).collect()
    }

    fn finish_parts(mut self) -> (&'a ShapeRecorder, &'a RecordingContent) {
        while !self.content.groups.open().is_empty() {
            self.push_group_end();
        }
        if self.fresh_shapes {
            self.shapes.tables.trim_growth();
        }
        self.content.shape_count = self.shapes.tables.shapes.len() as u32;
        self.content.segment_count = self.shapes.tables.segments.len() as u32;
        (self.shapes, self.content)
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
        self.content.groups.clear();
        self.content.min = [f32::INFINITY; 2];
        self.content.max = [f32::NEG_INFINITY; 2];
        self.content.shape_count = 0;
        self.content.segment_count = 0;
        self.content.summary = RecordingSummary::default();
        self.content.content_markers = 0;
    }

    /// Reserves capacity for additional shape records.
    pub fn reserve_shapes(&mut self, additional: usize) {
        self.shapes.tables_mut().shapes.reserve(additional);
    }

    /// Records a content marker at the current command position. The
    /// node's content draws outside the groups open here: they end before
    /// the marker and start again after it.
    pub fn push_content(&mut self) {
        self.content.content_markers += 1;
        self.content.summary.has_content_markers = true;
        for _ in self.content.groups.open() {
            self.shapes.push_marker_segment(RecordLane::GroupEnd, 0);
        }
        self.shapes.push_marker_segment(RecordLane::Content, 0);
        for &group in self.content.groups.open() {
            self.shapes
                .push_marker_segment(RecordLane::GroupStart, group);
        }
    }

    /// Starts a group of draws that `group` places: what is recorded until
    /// the matching [`Self::push_group_end`] is in the group's coordinates.
    pub fn push_group_start(&mut self, group: DrawGroup) {
        let index = self.content.groups.start(group);
        self.content.summary.has_groups = true;
        self.shapes
            .push_marker_segment(RecordLane::GroupStart, index);
    }

    /// Ends the group started last; nothing when no group is open.
    pub fn push_group_end(&mut self) {
        if self.content.groups.end() {
            self.shapes.push_marker_segment(RecordLane::GroupEnd, 0);
        }
    }

    /// Records a primitive the way the draw scope would have: shapes and
    /// blended shapes become records, group markers open and end groups,
    /// everything else joins the others lane.
    pub fn push_primitive(&mut self, primitive: DrawPrimitive) {
        match primitive {
            DrawPrimitive::Content => {
                self.push_content();
                return;
            }
            DrawPrimitive::Group(GroupMarker::Start(group)) => {
                self.push_group_start(group);
                return;
            }
            DrawPrimitive::Group(GroupMarker::End) => {
                self.push_group_end();
                return;
            }
            _ => {}
        }
        match self.shapes.push_primitive(primitive) {
            Recorded::Shape(_) => self.note_shape(),
            Recorded::Other(other) => self.push_other(other),
        }
    }

    #[inline]
    fn note_shape(&mut self) {
        self.content.summary.has_non_shadow = true;
        self.content.summary.has_shapes = true;
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

    /// Starts the segments of a stroke with `brush`, `stroke`'s width and
    /// join, and `blend`; see [`LineRun`].
    pub fn line_run<'r>(
        &'r mut self,
        brush: &'r Brush,
        stroke: Stroke,
        blend: BlendMode,
    ) -> LineRun<'r, 'a> {
        LineRun {
            flags: pack_flags(
                RECORD_KIND_LINE,
                Some(Stroke {
                    cap: StrokeCap::Butt,
                    ..stroke
                }),
                blend,
                StrokeCap::Butt,
            ),
            recorder: self,
            brush,
            stroke,
            drawn: LineGeometry::width_draws(stroke.half_width()),
            blend,
            interned: None,
        }
    }

    /// Records the slices of a path fill a draw scope cut; see
    /// [`ShapeRecorder::push_trapezoids`].
    pub fn push_trapezoids(
        &mut self,
        brush_rect: Rect,
        trapezoids: &[Trapezoid],
        brush: &Brush,
        blend: BlendMode,
    ) {
        if trapezoids.is_empty() {
            return;
        }
        self.shapes
            .push_trapezoids(brush_rect, trapezoids, brush, blend);
        self.note_shape();
    }

    /// Records a primitive in the non-shape lane and updates its metadata.
    pub fn push_other(&mut self, primitive: DrawPrimitive) {
        self.content.summary.has_others = true;
        self.content.summary.note(&primitive);
        if let Some(rect) = primitive_coverage_rect(&primitive) {
            self.include_bounds(rect);
        }
        let index = self.content.others.len() as u32;
        self.content.others.push(primitive);
        self.shapes
            .extend_segment(RecordLane::Others, index, BlendMode::SrcOver, 0);
    }

    /// Folds `other`'s summary into this recording's, for callers that
    /// combine recordings.
    pub fn merge_summary(&mut self, other: RecordingSummary) {
        self.content.summary.merge(other);
    }
}

/// The segments of one stroke a draw scope records in turn, each with its
/// own cap. The brush is interned and its shading found at the first
/// segment, and each later one extends the segment of records the first
/// opened or joined. [`CommandRecorder::line_run`] starts one.
pub struct LineRun<'r, 'a> {
    recorder: &'r mut CommandRecorder<'a>,
    brush: &'r Brush,
    flags: u32,
    stroke: Stroke,
    /// Whether the stroke's width draws anything.
    drawn: bool,
    blend: BlendMode,
    /// The interned brush and its shading, from the first segment on.
    interned: Option<((u32, [f32; 4]), BrushShading)>,
}

impl LineRun<'_, '_> {
    /// Records the segment from `start` to `end` with `cap` at both ends.
    /// A degenerate segment ([`LineGeometry::is_degenerate`]) draws
    /// nothing and is left out, as Compose's `drawLine` draws nothing there.
    pub fn push(&mut self, start: Point, end: Point, cap: StrokeCap) {
        if !self.drawn || !LineGeometry::ends_draw(start, end, cap) {
            return;
        }
        let line = LineGeometry::new(start, end, Stroke { cap, ..self.stroke });
        let started = self.interned.is_some();
        let (brush, shading) = match self.interned {
            Some(interned) => interned,
            None => {
                let shapes = &mut *self.recorder.shapes;
                let brush = shapes.intern_brush(self.brush);
                let shading = shapes.brush_shading(
                    &ShapeRecordBody {
                        brush: brush.0,
                        flags: self.flags,
                        ..ShapeRecordBody::zeroed()
                    },
                    &ShapeRecordCurve::zeroed(),
                );
                self.recorder.note_shape();
                self.interned = Some((brush, shading));
                (brush, shading)
            }
        };
        let frame = line.frame();
        let flags = self.flags | ((cap as u32) << CAP_SHIFT);
        let (body, curve) = line_record(&line, &frame, brush, self.stroke.width, flags);
        let shapes = &mut *self.recorder.shapes;
        shapes.include_bounds(line.bounds_in(&frame));
        shapes.push_in_run(started, body, curve, [0.0; 4], (self.blend, shading));
    }
}
