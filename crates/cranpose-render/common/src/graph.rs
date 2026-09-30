use std::{mem::size_of, ops::Range, rc::Rc};

use cranpose_core::{NodeId, collections::map::HashSet};
use cranpose_ui::{
    GraphicsLayer, ModifierNodeSlices, Point, Rect, RenderEffect, RoundedCornerShape,
    TextLayoutOptions, TextOverflow, TextStyle,
    text::{AnnotatedString, RenderString},
};
use cranpose_ui_graphics::{
    BlendMode, ColorFilter, CommandRecording, DrawPrimitive, RecordingSummary, ShadowPrimitive,
};

use crate::{raster_cache::LayerRasterCacheHashes, style_shared::DrawPlacement};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectiveTransform {
    matrix: [[f32; 3]; 3],
}

impl ProjectiveTransform {
    pub const fn identity() -> Self {
        Self {
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    pub fn translation(tx: f32, ty: f32) -> Self {
        Self {
            matrix: [[1.0, 0.0, tx], [0.0, 1.0, ty], [0.0, 0.0, 1.0]],
        }
    }

    /// Uniform scale about the origin (device-scale root transform: render
    /// graphs stay in logical dp; density applies at execution).
    pub fn uniform_scale(scale: f32) -> Self {
        Self {
            matrix: [[scale, 0.0, 0.0], [0.0, scale, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    pub fn from_rect_to_quad(rect: Rect, quad: [[f32; 2]; 4]) -> Self {
        if rect.width.abs() <= f32::EPSILON || rect.height.abs() <= f32::EPSILON {
            return Self::translation(quad[0][0], quad[0][1]);
        }

        if let Some(axis_aligned) = axis_aligned_rect_from_quad(quad) {
            let scale_x = axis_aligned.width / rect.width;
            let scale_y = axis_aligned.height / rect.height;
            return Self {
                matrix: [
                    [scale_x, 0.0, axis_aligned.x - rect.x * scale_x],
                    [0.0, scale_y, axis_aligned.y - rect.y * scale_y],
                    [0.0, 0.0, 1.0],
                ],
            };
        }

        let source = [
            [rect.x, rect.y],
            [rect.x + rect.width, rect.y],
            [rect.x, rect.y + rect.height],
            [rect.x + rect.width, rect.y + rect.height],
        ];
        let Some(coefficients) = solve_homography(source, quad) else {
            return Self::identity();
        };

        Self {
            matrix: [
                [coefficients[0], coefficients[1], coefficients[2]],
                [coefficients[3], coefficients[4], coefficients[5]],
                [coefficients[6], coefficients[7], 1.0],
            ],
        }
    }

    /// The transform a homogeneous matrix describes, scaled so its last entry
    /// is one wherever that entry is not zero: its other entries then read as
    /// the scale, turn, translation and perspective they are.
    pub(crate) fn from_homogeneous(matrix: [[f32; 3]; 3]) -> Self {
        let w = matrix[2][2];
        if w == 1.0 || w.abs() <= f32::EPSILON {
            return Self { matrix };
        }
        Self {
            matrix: matrix.map(|row| row.map(|value| value / w)),
        }
    }

    /// Returns the composed transform that applies `self` first and `next` second.
    pub fn then(self, next: Self) -> Self {
        Self {
            matrix: multiply_matrices(next.matrix, self.matrix),
        }
    }

    pub fn inverse(self) -> Option<Self> {
        let m = self.matrix;
        let a = m[0][0];
        let b = m[0][1];
        let c = m[0][2];
        let d = m[1][0];
        let e = m[1][1];
        let f = m[1][2];
        let g = m[2][0];
        let h = m[2][1];
        let i = m[2][2];

        let cofactor00 = e * i - f * h;
        let cofactor01 = -(d * i - f * g);
        let cofactor02 = d * h - e * g;
        let cofactor10 = -(b * i - c * h);
        let cofactor11 = a * i - c * g;
        let cofactor12 = -(a * h - b * g);
        let cofactor20 = b * f - c * e;
        let cofactor21 = -(a * f - c * d);
        let cofactor22 = a * e - b * d;

        let determinant = a * cofactor00 + b * cofactor01 + c * cofactor02;
        if determinant.abs() <= f32::EPSILON {
            return None;
        }
        let inverse_determinant = 1.0 / determinant;

        Some(Self {
            matrix: [
                [
                    cofactor00 * inverse_determinant,
                    cofactor10 * inverse_determinant,
                    cofactor20 * inverse_determinant,
                ],
                [
                    cofactor01 * inverse_determinant,
                    cofactor11 * inverse_determinant,
                    cofactor21 * inverse_determinant,
                ],
                [
                    cofactor02 * inverse_determinant,
                    cofactor12 * inverse_determinant,
                    cofactor22 * inverse_determinant,
                ],
            ],
        })
    }

    pub fn matrix(self) -> [[f32; 3]; 3] {
        self.matrix
    }

    pub fn map_point(self, point: Point) -> Point {
        let x = point.x;
        let y = point.y;
        let w = self.matrix[2][0] * x + self.matrix[2][1] * y + self.matrix[2][2];
        let safe_w = if w.abs() <= f32::EPSILON { 1.0 } else { w };

        Point {
            x: (self.matrix[0][0] * x + self.matrix[0][1] * y + self.matrix[0][2]) / safe_w,
            y: (self.matrix[1][0] * x + self.matrix[1][1] * y + self.matrix[1][2]) / safe_w,
        }
    }

    pub fn map_rect(self, rect: Rect) -> [[f32; 2]; 4] {
        [
            self.map_point(Point {
                x: rect.x,
                y: rect.y,
            }),
            self.map_point(Point {
                x: rect.x + rect.width,
                y: rect.y,
            }),
            self.map_point(Point {
                x: rect.x,
                y: rect.y + rect.height,
            }),
            self.map_point(Point {
                x: rect.x + rect.width,
                y: rect.y + rect.height,
            }),
        ]
        .map(|point| [point.x, point.y])
    }

    pub fn bounds_for_rect(self, rect: Rect) -> Rect {
        quad_bounds(self.map_rect(rect))
    }
}

fn axis_aligned_rect_from_quad(quad: [[f32; 2]; 4]) -> Option<Rect> {
    let top_left = quad[0];
    let top_right = quad[1];
    let bottom_left = quad[2];
    let bottom_right = quad[3];
    let x_epsilon = 1e-4;
    let y_epsilon = 1e-4;

    if (top_left[1] - top_right[1]).abs() > y_epsilon
        || (bottom_left[1] - bottom_right[1]).abs() > y_epsilon
        || (top_left[0] - bottom_left[0]).abs() > x_epsilon
        || (top_right[0] - bottom_right[0]).abs() > x_epsilon
    {
        return None;
    }

    Some(Rect {
        x: top_left[0],
        y: top_left[1],
        width: top_right[0] - top_left[0],
        height: bottom_left[1] - top_left[1],
    })
}

impl Default for ProjectiveTransform {
    fn default() -> Self {
        Self::identity()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IsolationReasons {
    pub explicit_offscreen: bool,
    pub shape_clip: bool,
    pub effect: bool,
    pub backdrop: bool,
    pub group_opacity: bool,
    pub blend_mode: bool,
}

impl IsolationReasons {
    pub fn has_any(self) -> bool {
        self.explicit_offscreen
            || self.shape_clip
            || self.effect
            || self.backdrop
            || self.group_opacity
            || self.blend_mode
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CachePolicy {
    #[default]
    None,
    Auto,
}

#[derive(Clone)]
pub struct HitTestNode {
    pub shape: Option<RoundedCornerShape>,
    /// The node's modifier slices, shared rather than copied: they hold its
    /// pointer inputs and the pointer icon it asks for while hovered. A node
    /// that only names an icon is still a hit target, which is how a
    /// decorative panel carries a cursor without handling clicks.
    pub handlers: Rc<ModifierNodeSlices>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawPrimitiveNode {
    pub primitive: DrawPrimitive,
    pub clip: Option<Rect>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextPrimitiveNode {
    pub node_id: NodeId,
    pub rect: Rect,
    /// Shared so the renderer can hand the same allocation to every draw it
    /// emits for this node instead of deep-copying the string once per emit.
    pub text: Rc<AnnotatedString>,
    /// `text` as the renderer's draws carry it across threads, shared with
    /// the prepared layout it came from so no frame converts it again.
    pub render_text: std::sync::Arc<RenderString>,
    /// Shared so every frame's draws of this text hand over the same style
    /// instead of copying it.
    pub text_style: std::sync::Arc<TextStyle>,
    pub font_size: f32,
    pub layout_options: TextLayoutOptions,
    pub clip: Option<Rect>,
}

impl TextPrimitiveNode {
    /// Whether the text's glyphs stay inside `bounds`: nothing casts a shadow
    /// past them and the layout does not let the text overflow its rect.
    fn draws_within(&self, bounds: Rect) -> bool {
        self.text_style.span_style.shadow.is_none()
            && self
                .text
                .span_styles
                .iter()
                .all(|span| span.item.shadow.is_none())
            && !matches!(self.layout_options.overflow, TextOverflow::Visible)
            && rect_within(self.rect, bounds)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitivePhase {
    BeforeChildren,
    AfterChildren,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PrimitiveNode {
    Draw(Box<DrawPrimitiveNode>),
    Text(Box<TextPrimitiveNode>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrimitiveEntry {
    pub phase: PrimitivePhase,
    pub node: PrimitiveNode,
}

#[derive(Clone)]
pub struct LayerNode {
    pub node_id: Option<NodeId>,
    /// Set on the synthetic layer that holds a node's outer draws, those chained
    /// before its graphics layer, around that node's own layer. Scene updates
    /// address the node through this id because the node's layer has none of the
    /// outer draws and the wrapper has no node id of its own.
    pub wraps: Option<NodeId>,
    /// The bounds of the node's layer, where its clip cuts, its transforms
    /// pivot and an offscreen pass draws it.
    pub local_bounds: Rect,
    /// The node's own rect, `(0, 0, size)`, where its pointer handlers are,
    /// when it differs from `local_bounds`: a clip or graphics layer that
    /// wraps a coordinator a later padding or offset moves bounds the layer
    /// there instead, as in Compose. `None` when the two agree.
    pub node_bounds: Option<Rect>,
    pub transform_to_parent: ProjectiveTransform,
    pub content_offset: Point,
    pub motion_context_animated: bool,
    pub translated_content_context: bool,
    pub translated_content_offset: Point,
    /// Where layout placed this layer within its parent's content, before the
    /// parent's content offset. Scene updates add these up from the root, with
    /// each layer's content offset and graphics-layer translation, to find the
    /// window origin of a subtree they rebuild.
    pub origin_in_parent: Point,
    pub graphics_layer: GraphicsLayer,
    pub clip_to_bounds: bool,
    pub shadow_clip: Option<Rect>,
    pub hit_test: Option<HitTestNode>,
    pub has_hit_targets: bool,
    /// Whether this subtree publishes live window origins (a text field's
    /// popup anchor, a scroll container's viewport rect). Those sinks are
    /// written during a full lowering, so the scroll fast path may translate
    /// a retained subtree in place only when this is false.
    pub has_origin_sinks: bool,
    /// Whether everything this layer and its subtree draw lies within its
    /// `local_bounds`, give or take [`CONTAINED_DRAW_SLACK`], so a renderer
    /// may skip the whole subtree where those bounds are clipped away. Scene
    /// building keeps it current; `false` promises nothing.
    pub draws_within_bounds: bool,
    pub isolation: IsolationReasons,
    pub cache_policy: CachePolicy,
    pub cache_hashes: LayerRasterCacheHashes,
    pub cache_hashes_valid: bool,
    pub children: Vec<RenderNode>,
}

impl Default for LayerNode {
    fn default() -> Self {
        Self {
            node_id: None,
            wraps: None,
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            node_bounds: None,
            transform_to_parent: ProjectiveTransform::identity(),
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            translated_content_offset: Point::default(),
            origin_in_parent: Point::default(),
            graphics_layer: GraphicsLayer::default(),
            clip_to_bounds: false,
            shadow_clip: None,
            hit_test: None,
            has_hit_targets: false,
            has_origin_sinks: false,
            draws_within_bounds: false,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            cache_hashes_valid: false,
            children: Vec::new(),
        }
    }
}

/// How far past its bounds a layer that draws within them may still put
/// pixels: glyph and edge antialiasing.
pub const CONTAINED_DRAW_SLACK: f32 = 1.0;

impl LayerNode {
    /// Whether this layer's content, as [`LayerNode::draws_within_bounds`]
    /// describes it, stays within its bounds: it clips to them, or its draws,
    /// its texts and its children placed where they are all fit inside.
    pub fn content_draws_within_bounds(&self) -> bool {
        if self.visual_clip_rect().is_some() {
            return true;
        }
        let bounds = inflate_rect(self.local_bounds, CONTAINED_DRAW_SLACK);
        self.children.iter().all(|child| match child {
            RenderNode::DrawRun(run) => run
                .recording
                .bounds()
                .is_none_or(|drawn| rect_within(drawn, bounds)),
            RenderNode::Primitive(entry) => match &entry.node {
                PrimitiveNode::Text(text) => text.draws_within(bounds),
                PrimitiveNode::Draw(_) => false,
            },
            RenderNode::Layer(layer) => layer.draws_within_parent(bounds),
        })
    }

    pub(crate) fn refresh_child_facts(&mut self) {
        self.has_hit_targets = self.hit_test.is_some()
            || self.children.iter().any(|child| match child {
                RenderNode::Layer(child_layer) => child_layer.has_hit_targets,
                RenderNode::Primitive(_) | RenderNode::DrawRun(_) => false,
            });
        self.draws_within_bounds = self.content_draws_within_bounds();
    }

    /// Whether this layer, drawn as a child, puts nothing outside `bounds`:
    /// it draws within its own bounds, casts no shadow, applies no effect,
    /// and its bounds placed in its parent lie inside.
    fn draws_within_parent(&self, bounds: Rect) -> bool {
        self.draws_within_bounds
            && self.graphics_layer.shadow_elevation <= 0.0
            && self.effect().is_none()
            && self.backdrop().is_none()
            && rect_within(
                quad_bounds(self.transform_to_parent.map_rect(self.local_bounds)),
                bounds,
            )
    }

    /// The node's own rect, where its pointer handlers are.
    pub fn node_rect(&self) -> Rect {
        self.node_bounds.unwrap_or(self.local_bounds)
    }

    pub fn clip_rect(&self) -> Option<Rect> {
        (self.clip_to_bounds || self.graphics_layer.clip).then_some(self.local_bounds)
    }

    /// Where the layer's drawing is cut: its clip or, when it composites
    /// through an offscreen buffer for its alpha or by request, its bounds,
    /// as Compose's layer-sized buffer cuts it. Hit testing takes only
    /// [`Self::clip_rect`]: alpha hides nothing from a finger.
    pub fn visual_clip_rect(&self) -> Option<Rect> {
        self.clip_rect().or_else(|| {
            (self.isolation.group_opacity || self.isolation.explicit_offscreen)
                .then_some(self.local_bounds)
        })
    }

    pub fn effect(&self) -> Option<&RenderEffect> {
        self.graphics_layer.render_effect.as_ref()
    }

    pub fn backdrop(&self) -> Option<&RenderEffect> {
        self.graphics_layer.backdrop_effect.as_ref()
    }

    pub fn opacity(&self) -> f32 {
        self.graphics_layer.alpha
    }

    pub fn blend_mode(&self) -> BlendMode {
        self.graphics_layer.blend_mode
    }

    pub fn color_filter(&self) -> Option<ColorFilter> {
        self.graphics_layer.color_filter
    }

    pub fn target_content_hash(&self) -> u64 {
        if self.cache_hashes_valid {
            self.cache_hashes.target_content
        } else {
            crate::graph_hash::layer_raster_cache_hashes(self).target_content
        }
    }

    pub fn motion_source_content_hash(&self) -> u64 {
        crate::graph_hash::layer_motion_source_content_hash(self)
    }

    pub fn effect_hash(&self) -> u64 {
        if self.cache_hashes_valid {
            self.cache_hashes.effect
        } else {
            crate::graph_hash::layer_raster_cache_hashes(self).effect
        }
    }

    pub fn recompute_raster_cache_hashes(&mut self) {
        crate::graph_hash::recompute_layer_raster_cache_hashes(self);
    }
}

#[derive(Clone)]
pub enum RenderNode {
    Primitive(PrimitiveEntry),
    /// A whole draw command's primitives as one node. A heavy canvas records
    /// thousands of primitives per frame; wrapping each in its own
    /// [`RenderNode`] made the graph rebuild move every one of them twice and
    /// free seventeen thousand nodes per frame on a stress scene. The run
    /// keeps the recorded vector intact — semantically it is exactly that
    /// many consecutive `Primitive` draw entries with no per-primitive clip.
    DrawRun(DrawRunNode),
    Layer(Box<LayerNode>),
}

/// Stable identity of the draw command a run was recorded from: the layout
/// node owning the command, the command's index in that node's command list,
/// and which placement pass produced this run (a `WithContent` command emits
/// one run per placement, so the pair alone is not unique). Rendering does
/// not read it yet; it is the key under which retained recording state lives
/// as retention moves up to the draw-command recorder, and it must survive
/// recording, graph construction, normalized-scene creation, and renderer
/// cache lookup unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DrawCommandId {
    pub node_id: NodeId,
    pub command_index: u32,
    pub placement: DrawPlacement,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawRunNode {
    pub phase: PrimitivePhase,
    /// Which draw command recorded these primitives. `None` only for runs
    /// with no per-command provenance (hand-built tests).
    pub command: Option<DrawCommandId>,
    /// Shared, not owned: the recording registry keyed by [`DrawCommandId`]
    /// keeps a handle to the same recording, so its buffers survive this
    /// node being dropped on the next rebuild and the command re-records
    /// into them. Nothing mutates a recording after construction, which is
    /// what makes sharing sound.
    pub recording: Rc<CommandRecording>,
    /// The segments of the recording this run draws: one placement's part
    /// of a with-content command, or the whole recording.
    pub segments: Range<u32>,
    /// Content facts consumers keep asking per frame, answered while
    /// recording and never by rescanning.
    pub summary: DrawRunSummary,
}

/// What a run contains, answered while recording.
pub type DrawRunSummary = RecordingSummary;

impl DrawRunNode {
    pub fn new(phase: PrimitivePhase, primitives: Vec<DrawPrimitive>) -> Self {
        Self::for_command(phase, None, primitives)
    }

    pub fn for_command(
        phase: PrimitivePhase,
        command: Option<DrawCommandId>,
        primitives: Vec<DrawPrimitive>,
    ) -> Self {
        let recording = CommandRecording::from_primitives(primitives);
        let segments = recording.all_segments();
        Self::for_command_shared(phase, command, Rc::new(recording), segments)
    }

    pub fn for_command_shared(
        phase: PrimitivePhase,
        command: Option<DrawCommandId>,
        recording: Rc<CommandRecording>,
        segments: Range<u32>,
    ) -> Self {
        let summary = recording.summary_in(&segments);
        Self {
            phase,
            command,
            recording,
            segments,
            summary,
        }
    }

    /// The run's primitives, materialised from the recording in order.
    pub fn primitives(&self) -> impl Iterator<Item = DrawPrimitive> + '_ {
        self.recording.primitives(self.segments.clone())
    }

    /// The rect every entry of the run can reach.
    pub fn coverage_rects(&self) -> impl Iterator<Item = Rect> + '_ {
        self.recording.coverage_rects(self.segments.clone())
    }

    pub fn len(&self) -> usize {
        self.recording.len_in(&self.segments)
    }

    pub fn is_empty(&self) -> bool {
        self.recording.is_empty_in(&self.segments)
    }
}

#[derive(Clone)]
pub struct RenderGraph {
    pub root: LayerNode,
}

impl RenderGraph {
    pub fn new(mut root: LayerNode) -> Self {
        root.recompute_raster_cache_hashes();
        Self { root }
    }

    pub fn node_count(&self) -> usize {
        fn count_layer(layer: &LayerNode) -> usize {
            1 + layer
                .children
                .iter()
                .map(|child| match child {
                    RenderNode::Primitive(_) => 1,
                    RenderNode::DrawRun(run) => run.len(),
                    RenderNode::Layer(child_layer) => count_layer(child_layer),
                })
                .sum::<usize>()
        }

        count_layer(&self.root)
    }

    pub fn heap_bytes(&self) -> usize {
        layer_heap_bytes(&self.root)
    }

    /// Replaces the set with the graph's visual observation owners, retaining its capacity.
    pub fn collect_retained_visual_observation_nodes(&self, nodes: &mut HashSet<NodeId>) {
        fn collect(layer: &LayerNode, nodes: &mut HashSet<NodeId>) {
            if let Some(node_id) = layer.node_id {
                nodes.insert(node_id);
            }
            for child in &layer.children {
                match child {
                    RenderNode::DrawRun(run) => {
                        if let Some(command) = run.command {
                            nodes.insert(command.node_id);
                        }
                    }
                    RenderNode::Layer(child) => collect(child, nodes),
                    RenderNode::Primitive(_) => {}
                }
            }
        }

        nodes.clear();
        collect(&self.root, nodes);
    }
}

fn layer_heap_bytes(layer: &LayerNode) -> usize {
    size_of::<RenderNode>() * layer.children.capacity()
        + layer
            .children
            .iter()
            .map(render_node_heap_bytes)
            .sum::<usize>()
}

fn render_node_heap_bytes(node: &RenderNode) -> usize {
    match node {
        RenderNode::Primitive(entry) => primitive_entry_heap_bytes(entry),
        RenderNode::DrawRun(run) => {
            run.recording.pod_heap_bytes()
                + std::mem::size_of_val(run.recording.others())
                + run
                    .recording
                    .others()
                    .iter()
                    .map(draw_primitive_heap_bytes)
                    .sum::<usize>()
        }
        RenderNode::Layer(layer) => size_of::<LayerNode>() + layer_heap_bytes(layer),
    }
}

fn primitive_entry_heap_bytes(entry: &PrimitiveEntry) -> usize {
    match &entry.node {
        PrimitiveNode::Draw(draw) => {
            size_of::<DrawPrimitiveNode>() + draw_primitive_heap_bytes(&draw.primitive)
        }
        PrimitiveNode::Text(text) => {
            size_of::<TextPrimitiveNode>() + annotated_string_heap_bytes(&text.text)
        }
    }
}

fn draw_primitive_heap_bytes(primitive: &DrawPrimitive) -> usize {
    match primitive {
        DrawPrimitive::Content
        | DrawPrimitive::Rect { .. }
        | DrawPrimitive::RoundRect { .. }
        | DrawPrimitive::Arc { .. }
        | DrawPrimitive::Line { .. } => 0,
        DrawPrimitive::Blend { primitive, .. } => {
            size_of::<DrawPrimitive>() + draw_primitive_heap_bytes(primitive)
        }
        DrawPrimitive::Image { .. } => 0,
        DrawPrimitive::Text(text) => {
            size_of::<cranpose_ui_graphics::TextPrimitive>()
                + text.text.len()
                + text
                    .style
                    .font_family
                    .as_ref()
                    .map_or(0, std::string::String::capacity)
        }
        DrawPrimitive::Shadow(shadow) => shadow_primitive_heap_bytes(shadow),
    }
}

fn shadow_primitive_heap_bytes(shadow: &ShadowPrimitive) -> usize {
    match shadow {
        ShadowPrimitive::Drop { shape, .. } => {
            size_of::<DrawPrimitive>() + draw_primitive_heap_bytes(shape)
        }
        ShadowPrimitive::Inner { fill, cutout, .. } => {
            size_of::<DrawPrimitive>() * 2
                + draw_primitive_heap_bytes(fill)
                + draw_primitive_heap_bytes(cutout)
        }
    }
}

fn annotated_string_heap_bytes(text: &AnnotatedString) -> usize {
    text.text.capacity()
        + text.span_styles.capacity() * size_of::<usize>() * 2
        + text.paragraph_styles.capacity() * size_of::<usize>() * 2
        + text.string_annotations.capacity() * size_of::<usize>() * 2
        + text.link_annotations.capacity() * size_of::<usize>() * 2
        + text
            .string_annotations
            .iter()
            .map(|annotation| {
                annotation.item.tag.capacity() + annotation.item.annotation.capacity()
            })
            .sum::<usize>()
        + text
            .link_annotations
            .iter()
            .map(|annotation| match &annotation.item {
                cranpose_ui::text::LinkAnnotation::Url(url) => url.capacity(),
                cranpose_ui::text::LinkAnnotation::Clickable { tag, .. } => tag.capacity(),
            })
            .sum::<usize>()
}

pub fn quad_bounds(quad: [[f32; 2]; 4]) -> Rect {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for [x, y] in quad {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }

    Rect {
        x: min_x,
        y: min_y,
        width: (max_x - min_x).max(0.0),
        height: (max_y - min_y).max(0.0),
    }
}

fn multiply_matrices(lhs: [[f32; 3]; 3], rhs: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for row in 0..3 {
        for col in 0..3 {
            out[row][col] =
                lhs[row][0] * rhs[0][col] + lhs[row][1] * rhs[1][col] + lhs[row][2] * rhs[2][col];
        }
    }
    out
}

fn solve_homography(source: [[f32; 2]; 4], target: [[f32; 2]; 4]) -> Option<[f32; 8]> {
    let mut matrix = [[0.0f32; 9]; 8];
    for (index, (src, dst)) in source.into_iter().zip(target).enumerate() {
        let row = index * 2;
        let x = src[0];
        let y = src[1];
        let u = dst[0];
        let v = dst[1];

        matrix[row] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        matrix[row + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }

    for pivot in 0..8 {
        let mut pivot_row = pivot;
        let mut pivot_value = matrix[pivot][pivot].abs();
        let mut candidate = pivot + 1;
        while candidate < 8 {
            let candidate_value = matrix[candidate][pivot].abs();
            if candidate_value > pivot_value {
                pivot_row = candidate;
                pivot_value = candidate_value;
            }
            candidate += 1;
        }

        if pivot_value <= f32::EPSILON {
            return None;
        }

        if pivot_row != pivot {
            matrix.swap(pivot, pivot_row);
        }

        let divisor = matrix[pivot][pivot];
        let mut col = pivot;
        while col < 9 {
            matrix[pivot][col] /= divisor;
            col += 1;
        }

        for row in 0..8 {
            if row == pivot {
                continue;
            }
            let factor = matrix[row][pivot];
            if factor.abs() <= f32::EPSILON {
                continue;
            }
            let mut col = pivot;
            while col < 9 {
                matrix[row][col] -= factor * matrix[pivot][col];
                col += 1;
            }
        }
    }

    let mut solution = [0.0f32; 8];
    for index in 0..8 {
        solution[index] = matrix[index][8];
    }
    Some(solution)
}

fn inflate_rect(rect: Rect, by: f32) -> Rect {
    Rect {
        x: rect.x - by,
        y: rect.y - by,
        width: rect.width + by * 2.0,
        height: rect.height + by * 2.0,
    }
}

fn rect_within(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

#[cfg(test)]
#[path = "tests/graph_tests.rs"]
mod tests;
