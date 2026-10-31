// How a layout node lowers into the layer that draws it: its layer
// properties, its own draws and text around its child layers, and the
// wrapper of the draws chained before its graphics layer.

use std::{any::Any, borrow::Cow, rc::Rc};

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_ui::{
    DrawCommand, LayoutBox, LayoutNode, ModifierNodeSlices, Point, Rect, Size, SubcomposeLayoutNode,
};
use cranpose_ui_graphics::{CompositingStrategy, DrawPlacement, GraphicsLayer, LayerShape};

use super::{
    BuildNodeSnapshot, DrawPass, DrawTarget, Level, SnapshotNodeData, TextNodeParts,
    TranslateAncestorContext, TranslateGeometry, append_draw_nodes, draw_nodes,
    inline_leaf::{lower_child, snapshot_lowers_inline},
    layer_node_capacity,
    layer_shape::{clip_shape, shape_clip},
    note_layer_lowered, note_space_change, shaped_graphics_layer, text_node_from_parts,
};
use crate::{
    graph::{
        CachePolicy, ContentChanges, HitTestNode, InlineNode, IsolationReasons, LayerNode,
        PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform, RenderNode,
    },
    layer_transform::{
        layer_scales_or_rotates, layer_transform_to_parent, layer_transform_to_window,
    },
};

#[derive(Clone, Copy)]
pub(super) struct LowerContext {
    pub(super) inherited_motion_context_animated: bool,
    pub(super) inherited_translated_content_context: bool,
    pub(super) parent_abs: Option<AbsOrigin>,
    pub(super) parent_content_offset: Point,
}

impl LowerContext {
    pub(super) const ROOT: Self = Self {
        inherited_motion_context_animated: false,
        inherited_translated_content_context: false,
        parent_abs: Some(AbsOrigin::ROOT),
        parent_content_offset: Point { x: 0.0, y: 0.0 },
    };

    // What a child lowers with beneath `ancestors`.
    pub(super) fn beneath(ancestors: TranslateAncestorContext) -> Self {
        Self {
            inherited_motion_context_animated: ancestors.inherited_motion_context_animated,
            inherited_translated_content_context: ancestors.inherited_translated_content_context,
            parent_abs: Some(ancestors.parent_abs),
            parent_content_offset: ancestors.parent_content_offset,
        }
    }

    fn for_children(
        self,
        slices: &ModifierNodeSlices,
        content_offset: Point,
        parent_abs: Option<AbsOrigin>,
    ) -> Self {
        Self {
            inherited_motion_context_animated: self.inherited_motion_context_animated
                || slices.motion_context_animated(),
            inherited_translated_content_context: self.inherited_translated_content_context
                || slices.translated_content_context(),
            parent_abs,
            parent_content_offset: content_offset,
        }
    }
}

pub(super) struct LayerHead {
    pub(super) node_id: Option<NodeId>,
    pub(super) wraps: Option<NodeId>,
    pub(super) local_bounds: Rect,
    pub(super) node_bounds: Option<Rect>,
    pub(super) transform_to_parent: ProjectiveTransform,
    pub(super) content_offset: Point,
    pub(super) motion_context_animated: bool,
    pub(super) translated_content_context: bool,
    pub(super) translated_content_offset: Point,
    pub(super) origin_in_parent: Point,
    pub(super) graphics_layer: Option<GraphicsLayer>,
    pub(super) clip_to_bounds: bool,
    pub(super) hit_test: Option<HitTestNode>,
    pub(super) has_origin_sinks: bool,
    pub(super) isolation: IsolationReasons,
    pub(super) cache_policy: CachePolicy,
    pub(super) draw_phase: Option<PrimitivePhase>,
}

struct NodeFrame {
    local_bounds: Rect,
    node_bounds: Option<Rect>,
    placement: Point,
    content_offset: Point,
    translated_content_offset: Point,
}

fn node_layer_head(
    node_id: NodeId,
    slices: &Rc<ModifierNodeSlices>,
    frame: NodeFrame,
    graphics_layer: Option<GraphicsLayer>,
    context: LowerContext,
) -> LayerHead {
    let clip_to_bounds = slices.clip_to_bounds();
    let translated_content_context = slices.translated_content_context();
    let properties = graphics_layer.as_ref().unwrap_or(&GraphicsLayer::DEFAULT);
    let isolation = isolation_reasons(properties);
    LayerHead {
        node_id: Some(node_id),
        wraps: None,
        local_bounds: frame.local_bounds,
        node_bounds: frame.node_bounds,
        transform_to_parent: placed_transform(
            frame.local_bounds,
            frame.placement,
            graphics_layer.as_ref(),
            context.parent_content_offset,
        ),
        content_offset: frame.content_offset,
        motion_context_animated: context.inherited_motion_context_animated
            || slices.motion_context_animated(),
        translated_content_context,
        translated_content_offset: if translated_content_context {
            frame.translated_content_offset
        } else {
            Point::default()
        },
        origin_in_parent: frame.placement,
        cache_policy: layer_cache_policy(properties, isolation),
        isolation,
        graphics_layer,
        clip_to_bounds,
        hit_test: hit_test_from_slices(slices),
        has_origin_sinks: modifier_slices_have_origin_sinks(slices),
        draw_phase: None,
    }
}

pub(super) fn assign_layer(layer: &mut LayerNode, head: LayerHead) {
    let LayerNode {
        node_id,
        wraps,
        local_bounds,
        node_bounds,
        transform_to_parent,
        content_offset,
        motion_context_animated,
        translated_content_context,
        translated_content_offset,
        origin_in_parent,
        graphics_layer,
        clip_to_bounds,
        hit_test,
        has_hit_targets,
        space,
        has_origin_sinks,
        contained,
        isolation,
        draw_phase,
        cache_policy,
        cache_hashes: _,
        content_changes,
        change: _,
        own_change: _,
        children,
    } = layer;
    if *node_id != head.node_id {
        *content_changes = ContentChanges::default();
    }
    *node_id = head.node_id;
    *wraps = head.wraps;
    *local_bounds = head.local_bounds;
    *node_bounds = head.node_bounds;
    *transform_to_parent = head.transform_to_parent;
    *content_offset = head.content_offset;
    *motion_context_animated = head.motion_context_animated;
    *translated_content_context = head.translated_content_context;
    *translated_content_offset = head.translated_content_offset;
    *origin_in_parent = head.origin_in_parent;
    graphics_layer.replace(head.graphics_layer);
    if graphics_layer.space.is_some() {
        note_space_change();
    }
    *clip_to_bounds = head.clip_to_bounds;
    *hit_test = head.hit_test;
    *has_hit_targets = false;
    space.set(None);
    *has_origin_sinks = head.has_origin_sinks || children_have_origin_sinks(children);
    contained.set(None);
    *isolation = head.isolation;
    *draw_phase = head.draw_phase;
    *cache_policy = head.cache_policy;
    layer.forget_own_raster_cache_hashes();
    layer.refresh_child_facts();
}

pub(super) fn children_have_origin_sinks(children: &[RenderNode]) -> bool {
    children.iter().any(|child| match child {
        RenderNode::Layer(child_layer) => child_layer.has_origin_sinks,
        RenderNode::Primitive(_) | RenderNode::DrawRun(_) | RenderNode::Inline(_) => false,
    })
}

/// The transform that places a layer with `graphics_layer`, `None` for the
/// default properties, at `placement` in a parent whose content is offset by
/// `parent_content_offset`.
pub(super) fn placed_transform(
    local_bounds: Rect,
    placement: Point,
    graphics_layer: Option<&GraphicsLayer>,
    parent_content_offset: Point,
) -> ProjectiveTransform {
    // Most layers only move: one translation places them.
    let graphics_layer = match graphics_layer {
        Some(layer) if layer_scales_or_rotates(layer) => layer,
        moved => {
            let offset = moved.map_or_else(Point::default, |layer| Point {
                x: layer.translation_x,
                y: layer.translation_y,
            });
            return ProjectiveTransform::translation(
                placement.x + offset.x + parent_content_offset.x,
                placement.y + offset.y + parent_content_offset.y,
            );
        }
    };
    let transform = layer_transform_to_parent(local_bounds, placement, graphics_layer);
    if parent_content_offset == Point::default() {
        transform
    } else {
        transform.then(ProjectiveTransform::translation(
            parent_content_offset.x,
            parent_content_offset.y,
        ))
    }
}

/// Writes a node's own draws and text around its child layers: `kept`
/// layers already end `list`, and `write_children` appends `written` more.
pub(super) fn write_node_content(
    list: &mut Vec<RenderNode>,
    node_id: NodeId,
    slices: &ModifierNodeSlices,
    draws: DrawTarget,
    (kept, written): (usize, usize),
    write_children: impl FnOnce(&mut Vec<RenderNode>),
) {
    let size = draws.size;
    let outer_count = slices.outer_draw_command_count();
    let commands = &slices.draw_commands()[outer_count..];
    crate::layer_recycling::reserve_nodes(
        list,
        layer_node_capacity(commands, written, slices.has_text()),
    );
    let first_kept = list.len() - kept;
    let first_draw = list.len();
    append_draw_nodes(
        list,
        node_id,
        commands,
        outer_count,
        DrawPass::Record(DrawPlacement::Behind),
        draws,
        PrimitivePhase::BeforeChildren,
    );
    if let Some(text) = text_node_from_parts(&TextNodeParts::of(node_id, slices, size)) {
        list.push(RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Text(crate::layer_recycling::boxed_text(text)),
        }));
    }
    // The kept layers go after the draws and text just written.
    list[first_kept..].rotate_left(kept);
    let first_draw = first_draw - kept;
    write_children(list);
    append_draw_nodes(
        list,
        node_id,
        commands,
        outer_count,
        DrawPass::OverlayFromOutput(first_draw),
        draws,
        PrimitivePhase::AfterChildren,
    );
}

fn write_layer_with_outer(
    target: &mut LayerNode,
    head: LayerHead,
    outer: Option<OuterDraws>,
    placement: Point,
    parent_content_offset: Point,
    write_children: impl FnOnce(&mut Vec<RenderNode>, Option<&GraphicsLayer>),
) {
    debug_assert!(target.children.is_empty(), "a layer is written emptied");
    let Some(outer) = outer else {
        write_children(&mut target.children, head.graphics_layer.as_ref());
        assign_layer(target, head);
        return;
    };
    let mut layer = crate::layer_recycling::layer_box();
    write_children(&mut layer.children, head.graphics_layer.as_ref());
    assign_layer(&mut layer, head);
    write_wrapper(target, layer, placement, outer, parent_content_offset);
}

pub(super) fn write_snapshot_layer(
    snapshot: BuildNodeSnapshot,
    context: LowerContext,
    target: &mut LayerNode,
) {
    let BuildNodeSnapshot {
        node_id,
        placement,
        size,
        content_offset,
        slices,
        graphics_layer,
        children,
    } = snapshot;
    let (local_bounds, node_bounds) = layer_and_node_bounds(&slices, size);
    let child_context = context.for_children(&slices, content_offset, None);
    let head = node_layer_head(
        node_id,
        &slices,
        NodeFrame {
            local_bounds,
            node_bounds,
            placement,
            content_offset,
            translated_content_offset: content_offset,
        },
        graphics_layer,
        context,
    );
    let draws = DrawTarget {
        size,
        motion_context_animated: head.motion_context_animated,
        layer: None,
    };
    let outer = outer_draws(
        node_id,
        slices.draw_commands(),
        slices.outer_draw_command_count(),
        draws,
    );
    write_layer_with_outer(
        target,
        head,
        outer,
        placement,
        context.parent_content_offset,
        |list, layer| {
            let draws = DrawTarget { layer, ..draws };
            write_node_content(list, node_id, &slices, draws, (0, children.len()), |list| {
                for child in children {
                    list.push(lower_snapshot_child(child, child_context));
                }
            });
        },
    );
}

// Lowers `snapshot`, a child in a layout tree snapshot, into the node its
// parent holds: inline when it needs no layer of its own, as
// [`super::inline_leaf::lower_child`] lowers a node of the applier.
fn lower_snapshot_child(snapshot: BuildNodeSnapshot, context: LowerContext) -> RenderNode {
    if !snapshot_lowers_inline(&snapshot) {
        let mut layer = crate::layer_recycling::layer_box();
        write_snapshot_layer(snapshot, context, &mut layer);
        return RenderNode::Layer(layer);
    }
    let BuildNodeSnapshot {
        node_id,
        placement,
        size,
        content_offset,
        slices,
        ..
    } = snapshot;
    let motion_context_animated =
        context.inherited_motion_context_animated || slices.motion_context_animated();
    let draws = DrawTarget {
        size,
        motion_context_animated,
        layer: None,
    };
    let mut content = Vec::new();
    write_node_content(&mut content, node_id, &slices, draws, (0, 0), |_| {});
    if content
        .iter()
        .any(|node| matches!(node, RenderNode::Layer(_)))
    {
        let (local_bounds, node_bounds) = layer_and_node_bounds(&slices, size);
        let frame = NodeFrame {
            local_bounds,
            node_bounds,
            placement,
            content_offset,
            translated_content_offset: content_offset,
        };
        let mut layer = crate::layer_recycling::layer_box();
        layer.children = content;
        assign_layer(
            &mut layer,
            node_layer_head(node_id, &slices, frame, None, context),
        );
        return RenderNode::Layer(layer);
    }
    let origin = Point::new(
        placement.x + context.parent_content_offset.x,
        placement.y + context.parent_content_offset.y,
    );
    let mut inline = Box::<InlineNode>::default();
    inline.node_id = node_id;
    inline.origin = origin;
    inline.size = size;
    inline.motion_context_animated = motion_context_animated;
    inline.content = content;
    RenderNode::Inline(inline)
}

#[derive(Clone, Copy)]
pub(super) struct AbsOrigin {
    pub(super) content_origin: Point,
    pub(super) window_transform: ProjectiveTransform,
}

impl AbsOrigin {
    pub(super) const ROOT: Self = Self {
        content_origin: Point { x: 0.0, y: 0.0 },
        window_transform: ProjectiveTransform::identity(),
    };

    pub(super) fn children_of(self, layer: &LayerNode) -> Self {
        let top_left = Point {
            x: self.content_origin.x + layer.origin_in_parent.x,
            y: self.content_origin.y + layer.origin_in_parent.y,
        };
        Self {
            content_origin: Point {
                x: self.content_origin.x + layer.origin_in_parent.x + layer.content_offset.x,
                y: self.content_origin.y + layer.origin_in_parent.y + layer.content_offset.y,
            },
            window_transform: layer_transform_to_window(
                self.window_transform,
                top_left,
                layer.local_bounds,
                &layer.graphics_layer,
            ),
        }
    }
}

pub(super) fn read_placed_node_data<R>(
    applier: &MemoryApplier,
    node_id: NodeId,
    level: Level,
    read: impl FnOnce(SnapshotNodeData<'_>) -> R,
) -> Option<R> {
    read_node_data(applier, node_id, |mut data| {
        if data.window_root {
            if level == Level::Child {
                return None;
            }
            data.layout_state = data.layout_state.at_origin();
        }
        note_layer_lowered();
        if !data.layout_state.is_placed() {
            return None;
        }
        Some(read(data))
    })?
}

pub(super) fn lower_root_into(
    applier: &MemoryApplier,
    node_id: NodeId,
    target: &mut LayerNode,
) -> bool {
    read_placed_node_data(applier, node_id, Level::Root, |data| {
        write_node_layer(applier, node_id, data, LowerContext::ROOT, target);
    })
    .is_some()
}

/// Where a placed child sits in its parent and how large it is.
#[derive(Clone, Copy)]
pub(super) struct PlacedGeometry {
    pub(super) position: Point,
    pub(super) size: Size,
}

impl PlacedGeometry {
    fn of_placed(state: &cranpose_ui::widgets::LayoutState) -> Option<Self> {
        state.is_placed().then(|| Self {
            position: state.position(),
            size: state.size(),
        })
    }
}

/// The geometry of the placed child `node_id`; `None` for a child that is
/// not placed or is a window root, drawn in a surface of its own.
pub(super) fn placed_child_geometry(
    applier: &MemoryApplier,
    node_id: NodeId,
) -> Option<PlacedGeometry> {
    let node: &dyn Any = applier.get_ref(node_id).ok()?;
    if let Some(node) = node.downcast_ref::<LayoutNode>() {
        if node.is_window_root() {
            return None;
        }
        return node.with_layout_state(PlacedGeometry::of_placed);
    }
    PlacedGeometry::of_placed(&node.downcast_ref::<SubcomposeLayoutNode>()?.layout_state())
}

pub(super) fn read_node_data<R>(
    applier: &MemoryApplier,
    node_id: NodeId,
    read: impl FnOnce(SnapshotNodeData<'_>) -> R,
) -> Option<R> {
    let node: &dyn Any = applier.get_ref(node_id).ok()?;
    if let Some(node) = node.downcast_ref::<LayoutNode>() {
        let z_order = z_ordered_children(applier, &node.children);
        return Some(read(SnapshotNodeData {
            layout_state: node.layout_state(),
            modifier_slices: node.modifier_slices_snapshot(),
            children: z_order.as_deref().unwrap_or(&node.children),
            window_root: node.is_window_root(),
        }));
    }
    let node = node.downcast_ref::<SubcomposeLayoutNode>()?;
    let layout_state = node.layout_state();
    let modifier_slices = node.modifier_slices_snapshot();
    Some(node.with_active_children(|children| {
        let z_order = z_ordered_children(applier, children);
        read(SnapshotNodeData {
            layout_state,
            modifier_slices,
            children: z_order.as_deref().unwrap_or(children),
            window_root: false,
        })
    }))
}

/// `children` in the order they draw and take taps, when a `z_index`
/// modifier moves one of them: by z index, and in composition order among
/// equal ones, as Compose's `zSortedChildren`. `None` while the composition
/// order already is that order, which it always is while no `z_index`
/// modifier exists.
#[inline]
fn z_ordered_children(applier: &MemoryApplier, children: &[NodeId]) -> Option<Vec<NodeId>> {
    if children.len() < 2 || !cranpose_ui::z_index_in_use() {
        return None;
    }
    z_sorted_children(applier, children)
}

#[inline(never)]
fn z_sorted_children(applier: &MemoryApplier, children: &[NodeId]) -> Option<Vec<NodeId>> {
    let mut previous = f32::NEG_INFINITY;
    let in_order = children.iter().all(|&child| {
        let z_index = node_z_index(applier, child);
        let ordered = z_index >= previous;
        previous = z_index;
        ordered
    });
    if in_order {
        return None;
    }
    let mut ordered: Vec<(f32, NodeId)> = children
        .iter()
        .map(|&child| (node_z_index(applier, child), child))
        .collect();
    ordered.sort_by(|a, b| a.0.total_cmp(&b.0));
    Some(ordered.into_iter().map(|(_, child)| child).collect())
}

fn node_z_index(applier: &MemoryApplier, node_id: NodeId) -> f32 {
    let Ok(node) = applier.get_ref(node_id) else {
        return 0.0;
    };
    let node: &dyn Any = node;
    if let Some(node) = node.downcast_ref::<LayoutNode>() {
        return node.modifier_slices_snapshot().z_index();
    }
    node.downcast_ref::<SubcomposeLayoutNode>()
        .map_or(0.0, |node| node.modifier_slices_snapshot().z_index())
}

pub(super) fn hit_test_from_slices(slices: &Rc<ModifierNodeSlices>) -> Option<HitTestNode> {
    slices_hit_something(slices).then(|| HitTestNode {
        handlers: Rc::clone(slices),
    })
}

/// A node's layer bounds and, when they differ from it, its own rect (see
/// [`LayerNode::node_bounds`]).
pub(super) fn layer_and_node_bounds(
    slices: &ModifierNodeSlices,
    size: Size,
) -> (Rect, Option<Rect>) {
    let node_bounds = Rect::from_size(size);
    let layer_bounds = slices.layer_bounds(size);
    (
        layer_bounds,
        (layer_bounds != node_bounds).then_some(node_bounds),
    )
}

/// Whether a node's slices make it a hit target: a pointer input or a
/// pointer icon.
pub(super) fn slices_hit_something(slices: &ModifierNodeSlices) -> bool {
    !slices.pointer_inputs().is_empty() || slices.pointer_icon().is_some()
}

pub(super) fn prepare_node_layer(
    node_id: NodeId,
    slices: &Rc<ModifierNodeSlices>,
    layout_state: &cranpose_ui::widgets::LayoutState,
    context: LowerContext,
) -> (LayerHead, LowerContext) {
    let size = layout_state.size();
    let placement = layout_state.position();
    let (local_bounds, node_bounds) = layer_and_node_bounds(slices, size);
    let graphics_layer = shaped_graphics_layer(slices, local_bounds);
    let geometry = context.parent_abs.map(|parent_abs| {
        TranslateGeometry::new(
            layout_state,
            graphics_layer.as_deref().unwrap_or(&GraphicsLayer::DEFAULT),
            local_bounds,
            parent_abs,
        )
    });
    if let Some(geometry) = geometry {
        slices.publish_window_geometry(geometry.top_left, geometry.window_transform, size);
    } else {
        slices.publish_pointer_input_size(size);
    }
    let content_offset = layout_state.content_offset();
    let child_context = context.for_children(
        slices,
        content_offset,
        geometry.map(TranslateGeometry::child_abs),
    );
    let head = node_layer_head(
        node_id,
        slices,
        NodeFrame {
            local_bounds,
            node_bounds,
            placement,
            content_offset,
            translated_content_offset: slices.translated_content_offset().unwrap_or(content_offset),
        },
        graphics_layer.map(Cow::into_owned),
        context,
    );
    (head, child_context)
}

#[expect(clippy::print_stderr, reason = "CRANPOSE_SCENE_UPDATE_DIAG asks")]
pub(super) fn write_node_layer(
    applier: &MemoryApplier,
    node_id: NodeId,
    data: SnapshotNodeData<'_>,
    context: LowerContext,
    target: &mut LayerNode,
) {
    let SnapshotNodeData {
        layout_state,
        modifier_slices: slices,
        children,
        window_root: _,
    } = data;
    let size = layout_state.size();
    let placement = layout_state.position();
    if cranpose_core::DIAGNOSTICS.scene_update_diag.is_set() {
        eprintln!(
            "[scene-update-diag] build layer node={node_id:?} size=({:.2},{:.2}) pos=({:.2},{:.2})",
            size.width, size.height, placement.x, placement.y,
        );
    }
    let (head, child_context) = prepare_node_layer(node_id, &slices, &layout_state, context);
    let draws = DrawTarget {
        size,
        motion_context_animated: head.motion_context_animated,
        layer: None,
    };
    let outer = outer_draws(
        node_id,
        slices.draw_commands(),
        slices.outer_draw_command_count(),
        draws,
    );
    write_layer_with_outer(
        target,
        head,
        outer,
        placement,
        context.parent_content_offset,
        |list, layer| {
            let draws = DrawTarget { layer, ..draws };
            write_node_content(list, node_id, &slices, draws, (0, children.len()), |list| {
                for &child_id in children {
                    if let Some(child) = lower_child(applier, child_id, child_context) {
                        list.push(child);
                    }
                }
            });
        },
    );
}

pub(super) struct OuterDraws {
    behind: Vec<RenderNode>,
    overlay: Vec<RenderNode>,
}

pub(super) fn outer_draws(
    node_id: NodeId,
    draw_commands: &[DrawCommand],
    outer_draw_command_count: usize,
    draws: DrawTarget,
) -> Option<OuterDraws> {
    (outer_draw_command_count > 0).then(|| {
        let commands = &draw_commands[..outer_draw_command_count];
        let behind = draw_nodes(
            node_id,
            commands,
            0,
            DrawPlacement::Behind,
            draws,
            PrimitivePhase::BeforeChildren,
        );
        let mut overlay = crate::layer_recycling::child_list(commands.len());
        append_draw_nodes(
            &mut overlay,
            node_id,
            commands,
            0,
            DrawPass::OverlayFrom(behind.iter()),
            draws,
            PrimitivePhase::AfterChildren,
        );
        OuterDraws { behind, overlay }
    })
}

pub(super) fn write_wrapper(
    wrapper: &mut LayerNode,
    mut layer: Box<LayerNode>,
    placement: Point,
    outer: OuterDraws,
    parent_content_offset: Point,
) {
    debug_assert!(wrapper.children.is_empty(), "a wrapper is written emptied");
    layer.transform_to_parent =
        layer_transform_to_parent(layer.local_bounds, Point::default(), &layer.graphics_layer);
    layer.origin_in_parent = Point::default();
    let node_rect = layer.node_rect();
    let graphics_layer = None;
    let head = LayerHead {
        node_id: None,
        wraps: layer.node_id,
        local_bounds: node_rect,
        node_bounds: None,
        transform_to_parent: placed_transform(node_rect, placement, None, parent_content_offset),
        content_offset: Point::default(),
        motion_context_animated: layer.motion_context_animated,
        translated_content_context: false,
        translated_content_offset: Point::default(),
        origin_in_parent: placement,
        graphics_layer,
        clip_to_bounds: false,
        hit_test: None,
        has_origin_sinks: false,
        isolation: IsolationReasons::default(),
        cache_policy: CachePolicy::None,
        draw_phase: None,
    };
    let OuterDraws {
        mut behind,
        mut overlay,
    } = outer;
    let children = &mut wrapper.children;
    crate::layer_recycling::reserve_nodes(children, behind.len() + 1 + overlay.len());
    children.append(&mut behind);
    children.push(RenderNode::Layer(layer));
    children.append(&mut overlay);
    crate::layer_recycling::recycle_list(behind);
    crate::layer_recycling::recycle_list(overlay);
    assign_layer(wrapper, head);
}

pub(super) fn take_wrapped_layer(
    wrapper: &mut LayerNode,
    node_id: NodeId,
) -> Option<Box<LayerNode>> {
    let mut wrapped = None;
    for child in wrapper.children.drain(..) {
        if let RenderNode::Layer(layer) = child
            && layer.node_id == Some(node_id)
        {
            wrapped = Some(layer);
        }
    }
    wrapped
}

pub(super) fn layer_identity(layer: &LayerNode) -> Option<NodeId> {
    layer.node_id.or(layer.wraps)
}

pub(super) fn layout_box_to_snapshot(
    node: &LayoutBox,
    parent: Option<&LayoutBox>,
) -> BuildNodeSnapshot {
    let placement = parent
        .map(|parent_box| Point {
            x: node.rect.x - parent_box.rect.x - parent_box.content_offset.x,
            y: node.rect.y - parent_box.rect.y - parent_box.content_offset.y,
        })
        .unwrap_or_default();
    let mut children = Vec::with_capacity(node.children.len());
    for child in &node.children {
        children.push(layout_box_to_snapshot(child, Some(node)));
    }
    if cranpose_ui::z_index_in_use() {
        // Stable: equal z indices keep the composition order.
        children.sort_by(|a, b| a.slices.z_index().total_cmp(&b.slices.z_index()));
    }
    let mut graphics_layer = node.node_data.modifier_slices.graphics_layer();
    let has_graphics_layer = graphics_layer.is_some();
    shape_clip(
        &mut graphics_layer,
        clip_shape(&node.node_data.modifier_slices),
        Rect {
            x: 0.0,
            y: 0.0,
            width: node.rect.width,
            height: node.rect.height,
        },
    );
    let has_graphics_layer = has_graphics_layer
        || graphics_layer
            .as_ref()
            .is_some_and(|layer| layer.render_effect.is_some());

    BuildNodeSnapshot {
        node_id: node.node_id,
        placement,
        size: Size {
            width: node.rect.width,
            height: node.rect.height,
        },
        content_offset: node.content_offset,
        slices: Rc::clone(&node.node_data.modifier_slices),
        graphics_layer: graphics_layer
            .filter(|_| has_graphics_layer)
            .map(Cow::into_owned),
        children,
    }
}

pub(super) fn modifier_slices_have_origin_sinks(slices: &ModifierNodeSlices) -> bool {
    slices.text_window().is_some() || slices.viewport_window_rect().is_some()
}

fn layer_cache_policy(layer: &GraphicsLayer, isolation: IsolationReasons) -> CachePolicy {
    if isolation.has_any() || layer_scales_or_rotates(layer) {
        CachePolicy::Auto
    } else {
        CachePolicy::None
    }
}

pub(super) fn isolation_reasons(layer: &GraphicsLayer) -> IsolationReasons {
    IsolationReasons {
        explicit_offscreen: layer.compositing_strategy == CompositingStrategy::Offscreen,
        shape_clip: layer.clip && !matches!(layer.shape, LayerShape::Rectangle),
        effect: layer.render_effect.is_some(),
        backdrop: layer.backdrop_effect.is_some(),
        group_opacity: layer.compositing_strategy != CompositingStrategy::ModulateAlpha
            && layer.alpha < 1.0,
        blend_mode: layer.blend_mode != cranpose_ui::BlendMode::SrcOver,
    }
}
