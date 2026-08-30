use std::{collections::HashSet, mem::size_of, rc::Rc};

use cranpose_core::NodeId;
use cranpose_foundation::PointerEvent;
use cranpose_ui::{
    GraphicsLayer, Point, Rect, RenderEffect, RoundedCornerShape, TextLayoutOptions, TextStyle,
    text::AnnotatedString,
};
use cranpose_ui_graphics::{BlendMode, ColorFilter, DrawPrimitive, ShadowPrimitive};

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
    pub click_actions: Vec<Rc<dyn Fn(Point)>>,
    pub pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    pub clip: Option<Rect>,
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
    pub text_style: TextStyle,
    pub font_size: f32,
    pub layout_options: TextLayoutOptions,
    pub clip: Option<Rect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitivePhase {
    BeforeChildren,
    AfterChildren,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PrimitiveNode {
    Draw(DrawPrimitiveNode),
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
    pub local_bounds: Rect,
    pub transform_to_parent: ProjectiveTransform,
    pub content_offset: Point,
    pub motion_context_animated: bool,
    pub translated_content_context: bool,
    pub translated_content_offset: Point,
    /// Window-space origin this layer's children are placed from, and the
    /// accumulated ancestor graphics-layer translation, captured during the
    /// full per-frame scene build. Read back when a dirty subtree is rebuilt in
    /// isolation (`update_scene_from_applier`) so a scrolling field's window
    /// origin stays live during a fling even though the ancestor chain is not
    /// re-walked from the root. Defaults to the identity origin.
    pub scene_children_origin: Point,
    pub scene_children_layer_translation: Point,
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
            local_bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            transform_to_parent: ProjectiveTransform::identity(),
            content_offset: Point::default(),
            motion_context_animated: false,
            translated_content_context: false,
            translated_content_offset: Point::default(),
            scene_children_origin: Point::default(),
            scene_children_layer_translation: Point::default(),
            graphics_layer: GraphicsLayer::default(),
            clip_to_bounds: false,
            shadow_clip: None,
            hit_test: None,
            has_hit_targets: false,
            has_origin_sinks: false,
            isolation: IsolationReasons::default(),
            cache_policy: CachePolicy::None,
            cache_hashes: LayerRasterCacheHashes::default(),
            cache_hashes_valid: false,
            children: Vec::new(),
        }
    }
}

impl LayerNode {
    pub fn clip_rect(&self) -> Option<Rect> {
        (self.clip_to_bounds || self.graphics_layer.clip).then_some(self.local_bounds)
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
    /// keeps a handle to the same buffer, so its capacity survives this
    /// node being dropped on the next rebuild and the command re-records
    /// into it instead of growing a fresh vector. Nothing mutates a run's
    /// primitives after construction, which is what makes sharing sound.
    pub primitives: std::rc::Rc<Vec<DrawPrimitive>>,
    /// Content facts consumers keep asking per frame, answered once at
    /// construction. Surface planning used to rescan every primitive of
    /// every run per frame to learn "does it contain text?" — for a
    /// 17k-primitive game canvas with no text, that was two full walks per
    /// frame that could never early-exit.
    pub summary: DrawRunSummary,
    /// The command's verified retained/dynamic span structure for the frame
    /// this node was built, in primitive space. A renderer that retains by
    /// identity draws the run span by span — retained spans from slots
    /// keyed (command, slot), dynamic spans from `primitives` — instead of
    /// walking the whole vector. `None` means no verification ran or
    /// nothing was retained: the run is all ordinary primitives.
    pub replay: Option<Box<cranpose_ui_graphics::CommandReplayFrame>>,
}

impl DrawRunNode {
    pub fn new(phase: PrimitivePhase, primitives: Vec<DrawPrimitive>) -> Self {
        Self::for_command(phase, None, primitives)
    }

    pub fn for_command(
        phase: PrimitivePhase,
        command: Option<DrawCommandId>,
        primitives: Vec<DrawPrimitive>,
    ) -> Self {
        Self::for_command_shared(phase, command, std::rc::Rc::new(primitives))
    }

    pub fn for_command_shared(
        phase: PrimitivePhase,
        command: Option<DrawCommandId>,
        primitives: std::rc::Rc<Vec<DrawPrimitive>>,
    ) -> Self {
        Self::for_command_replayed(phase, command, primitives, None)
    }

    pub fn for_command_replayed(
        phase: PrimitivePhase,
        command: Option<DrawCommandId>,
        primitives: std::rc::Rc<Vec<DrawPrimitive>>,
        replay: Option<Box<cranpose_ui_graphics::CommandReplayFrame>>,
    ) -> Self {
        debug_assert!(
            replay.as_ref().is_none_or(|frame| {
                frame.fallback.is_some()
                    || !frame.spans.iter().any(|span| {
                        matches!(
                            span,
                            cranpose_ui_graphics::FrameSpan::Retained {
                                capture: false,
                                range,
                                ..
                            } if range.1 <= range.0
                        )
                    })
            }),
            "a replay frame with bypassed spans must own its fallback recording"
        );
        let mut summary = DrawRunSummary::scan(&primitives);
        if replay.as_ref().is_some_and(|frame| {
            frame
                .spans
                .iter()
                .any(|span| matches!(span, cranpose_ui_graphics::FrameSpan::Retained { .. }))
        }) {
            summary.has_non_shadow = true;
        }
        Self {
            phase,
            command,
            primitives,
            summary,
            replay,
        }
    }
}

/// One-pass discriminant census of a draw run, recursing through `Blend`
/// wrappers the same way the per-primitive predicates it replaces did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawRunSummary {
    /// Any `Text` primitive, including inside `Blend` — glyph masks want
    /// rigid snapping.
    pub has_text: bool,
    pub has_shadow: bool,
    /// Any primitive besides `Shadow` (direct drawable content).
    pub has_non_shadow: bool,
    /// Any `Image` or `Text`, including inside `Blend` — content that
    /// resamples badly on a fractionally offset surface.
    pub has_pixel_sensitive: bool,
}

impl DrawRunSummary {
    pub fn scan(primitives: &[DrawPrimitive]) -> Self {
        fn unwrap_blend(mut primitive: &DrawPrimitive) -> &DrawPrimitive {
            while let DrawPrimitive::Blend {
                primitive: inner, ..
            } = primitive
            {
                primitive = inner;
            }
            primitive
        }
        let mut summary = Self::default();
        for primitive in primitives {
            if matches!(primitive, DrawPrimitive::Shadow(_)) {
                summary.has_shadow = true;
                continue;
            }
            summary.has_non_shadow = true;
            match unwrap_blend(primitive) {
                DrawPrimitive::Text(_) => {
                    summary.has_text = true;
                    summary.has_pixel_sensitive = true;
                }
                DrawPrimitive::Image { .. } => summary.has_pixel_sensitive = true,
                _ => {}
            }
        }
        summary
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
                    RenderNode::DrawRun(run) => run.primitives.len(),
                    RenderNode::Layer(child_layer) => count_layer(child_layer),
                })
                .sum::<usize>()
        }

        count_layer(&self.root)
    }

    pub fn heap_bytes(&self) -> usize {
        layer_heap_bytes(&self.root)
    }

    pub fn retained_visual_observation_nodes(&self) -> HashSet<NodeId> {
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

        let mut nodes = HashSet::new();
        collect(&self.root, &mut nodes);
        nodes
    }
}

fn layer_heap_bytes(layer: &LayerNode) -> usize {
    layer.hit_test.as_ref().map_or(0, hit_test_heap_bytes)
        + size_of::<RenderNode>() * layer.children.capacity()
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
            size_of::<DrawPrimitive>() * run.primitives.capacity()
                + run
                    .primitives
                    .iter()
                    .map(draw_primitive_heap_bytes)
                    .sum::<usize>()
        }
        RenderNode::Layer(layer) => size_of::<LayerNode>() + layer_heap_bytes(layer),
    }
}

fn primitive_entry_heap_bytes(entry: &PrimitiveEntry) -> usize {
    match &entry.node {
        PrimitiveNode::Draw(draw) => draw_primitive_heap_bytes(&draw.primitive),
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
        | DrawPrimitive::Arc { .. } => 0,
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
                    .map_or(0, |family| family.capacity())
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

fn hit_test_heap_bytes(hit_test: &HitTestNode) -> usize {
    hit_test.click_actions.capacity() * size_of::<Rc<dyn Fn(Point)>>()
        + hit_test.pointer_inputs.capacity() * size_of::<Rc<dyn Fn(PointerEvent)>>()
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

#[cfg(test)]
mod tests {
    use cranpose_ui_graphics::{Brush, Color, DrawPrimitive};

    use super::*;

    fn test_layer(local_bounds: Rect, children: Vec<RenderNode>) -> LayerNode {
        LayerNode {
            local_bounds,
            children,
            ..Default::default()
        }
    }

    #[test]
    fn projective_transform_translation_maps_points() {
        let transform = ProjectiveTransform::translation(7.0, -3.5);
        let mapped = transform.map_point(Point { x: 2.0, y: 4.0 });
        assert!((mapped.x - 9.0).abs() < 1e-6);
        assert!((mapped.y - 0.5).abs() < 1e-6);
    }

    #[test]
    fn projective_transform_then_composes_in_parent_order() {
        let child = ProjectiveTransform::translation(4.0, 2.0);
        let parent = ProjectiveTransform::translation(10.0, -1.0);
        let composed = child.then(parent);
        let mapped = composed.map_point(Point { x: 1.0, y: 1.0 });
        assert!((mapped.x - 15.0).abs() < 1e-6);
        assert!((mapped.y - 2.0).abs() < 1e-6);
    }

    #[test]
    fn homography_maps_rect_corners_to_target_quad() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 10.0,
        };
        let quad = [[5.0, 7.0], [25.0, 6.0], [7.0, 20.0], [28.0, 21.0]];
        let transform = ProjectiveTransform::from_rect_to_quad(rect, quad);
        let mapped = transform.map_rect(rect);
        for (expected, actual) in quad.into_iter().zip(mapped) {
            assert!((expected[0] - actual[0]).abs() < 1e-4);
            assert!((expected[1] - actual[1]).abs() < 1e-4);
        }
    }

    #[test]
    fn axis_aligned_rect_to_quad_keeps_exact_affine_matrix() {
        let rect = Rect {
            x: 2.0,
            y: 3.0,
            width: 20.0,
            height: 10.0,
        };
        let quad = [[12.0, 9.0], [32.0, 9.0], [12.0, 19.0], [32.0, 19.0]];
        let transform = ProjectiveTransform::from_rect_to_quad(rect, quad);

        assert_eq!(
            transform.matrix(),
            [[1.0, 0.0, 10.0], [0.0, 1.0, 6.0], [0.0, 0.0, 1.0]]
        );
    }

    #[test]
    fn axis_aligned_rect_to_quad_keeps_exact_axis_aligned_scale() {
        let rect = Rect {
            x: 4.0,
            y: 6.0,
            width: 10.0,
            height: 8.0,
        };
        let quad = [[20.0, 18.0], [50.0, 18.0], [20.0, 42.0], [50.0, 42.0]];
        let transform = ProjectiveTransform::from_rect_to_quad(rect, quad);

        assert_eq!(
            transform.matrix(),
            [[3.0, 0.0, 8.0], [0.0, 3.0, 0.0], [0.0, 0.0, 1.0]]
        );
    }

    #[test]
    fn retained_visual_observation_nodes_collect_layers_and_command_owners() {
        let bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 20.0,
        };
        let command = |node_id| DrawCommandId {
            node_id,
            command_index: 0,
            placement: DrawPlacement::Behind,
        };
        let mut child = test_layer(
            bounds,
            vec![RenderNode::DrawRun(DrawRunNode::for_command(
                PrimitivePhase::BeforeChildren,
                Some(command(17)),
                Vec::new(),
            ))],
        );
        child.node_id = Some(13);
        let mut root = test_layer(
            bounds,
            vec![
                RenderNode::DrawRun(DrawRunNode::for_command(
                    PrimitivePhase::BeforeChildren,
                    Some(command(9)),
                    Vec::new(),
                )),
                RenderNode::DrawRun(DrawRunNode::new(PrimitivePhase::BeforeChildren, Vec::new())),
                RenderNode::Layer(Box::new(child)),
            ],
        );

        root.node_id = Some(5);

        assert_eq!(
            RenderGraph::new(root).retained_visual_observation_nodes(),
            HashSet::from([5, 9, 13, 17])
        );
    }

    #[test]
    fn render_graph_new_recomputes_manual_layer_hashes() {
        let primitive = PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Rect {
                    rect: Rect {
                        x: 1.0,
                        y: 2.0,
                        width: 8.0,
                        height: 6.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                    stroke: None,
                },
                clip: None,
            }),
        };
        let mut root = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            vec![RenderNode::Primitive(primitive)],
        );
        root.graphics_layer.render_effect = Some(RenderEffect::blur(3.0));
        let mut expected = root.clone();
        expected.recompute_raster_cache_hashes();

        let graph = RenderGraph::new(root);
        assert_eq!(
            graph.root.target_content_hash(),
            expected.target_content_hash()
        );
        assert_eq!(graph.root.effect_hash(), expected.effect_hash());
    }

    #[test]
    fn motion_source_content_hash_ignores_translated_content_offset() {
        let primitive = PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Rect {
                    rect: Rect {
                        x: 1.0,
                        y: 2.0,
                        width: 8.0,
                        height: 6.0,
                    },
                    brush: Brush::solid(Color::WHITE),
                    stroke: None,
                },
                clip: None,
            }),
        };
        let mut base = test_layer(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            vec![RenderNode::Primitive(primitive)],
        );
        base.translated_content_context = true;
        base.translated_content_offset = Point::new(0.0, -24.0);
        base.recompute_raster_cache_hashes();

        let mut moved = base.clone();
        moved.translated_content_offset = Point::new(0.0, -72.0);
        moved.recompute_raster_cache_hashes();

        assert_ne!(base.target_content_hash(), moved.target_content_hash());
        assert_eq!(
            base.motion_source_content_hash(),
            moved.motion_source_content_hash()
        );
    }
}
