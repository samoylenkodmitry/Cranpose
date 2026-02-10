//! Shaders demo tab: sweep gradient and interactive backdrop effects showcase.

#![allow(non_snake_case)]

use cranpose_foundation::PointerButton;
use cranpose_ui::{
    composable, Alignment, Box, BoxSpec, Brush, Color, Column, ColumnSpec, ContentScale,
    CornerRadii, GraphicsLayer, Image, ImageBitmap, LinearArrangement, Modifier, Point,
    PointerEventKind, PointerInputScope, Text, TextStyle,
};
use cranpose_ui_graphics::{
    liquid_glass_effect, LiquidGlassRect, LiquidGlassSpec, RenderEffect, RuntimeShader,
};

use super::images::generate_chessboard_bitmap;

// ── Rect-masked single-pass blur shader ─────────────────────────────────────
//
// Blurs only within a rounded rectangle region; passes through original pixels
// outside. Uses a golden-angle spiral sampling pattern (28 taps) for a smooth
// disc-shaped kernel that looks good at any radius.
//
// Uniform layout (float indices):
//   0,1: container size (w,h) px
//   2,3: rect center (cx, cy) px
//   4,5: rect size (w,h) px
//   6: corner radius px
//   7: blur radius px

const RECT_BLUR_WGSL: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn fullscreen_vs(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;
    let x = f32(i32(vertex_index & 1u) * 2 - 1);
    let y = f32(i32(vertex_index >> 1u) * 2 - 1);
    output.uv = vec2<f32>(x * 0.5 + 0.5, 1.0 - (y * 0.5 + 0.5));
    output.position = vec4<f32>(x, y, 0.0, 1.0);
    return output;
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> u: array<vec4<f32>, 64>;

fn get_float(index: u32) -> f32 { return u[index / 4u][index % 4u]; }
fn get_vec2(index: u32) -> vec2<f32> { return vec2<f32>(get_float(index), get_float(index + 1u)); }

fn sd_round_rect(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = input.uv;
    let tex_size = vec2<f32>(textureDimensions(input_texture));

    // Effect layer pixel rect injected by the renderer at uniform slot 62
    let effect_rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let container_dp = get_vec2(0u);

    // dp → pixel scale: effect area pixel size / container dp size
    let dp_scale = effect_rect.zw / max(container_dp, vec2<f32>(1.0));
    let s = min(dp_scale.x, dp_scale.y);

    // Fragment position in effect-local pixel coordinates
    let coord = uv * tex_size - effect_rect.xy;
    let center = get_vec2(2u) * dp_scale;
    let rect_size = get_vec2(4u) * dp_scale;
    let corner_radius = get_float(6u) * s;
    let blur_radius = get_float(7u) * s;

    let half_size = rect_size * 0.5;
    let p = coord - center;
    let d = sd_round_rect(p, half_size, corner_radius);

    let original = textureSample(input_texture, input_sampler, uv);

    // Outside rect with margin: pass through
    if d > blur_radius {
        return original;
    }

    // Blur using golden-angle spiral sampling (28 taps + center)
    let sigma = blur_radius * 0.45;
    let inv_2sigma2 = 1.0 / (2.0 * sigma * sigma);
    var color = original;
    var total = 1.0;

    for (var i = 1u; i < 29u; i++) {
        let fi = f32(i);
        let angle = fi * 2.399963;
        let r = sqrt(fi / 29.0) * blur_radius;
        let offset = vec2<f32>(cos(angle), sin(angle)) * r;
        let w = exp(-r * r * inv_2sigma2);
        let sample_uv = clamp(uv + offset / tex_size, vec2<f32>(0.0), vec2<f32>(1.0));
        color = color + textureSample(input_texture, input_sampler, sample_uv) * w;
        total = total + w;
    }
    let blurred = color / total;

    // Smooth transition at rect edge
    let alpha = smoothstep(2.0, -2.0, d);
    return mix(original, blurred, alpha);
}
"#;

/// Build a rect-masked blur RenderEffect in local layer coordinates.
fn rect_blur_effect(width: f32, height: f32, corner_radius: f32, blur_radius: f32) -> RenderEffect {
    let mut shader = RuntimeShader::new(RECT_BLUR_WGSL);
    // Backdrop effects are rendered in effect-local coordinates. Keep uniforms
    // local to the composable bounds so movement is driven only by layer translation.
    shader.set_float2(0, width, height);
    shader.set_float2(2, width * 0.5, height * 0.5);
    shader.set_float2(4, width, height);
    shader.set_float(6, corner_radius);
    shader.set_float(7, blur_radius);
    RenderEffect::runtime_shader(shader)
}

/// Build a local liquid-glass effect sized to the composable bounds.
fn liquid_glass_local_effect(width: f32, height: f32, corner: f32) -> RenderEffect {
    let glass_rect = LiquidGlassRect {
        left: 0.0,
        top: 0.0,
        width,
        height,
        tint_color: Color(0.6, 0.8, 1.0, 0.12),
    };
    liquid_glass_effect(
        &glass_rect,
        &LiquidGlassSpec {
            corner_radius: corner,
            // Tilt simulates viewing angle for refraction on desktop
            tilt_angle: 0.5,
            tilt_pitch: 0.3,
            ..LiquidGlassSpec::default()
        },
        width,
        height,
    )
}

fn clamped_drag_position(
    global_position: Point,
    drag_offset: (f32, f32),
    area_w: f32,
    area_h: f32,
    width: f32,
    height: f32,
) -> Point {
    Point {
        x: (global_position.x - drag_offset.0).clamp(0.0, area_w - width),
        y: (global_position.y - drag_offset.1).clamp(0.0, area_h - height),
    }
}

fn apply_drag_position(pos: &cranpose_core::MutableState<Point>, next: Point) {
    pos.set(next);
    // Lazy graphics-layer reads happen at render time, so dragging must request redraw.
    cranpose_ui::request_render_invalidation();
}

// ── Composables ─────────────────────────────────────────────────────────────

/// Main shaders demo tab composable.
#[composable]
pub(crate) fn ShadersTab() {
    Column(
        Modifier::empty()
            .padding(32.0)
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::solid(Color(0.10, 0.13, 0.20, 1.0)),
                    CornerRadii::uniform(24.0),
                );
            })
            .padding(20.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(24.0)),
        || {
            Text(
                "Shaders & Effects",
                Modifier::empty().padding(10.0).draw_behind(|scope| {
                    scope.draw_round_rect(
                        Brush::solid(Color(1.0, 1.0, 1.0, 0.08)),
                        CornerRadii::uniform(14.0),
                    );
                }),
                TextStyle::default(),
            );

            SweepGradientDemo();
            InteractiveEffectsDemo();
        },
    );
}

/// Demo: rainbow circle using SweepGradient brush.
#[composable]
fn SweepGradientDemo() {
    Column(
        Modifier::empty(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        || {
            Text(
                "Sweep Gradient",
                Modifier::empty().padding(8.0).draw_behind(|scope| {
                    scope.draw_round_rect(
                        Brush::solid(Color(0.14, 0.18, 0.30, 0.8)),
                        CornerRadii::uniform(12.0),
                    );
                }),
                TextStyle::default(),
            );

            let size = 120.0;
            let half = size / 2.0;

            Box(
                Modifier::empty()
                    .size_points(size, size)
                    .draw_behind(move |scope| {
                        scope.draw_round_rect(
                            Brush::sweep_gradient(
                                vec![
                                    Color(1.0, 0.0, 0.0, 1.0),
                                    Color(1.0, 1.0, 0.0, 1.0),
                                    Color(0.0, 1.0, 0.0, 1.0),
                                    Color(0.0, 1.0, 1.0, 1.0),
                                    Color(0.0, 0.0, 1.0, 1.0),
                                    Color(1.0, 0.0, 1.0, 1.0),
                                    Color(1.0, 0.0, 0.0, 1.0),
                                ],
                                Point { x: half, y: half },
                            ),
                            CornerRadii::uniform(half),
                        );
                    }),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

/// Interactive demo: checkerboard background with two draggable overlay rects.
/// One applies a rect-masked blur, the other applies LiquidGlass refraction.
/// The label text inside each rect is NOT affected by the effect — it floats
/// on top as a sibling outside the effect layer.
#[composable]
fn InteractiveEffectsDemo() {
    let area_w = 400.0;
    let area_h = 280.0;
    let rect_w = 140.0;
    let rect_h = 100.0;
    let corner = 20.0;

    let blur_pos = cranpose_core::useState(|| Point { x: 16.0, y: 16.0 });
    let glass_pos = cranpose_core::useState(|| Point { x: 244.0, y: 164.0 });

    Column(
        Modifier::empty(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                "Interactive Effects (drag the rects!)",
                Modifier::empty().padding(8.0).draw_behind(|scope| {
                    scope.draw_round_rect(
                        Brush::solid(Color(0.14, 0.18, 0.30, 0.8)),
                        CornerRadii::uniform(12.0),
                    );
                }),
                TextStyle::default(),
            );

            let blur_fx = rect_blur_effect(rect_w, rect_h, corner, 12.0);
            let glass_fx = liquid_glass_local_effect(rect_w, rect_h, corner);

            let checkerboard: ImageBitmap =
                cranpose_core::remember(|| generate_chessboard_bitmap(24, 17)).with(|b| b.clone());

            // Clipping parent — everything is positioned inside this box
            Box(
                Modifier::empty()
                    .size_points(area_w, area_h)
                    .rounded_corners(16.0),
                BoxSpec::default(),
                move || {
                    Box(
                        Modifier::empty().size_points(area_w, area_h),
                        BoxSpec::default(),
                        {
                            let board = checkerboard.clone();
                            move || {
                                Image(
                                    board.clone(),
                                    None,
                                    Modifier::empty().size_points(area_w, area_h),
                                    Alignment::CENTER,
                                    ContentScale::Crop,
                                    1.0,
                                    None,
                                );
                                // Colorful text rows to make effects visible
                                Column(
                                    Modifier::empty().absolute_offset(12.0, 12.0),
                                    ColumnSpec::new()
                                        .vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                                    || {
                                        for (text, color) in [
                                            ("Hello, World!", Color(1.0, 0.3, 0.3, 1.0)),
                                            ("Cranpose UI", Color(0.3, 1.0, 0.3, 1.0)),
                                            ("Shader Effects", Color(0.3, 0.5, 1.0, 1.0)),
                                            ("Drag me around!", Color(1.0, 0.9, 0.2, 1.0)),
                                        ] {
                                            Text(
                                                text,
                                                Modifier::empty().padding(4.0).draw_behind({
                                                    let c = color;
                                                    move |scope| {
                                                        scope.draw_round_rect(
                                                            Brush::solid(Color(
                                                                c.0 * 0.3,
                                                                c.1 * 0.3,
                                                                c.2 * 0.3,
                                                                0.8,
                                                            )),
                                                            CornerRadii::uniform(6.0),
                                                        );
                                                    }
                                                }),
                                                TextStyle {
                                                    color: Some(color),
                                                    ..Default::default()
                                                },
                                            );
                                        }
                                    },
                                );
                            }
                        },
                    );

                    // ── Draggable overlay rects ─────────────────────────
                    // These are SIBLINGS of the effect box, so their content
                    // (border + label) is rendered normally — not affected
                    // by the blur/glass effects.
                    DraggableOverlay(
                        blur_pos,
                        blur_fx.clone(),
                        rect_w,
                        rect_h,
                        corner,
                        "Blur",
                        Color(0.4, 0.7, 1.0, 0.5),
                        area_w,
                        area_h,
                    );
                    DraggableOverlay(
                        glass_pos,
                        glass_fx.clone(),
                        rect_w,
                        rect_h,
                        corner,
                        "Glass",
                        Color(0.6, 1.0, 0.7, 0.5),
                        area_w,
                        area_h,
                    );
                },
            );

            Text(
                "Drag the rects to see backdrop blur and glass refraction",
                Modifier::empty().padding(6.0).draw_behind(|scope| {
                    scope.draw_round_rect(
                        Brush::solid(Color(1.0, 1.0, 1.0, 0.05)),
                        CornerRadii::uniform(8.0),
                    );
                }),
                TextStyle::default(),
            );
        },
    );
}

/// A draggable overlay rect with a border and centered label.
/// Handles pointer drag to update `pos` state.
#[composable]
#[allow(clippy::too_many_arguments)]
fn DraggableOverlay(
    pos: cranpose_core::MutableState<Point>,
    backdrop_effect: RenderEffect,
    width: f32,
    height: f32,
    corner: f32,
    label: &'static str,
    border_color: Color,
    area_w: f32,
    area_h: f32,
) {
    Box(
        Modifier::empty()
            .size_points(width, height)
            .graphics_layer_lazy(move || GraphicsLayer {
                translation_x: pos.get().x,
                translation_y: pos.get().y,
                ..Default::default()
            })
            .backdrop_effect(backdrop_effect)
            .draw_behind(move |scope| {
                // Rounded border to show the rect boundary
                scope.draw_round_rect(Brush::solid(border_color), CornerRadii::uniform(corner));
            })
            .padding(2.0)
            .draw_behind(move |scope| {
                // Inner transparent fill (cut out the border)
                scope.draw_round_rect(
                    Brush::solid(Color(0.0, 0.0, 0.0, 0.0)),
                    CornerRadii::uniform(corner - 2.0),
                );
            })
            .pointer_input((), {
                move |scope: PointerInputScope| async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            let mut drag_offset: Option<(f32, f32)> = None;
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                match event.kind {
                                    PointerEventKind::Down => {
                                        let cur = pos.get();
                                        drag_offset = Some((
                                            event.global_position.x - cur.x,
                                            event.global_position.y - cur.y,
                                        ));
                                        event.consume();
                                    }
                                    PointerEventKind::Move => {
                                        if !event.buttons.contains(PointerButton::Primary) {
                                            drag_offset = None;
                                            continue;
                                        }
                                        if let Some((ox, oy)) = drag_offset {
                                            let next = clamped_drag_position(
                                                event.global_position,
                                                (ox, oy),
                                                area_w,
                                                area_h,
                                                width,
                                                height,
                                            );
                                            apply_drag_position(&pos, next);
                                        }
                                        event.consume();
                                    }
                                    PointerEventKind::Up | PointerEventKind::Cancel => {
                                        drag_offset = None;
                                    }
                                }
                            }
                        })
                        .await;
                }
            }),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            Text(
                label,
                Modifier::empty().padding(8.0).draw_behind(move |scope| {
                    scope.draw_round_rect(
                        Brush::solid(Color(0.0, 0.0, 0.0, 0.5)),
                        CornerRadii::uniform(8.0),
                    );
                }),
                TextStyle {
                    color: Some(Color(1.0, 1.0, 1.0, 0.9)),
                    ..Default::default()
                },
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn rect_blur_effect_uses_local_layer_space() {
        let effect = rect_blur_effect(140.0, 100.0, 20.0, 12.0);
        let RenderEffect::Shader { shader } = effect else {
            panic!("expected shader effect");
        };
        let u = shader.uniforms();

        assert_eq!(u[0], 140.0);
        assert_eq!(u[1], 100.0);
        assert_eq!(u[2], 70.0);
        assert_eq!(u[3], 50.0);
        assert_eq!(u[4], 140.0);
        assert_eq!(u[5], 100.0);
        assert_eq!(u[6], 20.0);
        assert_eq!(u[7], 12.0);
    }

    #[test]
    fn liquid_glass_local_effect_uses_local_layer_space() {
        let effect = liquid_glass_local_effect(140.0, 100.0, 20.0);
        let RenderEffect::Shader { shader } = effect else {
            panic!("expected shader effect");
        };
        let u = shader.uniforms();

        assert_eq!(u[0], 140.0);
        assert_eq!(u[1], 100.0);
        assert_eq!(u[2], 70.0);
        assert_eq!(u[3], 50.0);
        assert_eq!(u[4], 140.0);
        assert_eq!(u[5], 100.0);
        assert_eq!(u[6], 20.0);
    }

    #[test]
    fn clamped_drag_position_stays_within_bounds() {
        let next = clamped_drag_position(
            Point::new(999.0, -10.0),
            (10.0, 15.0),
            320.0,
            240.0,
            140.0,
            100.0,
        );
        assert_eq!(next, Point::new(180.0, 0.0));
    }

    #[test]
    fn apply_drag_position_updates_state_and_requests_render() {
        let runtime = cranpose_core::runtime::Runtime::new(Arc::new(
            cranpose_core::runtime::DefaultScheduler,
        ));
        let pos = cranpose_core::MutableState::with_runtime(Point::ZERO, runtime.handle());

        let _ = cranpose_ui::take_render_invalidation();
        apply_drag_position(&pos, Point::new(42.0, 24.0));

        assert_eq!(pos.get(), Point::new(42.0, 24.0));
        assert!(cranpose_ui::take_render_invalidation());
    }
}
