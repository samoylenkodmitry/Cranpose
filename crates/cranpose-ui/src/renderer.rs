use cranpose_core::{MemoryApplier, NodeId};
use cranpose_ui_graphics::DrawPrimitive;

use crate::{
    layout::{LayoutBox, LayoutNodeData, LayoutTree},
    modifier::{DrawCommand as ModifierDrawCommand, Point, Rect, Size},
    widgets::LayoutNode,
};

/// Layer that a paint operation targets within the rendering pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintLayer {
    Behind,
    Content,
    Overlay,
}

/// A rendered operation emitted by the headless renderer.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderOp {
    Primitive {
        node_id: NodeId,
        layer: PaintLayer,
        primitive: DrawPrimitive,
    },
    Text {
        node_id: NodeId,
        rect: Rect,
        value: String,
    },
}

/// A collection of render operations for a composed scene.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecordedRenderScene {
    operations: Vec<RenderOp>,
}

impl RecordedRenderScene {
    pub fn new(operations: Vec<RenderOp>) -> Self {
        Self { operations }
    }

    /// Returns a slice of recorded render operations in submission order.
    pub fn operations(&self) -> &[RenderOp] {
        &self.operations
    }

    /// Returns an iterator over primitives that target the provided paint layer.
    pub fn primitives_for(&self, layer: PaintLayer) -> impl Iterator<Item = &DrawPrimitive> {
        self.operations.iter().filter_map(move |op| match op {
            RenderOp::Primitive {
                layer: op_layer,
                primitive,
                ..
            } if *op_layer == layer => Some(primitive),
            _ => None,
        })
    }
}

/// A lightweight renderer that walks the layout tree and materialises paint commands.
#[derive(Default)]
pub struct HeadlessRenderer;

impl HeadlessRenderer {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, tree: &LayoutTree) -> RecordedRenderScene {
        let mut operations = Vec::new();
        self.render_box(tree.root(), &mut operations);
        RecordedRenderScene::new(operations)
    }

    fn render_box(&self, layout: &LayoutBox, operations: &mut Vec<RenderOp>) {
        let rect = layout.rect;
        let (mut behind, overlay) = evaluate_modifier(layout.node_id, &layout.node_data, rect);

        operations.append(&mut behind);

        if let Some(text) = layout.node_data.modifier_slices().text_content() {
            operations.push(RenderOp::Text {
                node_id: layout.node_id,
                rect,
                value: text.to_string(),
            });
        }

        for child in &layout.children {
            self.render_box(child, operations);
        }

        append_overlay(operations, layout.node_id, rect, overlay);
    }
}

enum PendingOverlay<'a> {
    Recorded(std::vec::IntoIter<DrawPrimitive>),
    Command(&'a crate::draw::DrawCommandFn),
}

fn evaluate_modifier(
    node_id: NodeId,
    data: &LayoutNodeData,
    rect: Rect,
) -> (Vec<RenderOp>, Vec<PendingOverlay<'_>>) {
    let size = Size {
        width: rect.width,
        height: rect.height,
    };

    collect_primitives_from_commands(node_id, rect, size, data.modifier_slices().draw_commands())
}

fn collect_primitives_from_commands(
    node_id: NodeId,
    rect: Rect,
    size: Size,
    commands: &[ModifierDrawCommand],
) -> (Vec<RenderOp>, Vec<PendingOverlay<'_>>) {
    let mut behind = Vec::new();
    let mut overlay = Vec::new();
    for command in commands {
        match command {
            ModifierDrawCommand::Behind(func) => {
                for primitive in record(func, size) {
                    append_primitive(&mut behind, node_id, rect, PaintLayer::Behind, primitive);
                }
            }
            ModifierDrawCommand::Overlay(func) => {
                overlay.push(PendingOverlay::Command(func));
            }
            ModifierDrawCommand::WithContent(func) => {
                let primitives = record(func, size);
                let last_content = primitives
                    .iter()
                    .rposition(|primitive| matches!(primitive, DrawPrimitive::Content));
                let mut primitives = primitives.into_iter();
                if let Some(last_content) = last_content {
                    for primitive in primitives.by_ref().take(last_content) {
                        append_primitive(&mut behind, node_id, rect, PaintLayer::Behind, primitive);
                    }
                    let _ = primitives.next();
                }
                if !primitives.as_slice().is_empty() {
                    overlay.push(PendingOverlay::Recorded(primitives));
                }
            }
        }
    }
    (behind, overlay)
}

fn record(func: &crate::draw::DrawCommandFn, size: Size) -> Vec<DrawPrimitive> {
    use cranpose_ui_graphics::DrawScope as _;
    let mut scope = crate::draw::command_draw_scope(size);
    func(&mut scope);
    scope.into_primitives()
}

fn append_overlay(
    operations: &mut Vec<RenderOp>,
    node_id: NodeId,
    rect: Rect,
    pending: Vec<PendingOverlay<'_>>,
) {
    let size = Size {
        width: rect.width,
        height: rect.height,
    };
    for part in pending {
        let primitives = match part {
            PendingOverlay::Recorded(primitives) => primitives,
            PendingOverlay::Command(func) => record(func, size).into_iter(),
        };
        for primitive in primitives {
            append_primitive(operations, node_id, rect, PaintLayer::Overlay, primitive);
        }
    }
}

fn append_primitive(
    operations: &mut Vec<RenderOp>,
    node_id: NodeId,
    rect: Rect,
    layer: PaintLayer,
    primitive: DrawPrimitive,
) {
    if !matches!(primitive, DrawPrimitive::Content) {
        operations.push(RenderOp::Primitive {
            node_id,
            layer,
            primitive: primitive.translate(rect.x, rect.y),
        });
    }
}

impl HeadlessRenderer {
    /// Renders the scene by traversing LayoutNodes directly via the Applier.
    /// This is the new architecture that eliminates per-frame LayoutTree reconstruction.
    pub fn render_from_applier(
        &self,
        applier: &mut MemoryApplier,
        root: NodeId,
    ) -> RecordedRenderScene {
        let mut operations = Vec::new();
        let mut child_stack = Vec::new();
        self.render_node_from_applier(
            applier,
            root,
            Point::default(),
            &mut operations,
            &mut child_stack,
        );
        RecordedRenderScene::new(operations)
    }

    /// Records `node_id` and its subtree. `child_stack` is shared by the whole
    /// walk: a node pushes its children, visits them while its descendants
    /// push and pop above them, and pops them, so no child list is copied out
    /// of the applier.
    fn render_node_from_applier(
        &self,
        applier: &mut MemoryApplier,
        node_id: NodeId,
        parent_offset: Point,
        operations: &mut Vec<RenderOp>,
        child_stack: &mut Vec<NodeId>,
    ) {
        let first_child = child_stack.len();
        let Ok(Some((layout_state, modifier_slices))) =
            applier.with_node::<LayoutNode, _>(node_id, |node| {
                let state = node.layout_state();
                state.is_placed().then(|| {
                    child_stack.extend_from_slice(&node.children);
                    (state, node.modifier_slices_snapshot())
                })
            })
        else {
            return;
        };

        let abs_x = parent_offset.x + layout_state.position().x;
        let abs_y = parent_offset.y + layout_state.position().y;

        let rect = Rect {
            x: abs_x,
            y: abs_y,
            width: layout_state.size().width,
            height: layout_state.size().height,
        };

        let size = Size {
            width: rect.width,
            height: rect.height,
        };

        let (behind, overlay) =
            collect_primitives_from_commands(node_id, rect, size, modifier_slices.draw_commands());
        operations.extend(behind);

        if let Some(text) = modifier_slices.text_content() {
            operations.push(RenderOp::Text {
                node_id,
                rect,
                value: text.to_string(),
            });
        }

        let child_offset = Point {
            x: abs_x + layout_state.content_offset().x,
            y: abs_y + layout_state.content_offset().y,
        };

        for index in first_child..child_stack.len() {
            let child_id = child_stack[index];
            self.render_node_from_applier(applier, child_id, child_offset, operations, child_stack);
        }
        child_stack.truncate(first_child);

        append_overlay(operations, node_id, rect, overlay);
    }
}

#[cfg(test)]
#[path = "tests/renderer_tests.rs"]
mod tests;
