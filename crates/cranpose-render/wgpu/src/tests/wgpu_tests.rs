use std::cell::Cell;

use cranpose_render_common::graph::RenderNode;
use cranpose_ui_graphics::GraphicsLayer;

use super::*;
use crate::pipeline::TextLayoutResolver;

static TEST_FONT: &[u8] =
    cranpose_render_common::software_text_raster::DEFAULT_SOFTWARE_TEXT_FONT_BYTES;

#[path = "inspector_overlay.rs"]
mod inspector_overlay;

#[test]
fn dev_overlay_is_recorded_outside_app_graph() {
    let mut renderer = WgpuRenderer::new(&[]);
    renderer.draw_dev_overlay(
        "240 FPS | avg 4.0ms | p95 4.5ms",
        Size {
            width: 800.0,
            height: 600.0,
        },
    );

    assert!(
        renderer
            .frontend
            .scene
            .graph
            .as_ref()
            .is_none_or(|graph| graph.root.children.iter().all(|child| {
                !matches!(
                    child,
                    RenderNode::Layer(layer) if layer.node_id == Some(NodeId::MAX)
                )
            })),
        "dev overlay must not be mixed into the app scene graph"
    );

    let graph = renderer
        .frontend
        .dev_overlay_graph
        .as_ref()
        .expect("overlay graph");
    let Some(RenderNode::Layer(overlay)) = graph.root.children.last() else {
        panic!("dev overlay should be the final top-level layer");
    };

    assert_eq!(overlay.node_id, Some(NodeId::MAX));
    assert_eq!(
        overlay.graphics_layer.compositing_strategy,
        GraphicsLayer::default().compositing_strategy,
        "dev overlay should not allocate an offscreen surface"
    );
}

struct CountingTextMeasurer {
    inner: SoftwareTextMeasurer,
    layout_calls: Rc<Cell<usize>>,
}

impl CountingTextMeasurer {
    fn new(layout_calls: Rc<Cell<usize>>) -> Self {
        Self {
            inner: SoftwareTextMeasurer::from_fonts_or_default(&[TEST_FONT], 16),
            layout_calls,
        }
    }
}

impl TextMeasurer for CountingTextMeasurer {
    fn measure(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
    ) -> cranpose_ui::TextMetrics {
        self.inner.measure(text, style)
    }

    fn get_offset_for_position(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        self.inner.get_offset_for_position(text, style, x, y)
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
        offset: usize,
    ) -> f32 {
        self.inner.get_cursor_x_for_offset(text, style, offset)
    }

    fn layout(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
    ) -> cranpose_ui::text_layout_result::TextLayoutResult {
        self.layout_calls.set(self.layout_calls.get() + 1);
        self.inner.layout(text, style)
    }
}

#[test]
fn headless_text_measurer_uses_software_text_font() {
    let measurer = headless_text_measurer_with_fonts(&[TEST_FONT]);
    let text = cranpose_ui::text::AnnotatedString::from("software text measurement");
    let style = cranpose_ui::text::TextStyle::default();

    let metrics = measurer.measure(&text, &style);
    let layout = measurer.layout(&text, &style);

    assert!(metrics.width > 0.0);
    assert!(metrics.height > 0.0);
    assert_eq!(layout.lines.len(), metrics.line_count);
}

#[test]
fn renderer_measurement_uses_software_text_service_without_render_cache_side_effect() {
    let mut renderer = WgpuRenderer::new(&[TEST_FONT]);
    let app_context = cranpose_ui::AppContext::new();
    renderer.attach_app_context_services(&app_context);

    let metrics = app_context.enter(|| {
        let text = cranpose_ui::text::AnnotatedString::from("phase local text cache");
        let style = cranpose_ui::text::TextStyle {
            span_style: cranpose_ui::text::SpanStyle {
                font_size: cranpose_ui::text::TextUnit::Sp(14.0),
                ..Default::default()
            },
            paragraph_style: cranpose_ui::text::ParagraphStyle {
                platform_style: Some(cranpose_ui::text::PlatformParagraphStyle {
                    include_font_padding: None,
                    shaping: Some(cranpose_ui::text::TextShaping::Basic),
                }),
                ..Default::default()
            },
        };
        cranpose_ui::text::measure_text(&text, &style)
    });

    assert!(
        metrics.width > 0.0,
        "software text service should measure text"
    );
    assert_eq!(
        renderer.frontend.text_state.text_cache_len(),
        0,
        "WGPU must not keep a renderer-side shaping cache for measurement"
    );
}

#[test]
fn renderer_attached_text_service_measures_long_multiline_text_with_software_line_height() {
    let mut renderer = WgpuRenderer::new(&[TEST_FONT]);
    let app_context = cranpose_ui::AppContext::new();
    renderer.attach_app_context_services(&app_context);

    let prepared = app_context.enter(|| {
        let text = cranpose_ui::text::AnnotatedString::from(
            (0..48)
                .map(|line| format!("// markdown code line {line:02}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let style = cranpose_ui::text::TextStyle::default();
        cranpose_ui::text::prepare_text_layout(
            &text,
            &style,
            cranpose_ui::text::TextLayoutOptions::default(),
            Some(952.0),
        )
    });

    assert_eq!(prepared.metrics.line_count, 48);
    assert!(
        prepared.metrics.line_height > 18.0,
        "renderer-attached text service must not use fallback monospaced line height: {:?}",
        prepared.metrics
    );
    assert!(
        prepared.metrics.height > 900.0,
        "48 software-measured lines should not collapse to a viewport-sized block: {:?}",
        prepared.metrics
    );
}

#[test]
fn render_text_layout_routes_through_attached_app_context_service() {
    let mut renderer = WgpuRenderer::new(&[TEST_FONT]);
    let app_context = cranpose_ui::AppContext::new();
    renderer.attach_app_context_services(&app_context);
    let layout_calls = Rc::new(Cell::new(0));
    app_context.set_text_measurer(CountingTextMeasurer::new(Rc::clone(&layout_calls)));

    app_context.enter(|| {
        let text = cranpose_ui::text::AnnotatedString::from("render text");
        let style = cranpose_ui::text::TextStyle::default();
        let layout = renderer.frontend.text_state.layout_text(&text, &style);
        assert!(layout.width > 0.0);
    });

    assert_eq!(layout_calls.get(), 1);
}
