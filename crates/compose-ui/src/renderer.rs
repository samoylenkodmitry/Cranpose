use crate::layout::{LayoutBox, LayoutNodeData, LayoutTree};
use crate::modifier::{DrawCommand as ModifierDrawCommand, Rect, Size};
use compose_core::NodeId;
use compose_ui_graphics::DrawPrimitive;

/// Layer that a paint operation targets within the rendering pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintLayer {
    Behind,
    Content,
    Overlay,
}

/// A rendered operation emitted by the headless renderer stub.
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

    /// Consumes the scene and yields the owned operations.
    pub fn into_operations(self) -> Vec<RenderOp> {
        self.operations
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

    #[allow(clippy::only_used_in_recursion)]
    fn render_box(&self, layout: &LayoutBox, operations: &mut Vec<RenderOp>) {
        let rect = layout.rect;
        let (mut behind, mut overlay) = evaluate_modifier(layout.node_id, &layout.node_data, rect);

        operations.append(&mut behind);

        // Render text content if present in modifier slices.
        // This follows Jetpack Compose's pattern where text is a modifier node capability
        // (TextModifierNode implements LayoutModifierNode + DrawModifierNode + SemanticsNode)
        if let Some(text) = layout.node_data.modifier_slices().text_content() {
            operations.push(RenderOp::Text {
                node_id: layout.node_id,
                rect,
                value: text.to_string(),
            });
        }

        // Render children
        for child in &layout.children {
            self.render_box(child, operations);
        }

        operations.append(&mut overlay);
    }
}

fn evaluate_modifier(
    node_id: NodeId,
    data: &LayoutNodeData,
    rect: Rect,
) -> (Vec<RenderOp>, Vec<RenderOp>) {
    let mut behind = Vec::new();
    let mut overlay = Vec::new();

    let size = Size {
        width: rect.width,
        height: rect.height,
    };

    // Render via modifier slices - all drawing now goes through draw commands
    // collected from the modifier node chain, including backgrounds, borders, etc.
    for command in data.modifier_slices().draw_commands() {
        match command {
            ModifierDrawCommand::Behind(func) => {
                for primitive in func(size) {
                    behind.push(RenderOp::Primitive {
                        node_id,
                        layer: PaintLayer::Behind,
                        primitive: translate_primitive(primitive, rect.x, rect.y),
                    });
                }
            }
            ModifierDrawCommand::Overlay(func) => {
                for primitive in func(size) {
                    overlay.push(RenderOp::Primitive {
                        node_id,
                        layer: PaintLayer::Overlay,
                        primitive: translate_primitive(primitive, rect.x, rect.y),
                    });
                }
            }
        }
    }

    (behind, overlay)
}

fn translate_primitive(primitive: DrawPrimitive, dx: f32, dy: f32) -> DrawPrimitive {
    match primitive {
        DrawPrimitive::Rect { rect, brush } => DrawPrimitive::Rect {
            rect: rect.translate(dx, dy),
            brush,
        },
        DrawPrimitive::RoundRect { rect, brush, radii } => DrawPrimitive::RoundRect {
            rect: rect.translate(dx, dy),
            brush,
            radii,
        },
    }
}

#[cfg(test)]
#[path = "tests/renderer_tests.rs"]
mod tests;
