use cranpose_render_common::{
    graph::{LayerNode, ProjectiveTransform, RenderNode},
    text_fonts::TextFonts,
};
use cranpose_ui_graphics::{GraphicsLayer, Rect};

pub(crate) fn layer_node(
    local_bounds: Rect,
    transform_to_parent: ProjectiveTransform,
    graphics_layer: GraphicsLayer,
    children: Vec<RenderNode>,
) -> LayerNode {
    LayerNode {
        local_bounds,
        transform_to_parent,
        graphics_layer: graphics_layer.into(),
        children,
        ..Default::default()
    }
}

// Runs `block` in an app context whose text lays out with the font a
// renderer without app fonts draws with.
fn in_layout_context<R>(block: impl FnOnce() -> R) -> R {
    let context = cranpose_ui::AppContext::new();
    context.set_text_fonts(TextFonts::from_fonts_or_default(&[]).resolver());
    context.enter(block)
}

// `node` with its text laid out in its style as wide as its rect.
pub(crate) fn with_layout(
    mut node: cranpose_render_common::graph::TextPrimitiveNode,
) -> cranpose_render_common::graph::TextPrimitiveNode {
    node.layout = in_layout_context(|| {
        std::sync::Arc::clone(
            &cranpose_ui::text::prepare_text_layout(
                &node.text,
                &node.text_style,
                node.layout_options,
                Some(node.rect.width),
            )
            .layout,
        )
    });
    node
}

// `text` in the default style laid out on one line.
pub(crate) fn prepared_text(text: &str) -> std::rc::Rc<cranpose_ui::PreparedTextLayout> {
    in_layout_context(|| {
        cranpose_ui::text::prepare_text_layout(
            &cranpose_ui::text::AnnotatedString::from(text),
            &cranpose_ui::text::TextStyle::default(),
            cranpose_ui::TextLayoutOptions::default(),
            None,
        )
    })
}

// The layout a draw scope gives `text` in `style`.
pub(crate) fn draw_text_layout(
    text: &str,
    style: &cranpose_ui_graphics::DrawTextStyle,
) -> Option<cranpose_ui_graphics::TextLayoutHandle> {
    use cranpose_ui_graphics::DrawTextMeasurer as _;
    in_layout_context(|| cranpose_ui::text::AppContextTextMeasurer.layout_text(text, style))
}
