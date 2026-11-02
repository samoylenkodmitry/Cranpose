use cranpose_render_common::graph::{LayerNode, PrimitivePhase, ProjectiveTransform, RenderNode};
use cranpose_ui_graphics::{Point, Rect};

use super::{
    ContentStyle, PrimitiveRenderContext, RasterTraversalContext, Reuse, drawn_by_its_scene,
    flush_copies, note_passed_down, populate_draws_from_graph, render_draw_child,
    space::push_seen_surfaces,
};
use crate::{
    layout_walk::ChildSlot,
    raster_scene::{PassedDown, RasterScene, Subtree},
};

// What a layer's children push under: the walk above it, what it passes
// down to its child layers, the paint of its own draws, the surfaces of a
// scene it shows, and the slot of its node's layout children.
pub(super) struct LayerChildren<'a, 's, 'w> {
    pub(super) context: &'a RasterTraversalContext<'a>,
    pub(super) record: usize,
    pub(super) passed_down_before: Option<usize>,
    pub(super) transform: ProjectiveTransform,
    pub(super) visual_clip: Option<Rect>,
    pub(super) translated_snap_anchor: Option<Point>,
    pub(super) translated: bool,
    pub(super) content_style: &'a ContentStyle,
    pub(super) primitive_context: PrimitiveRenderContext<'a>,
    pub(super) seen: Option<Vec<(&'a LayerNode, ProjectiveTransform)>>,
    pub(super) in_space: bool,
    // Worked out at the first child layer: most layers have none.
    pub(super) same_as_before: Option<bool>,
    pub(super) own_slot: Option<&'s mut ChildSlot<'w>>,
    pub(super) passed_slot: Option<&'s mut ChildSlot<'w>>,
}

impl LayerChildren<'_, '_, '_> {
    // Pushes `child`, the next in draw order.
    pub(super) fn push(
        &mut self,
        child: Next<'_>,
        scene: &mut RasterScene,
        reuse: &mut Option<Reuse<'_>>,
    ) {
        let subtree = match child {
            Next::Node(node) => {
                let Some(subtree) = Subtree::of_child(node) else {
                    flush_copies(reuse, scene);
                    return render_draw_child(scene, node, self.primitive_context);
                };
                if let Subtree::Layer(child_layer) = subtree {
                    if let Some(surfaces) = self.seen.take_if(|_| child_layer.has_space()) {
                        flush_copies(reuse, scene);
                        push_seen_surfaces(
                            surfaces,
                            scene,
                            self.context,
                            (self.content_style, self.visual_clip),
                        );
                    }
                    if drawn_by_its_scene(child_layer, self.in_space) {
                        return;
                    }
                }
                Some(subtree)
            }
            Next::Children => None,
        };
        let same_as_before = match self.same_as_before {
            Some(same) => same,
            None => {
                let passed_down = PassedDown {
                    transform: self.transform,
                    alpha: self.content_style.alpha,
                    color_filter: self.content_style.color_filter.clone(),
                    clip: self.visual_clip,
                    snap_anchor: self.translated_snap_anchor,
                    translated: self.translated,
                };
                let same = note_passed_down(
                    scene,
                    self.record,
                    passed_down,
                    self.context,
                    self.passed_down_before,
                    reuse,
                );
                *self.same_as_before.insert(same)
            }
        };
        let child_context = RasterTraversalContext {
            parent_transform: &self.transform,
            parent_content_style: self.content_style,
            parent_visual_clip: self.visual_clip,
            inherited_translated_snap_anchor: self.translated_snap_anchor,
            inherited_translated_content_context: self.translated,
            diagnostics: self.context.diagnostics,
            light: self.context.light,
            same_as_before,
            text: self.context.text,
            surface_root: false,
            in_space: self.in_space,
        };
        match subtree {
            Some(subtree) => {
                let slot = self.passed_slot.take_if(|slot| {
                    matches!(subtree, Subtree::Layer(layer) if layer.node_id == Some(slot.owner()))
                });
                populate_draws_from_graph(subtree, scene, &child_context, reuse, slot);
            }
            None => {
                if let Some(slot) = self.own_slot.as_deref_mut() {
                    slot.push_children(scene, &child_context, reuse);
                }
            }
        }
    }
}

// What a layer pushes next in draw order: a node of its own child list, or
// the layout children of its node, which a walk of the layout tree visits
// where a lowered node's layer leaves them out.
#[derive(Clone, Copy)]
pub(super) enum Next<'a> {
    Node(&'a RenderNode),
    Children,
}

// Visits `children`, a lowered node's own list, in draw order around the
// node's layout children: its draws before them, the children, then the
// draws after them.
pub(super) fn for_each_around_children<'a>(
    children: &'a [RenderNode],
    mut visit: impl FnMut(Next<'a>),
) {
    let draws_after = |node: &RenderNode| node.draw_phase() == Some(PrimitivePhase::AfterChildren);
    let after = children
        .iter()
        .position(draws_after)
        .unwrap_or(children.len());
    debug_assert!(
        children[after..].iter().all(draws_after),
        "a lowered node's draws after its children follow the ones before"
    );
    children[..after]
        .iter()
        .for_each(|node| visit(Next::Node(node)));
    visit(Next::Children);
    children[after..]
        .iter()
        .for_each(|node| visit(Next::Node(node)));
}
