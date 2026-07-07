//! Android-style cursor/selection magnifier ("loupe") shown while dragging a
//! text-field selection handle.
//!
//! When a finger drags the caret (or a selection endpoint) it sits right on top
//! of the glyphs it is trying to place, hiding them. Android floats a small
//! magnified slice of the text just above the finger so the exact caret
//! position stays visible. [`CursorMagnifier`] renders that loupe in the
//! top-level overlay via [`Popup`]: a rounded frame containing the caret's line
//! of text scaled up and translated so the caret sits at the loupe's center,
//! with a caret indicator drawn through it.
//!
//! It is touch-only by construction — the caller (`BasicTextField`) only emits
//! it while a finger is actively dragging a handle, and the field's handles are
//! themselves gated on touch input.
//!
//! ## What is simplified
//!
//! The loupe magnifies the caret's own text line (not wrapped neighbours) and
//! applies a pure scale+translate through a `graphics_layer`; it does not
//! re-render the surrounding field background or selection highlight inside the
//! lens. That is enough to keep the caret glyphs visible under the finger; a
//! fuller "screenshot the field region" lens is left for later.

#![allow(non_snake_case)]

use crate::composable;
use crate::modifier::Modifier;
use crate::text::{measure_text, AnnotatedString, TextStyle};
use crate::text_field_modifier_node::TextFieldHandleMetrics;
use crate::widgets::box_widget::{Box, BoxSpec};
use crate::widgets::popup::Popup;
use crate::widgets::Text;
use cranpose_ui_graphics::{Color, GraphicsLayer, Point, Rect, Size, TransformOrigin};

/// Magnification factor for the loupe (Android uses ~1.25–1.5×).
const MAGNIFICATION: f32 = 1.4;
/// Loupe frame size in logical px.
const LOUPE_WIDTH: f32 = 150.0;
const LOUPE_HEIGHT: f32 = 46.0;
/// Distance (px) between the finger/drag point and the bottom of the loupe, so
/// the finger never covers the magnified content.
const GAP_ABOVE_FINGER: f32 = 44.0;
/// 1px border ring thickness.
const BORDER_PX: f32 = 1.0;

const FRAME_FILL: Color = Color(1.0, 1.0, 1.0, 1.0);
const FRAME_BORDER: Color = Color(0.62, 0.64, 0.68, 1.0);
/// Caret indicator color (Android accent blue), matching the field handles.
const CARET_COLOR: Color = Color(0.26, 0.52, 0.96, 1.0);

/// The text of the line containing byte `offset`, and the caret's x within that
/// line (px, content space) measured with `style`.
fn caret_line(text: &str, style: &TextStyle, offset: usize) -> (String, f32) {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let after = &text[offset..];
    let line_end = offset + after.find('\n').unwrap_or(after.len());
    let line = text[line_start..line_end].to_string();
    let caret_x = measure_text(&AnnotatedString::from(&text[line_start..offset]), style).width;
    (line, caret_x)
}

/// Floating magnifier loupe centered on the caret at `caret_offset`, positioned
/// just above `finger` (window coordinates of the active drag point).
///
/// * `text` / `style` — the field's current text and text style.
/// * `metrics` — live field handle metrics (used for the line height).
/// * `caret_offset` — byte offset the loupe is magnifying (the dragged caret).
/// * `finger` — window position of the finger, so the loupe floats above it.
#[composable]
pub fn CursorMagnifier(
    text: String,
    style: TextStyle,
    metrics: TextFieldHandleMetrics,
    caret_offset: usize,
    finger: Point,
) {
    let (line, caret_x) = caret_line(&text, &style, caret_offset);
    let line_height = metrics.line_height.max(1.0);

    // Float the loupe above the finger, clamped on-screen at the top edge.
    let anchor = Rect {
        x: (finger.x - LOUPE_WIDTH / 2.0).max(0.0),
        y: (finger.y - GAP_ABOVE_FINGER - LOUPE_HEIGHT).max(0.0),
        width: 0.0,
        height: 0.0,
    };

    // Scale the caret's line about its top-left, then translate so the caret's
    // content-x lands at the loupe's horizontal center and the line is centered
    // vertically. drawn_x = MAGNIFICATION * local_x + translation_x.
    let magnified_line_height = line_height * MAGNIFICATION;
    let translation_x = LOUPE_WIDTH / 2.0 - MAGNIFICATION * caret_x;
    let translation_y = (LOUPE_HEIGHT - magnified_line_height) / 2.0;
    let text_layer = GraphicsLayer {
        transform_origin: TransformOrigin::new(0.0, 0.0),
        scale: 1.0,
        scale_x: MAGNIFICATION,
        scale_y: MAGNIFICATION,
        translation_x,
        translation_y,
        ..GraphicsLayer::default()
    };

    // Caret indicator: a thin vertical bar at the loupe center spanning the
    // magnified line height.
    let caret_top = (LOUPE_HEIGHT - magnified_line_height) / 2.0;

    Popup(anchor, Point { x: 0.0, y: 0.0 }, move || {
        let line = line.clone();
        let style = style.clone();
        let text_layer = text_layer.clone();
        // Border ring: an outer filled+rounded box, inset by BORDER_PX to the
        // fill (cranpose has no dedicated border modifier yet).
        Box(
            Modifier::empty()
                .size(Size {
                    width: LOUPE_WIDTH,
                    height: LOUPE_HEIGHT,
                })
                .background(FRAME_BORDER)
                .rounded_corners(10.0),
            BoxSpec::default(),
            move || {
                let line = line.clone();
                let style = style.clone();
                let text_layer = text_layer.clone();
                Box(
                    Modifier::empty()
                        .padding(BORDER_PX)
                        .size(Size {
                            width: LOUPE_WIDTH - 2.0 * BORDER_PX,
                            height: LOUPE_HEIGHT - 2.0 * BORDER_PX,
                        })
                        .background(FRAME_FILL)
                        .rounded_corners(9.0)
                        .clip_to_bounds(),
                    BoxSpec::default(),
                    move || {
                        // Magnified slice of the caret's text line.
                        Text(
                            line.clone(),
                            Modifier::empty().graphics_layer_value(text_layer.clone()),
                            style.clone(),
                        );
                        // Caret indicator through the loupe center.
                        Box(
                            Modifier::empty()
                                .absolute_offset(LOUPE_WIDTH / 2.0 - 1.0, caret_top)
                                .size(Size {
                                    width: 2.0,
                                    height: magnified_line_height,
                                })
                                .background(CARET_COLOR),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::LayoutEngine;
    use crate::renderer::{HeadlessRenderer, RecordedRenderScene, RenderOp};
    use crate::widgets::PopupHost;
    use cranpose_core::{location_key, Composition, MemoryApplier};

    fn render_magnifier(finger: Point, caret_offset: usize) -> RecordedRenderScene {
        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());
        let metrics = TextFieldHandleMetrics {
            focused: true,
            touch: true,
            node_origin: Point { x: 0.0, y: 40.0 },
            padding_left: 0.0,
            padding_top: 0.0,
            scroll_offset: 0.0,
            line_height: 18.0,
        };
        let mut content = move || {
            PopupHost(move || {
                CursorMagnifier(
                    "hello world".to_string(),
                    TextStyle::default(),
                    metrics,
                    caret_offset,
                    finger,
                );
            });
        };
        composition.render(key, &mut content).expect("render");
        for _ in 0..16 {
            if !composition.should_render() {
                break;
            }
            composition.reconcile(key, &mut content).expect("reconcile");
        }
        let root = composition.root().expect("root");
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let layout = applier
            .compute_layout(
                root,
                Size {
                    width: 500.0,
                    height: 500.0,
                },
            )
            .expect("layout");
        applier.clear_runtime_handle();
        drop(applier);
        HeadlessRenderer::new().render(&layout)
    }

    fn text_ops(scene: &RecordedRenderScene) -> Vec<(String, Rect)> {
        scene
            .operations()
            .iter()
            .filter_map(|op| match op {
                RenderOp::Text { value, rect, .. } => Some((value.clone(), *rect)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn magnifier_shows_the_caret_line_floated_above_the_finger() {
        let _app_context = crate::render_state::app_context_test_scope();
        let finger = Point { x: 120.0, y: 300.0 };
        let scene = render_magnifier(finger, 3);

        let texts = text_ops(&scene);
        // The loupe magnifies the caret's whole line ("hello world" has no
        // newline, so the line is the full text).
        let line = texts
            .iter()
            .find(|(value, _)| value == "hello world")
            .expect("magnifier renders the caret's line of text");
        // The loupe floats ABOVE the finger so it never covers the caret.
        assert!(
            line.1.y < finger.y,
            "the magnified text {} must render above the finger at y={}",
            line.1.y,
            finger.y
        );
    }

    #[test]
    fn magnifier_frame_is_drawn() {
        use cranpose_ui_graphics::DrawPrimitive;
        let _app_context = crate::render_state::app_context_test_scope();
        let scene = render_magnifier(Point { x: 120.0, y: 300.0 }, 3);
        // The loupe draws its rounded frame fill and a caret indicator as rects.
        let rects = scene
            .operations()
            .iter()
            .filter(|op| {
                matches!(
                    op,
                    RenderOp::Primitive {
                        primitive: DrawPrimitive::Rect { .. } | DrawPrimitive::RoundRect { .. },
                        ..
                    }
                )
            })
            .count();
        assert!(
            rects >= 2,
            "the loupe should draw a frame and a caret indicator, got {rects} rects"
        );
    }
}
