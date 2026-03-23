//! Shaders demo tab: sweep gradient and interactive backdrop effects showcase.

#![allow(non_snake_case)]

use cranpose_foundation::lazy::{remember_lazy_list_state, LazyListScope};
use cranpose_foundation::PointerButton;
use cranpose_ui::{
    composable, text::SpanStyle, Alignment, Box, BoxSpec, Brush, Color, Column, ColumnSpec,
    ContentScale, CornerRadii, GraphicsLayer, Image, ImageBitmap, LayerShape, LazyColumn,
    LazyColumnSpec, LinearArrangement, Modifier, Point, PointerEventKind, PointerInputScope,
    RoundedCornerShape, Text, TextStyle, TransformOrigin,
};
use cranpose_ui_graphics::{
    gradient_cut_mask_effect, liquid_glass_effect, rounded_alpha_mask_effect, BlendMode,
    CompositingStrategy, CutDirection, GradientCutMaskSpec, LiquidGlassRect, LiquidGlassSpec,
    RenderEffect, TileMode,
};

use super::images::generate_chessboard_bitmap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ShaderSection {
    SweepGradient,
    InteractiveEffects,
    EffectSemantics,
    GraphicsLayerFields,
    MaskApi,
}

impl ShaderSection {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::SweepGradient => "Sweep Gradient",
            Self::InteractiveEffects => "Interactive Effects",
            Self::EffectSemantics => "Effect Semantics",
            Self::GraphicsLayerFields => "GraphicsLayer Fields",
            Self::MaskApi => "Mask API",
        }
    }

    #[cfg(any(test, target_arch = "wasm32"))]
    pub(crate) fn from_startup_name(name: &str) -> Option<Self> {
        let normalized = name
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .map(|ch| ch.to_ascii_lowercase())
            .collect::<String>();
        match normalized.as_str() {
            "sweep" | "sweepgradient" => Some(Self::SweepGradient),
            "interactive" | "interactiveeffects" => Some(Self::InteractiveEffects),
            "effectsemantics" | "semantics" => Some(Self::EffectSemantics),
            "graphicslayer" | "graphicslayerfields" => Some(Self::GraphicsLayerFields),
            "mask" | "maskapi" => Some(Self::MaskApi),
            _ => None,
        }
    }
}

#[composable]
fn RenderShaderSection(section: ShaderSection) {
    match section {
        ShaderSection::SweepGradient => SweepGradientDemo(),
        ShaderSection::InteractiveEffects => InteractiveEffectsDemo(),
        ShaderSection::EffectSemantics => EffectSemanticsDemo(),
        ShaderSection::GraphicsLayerFields => GraphicsLayerFieldsDemo(),
        ShaderSection::MaskApi => MaskApiDemo(),
    }
}

/// Build a smooth rounded blur by chaining framework blur + rounded alpha mask.
fn smooth_rect_blur_effect(
    width: f32,
    height: f32,
    corner_radius: f32,
    blur_radius: f32,
) -> RenderEffect {
    let blur = RenderEffect::blur_xy(blur_radius, blur_radius, TileMode::Clamp);
    let mask =
        rounded_alpha_mask_effect(width, height, corner_radius, (blur_radius * 0.75).max(4.0));
    blur.then(mask)
}

#[cfg(test)]
fn half_cut_mask_effect(width: f32, height: f32) -> RenderEffect {
    gradient_cut_mask_effect(
        &GradientCutMaskSpec {
            progress: 0.5,
            feather: 26.0,
            corner_radius: 20.0,
            direction: CutDirection::LeftToRight,
        },
        width,
        height,
    )
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

fn solid_color_bitmap(width: u32, height: u32, color: Color) -> ImageBitmap {
    let r = (color.r().clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (color.g().clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (color.b().clamp(0.0, 1.0) * 255.0).round() as u8;
    let a = (color.a().clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    for px in pixels.chunks_exact_mut(4) {
        px[0] = r;
        px[1] = g;
        px[2] = b;
        px[3] = a;
    }
    ImageBitmap::from_rgba8(width, height, pixels).expect("valid solid color bitmap")
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
}

fn slider_fraction(value: f32, min: f32, max: f32) -> f32 {
    let range = max - min;
    if range.abs() <= f32::EPSILON {
        0.0
    } else {
        ((value - min) / range).clamp(0.0, 1.0)
    }
}

fn slider_value(fraction: f32, min: f32, max: f32) -> f32 {
    min + fraction.clamp(0.0, 1.0) * (max - min)
}

fn lerp_f32(start: f32, end: f32, t: f32) -> f32 {
    start + (end - start) * t.clamp(0.0, 1.0)
}

fn mix_color(start: Color, end: Color, t: f32) -> Color {
    Color(
        lerp_f32(start.r(), end.r(), t),
        lerp_f32(start.g(), end.g(), t),
        lerp_f32(start.b(), end.b(), t),
        lerp_f32(start.a(), end.a(), t),
    )
}

fn shadow_blend_mode(value: f32) -> BlendMode {
    match value.round() as i32 {
        1 => BlendMode::Overlay,
        2 => BlendMode::Multiply,
        3 => BlendMode::Screen,
        _ => BlendMode::SrcOver,
    }
}

fn shadow_blend_mode_label(mode: BlendMode) -> &'static str {
    match mode {
        BlendMode::Overlay => "Overlay",
        BlendMode::Multiply => "Multiply",
        BlendMode::Screen => "Screen",
        _ => "SrcOver",
    }
}

#[composable]
fn ValueSlider(
    label: &'static str,
    value: cranpose_core::MutableState<f32>,
    min: f32,
    max: f32,
    width: f32,
) {
    let current = value.get();
    let fraction = slider_fraction(current, min, max);
    let thumb_w = 12.0;
    let thumb_x = (width * fraction - thumb_w * 0.5).clamp(0.0, (width - thumb_w).max(0.0));

    Column(
        Modifier::empty(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(3.0)),
        move || {
            Text(
                format!("{label}: {current:.2}"),
                Modifier::empty(),
                TextStyle {
                    span_style: SpanStyle {
                        color: Some(Color(0.86, 0.9, 0.98, 0.95)),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            Box(
                Modifier::empty()
                    .size_points(width, 16.0)
                    .draw_behind(|scope| {
                        scope.draw_round_rect(
                            Brush::solid(Color(1.0, 1.0, 1.0, 0.15)),
                            CornerRadii::uniform(8.0),
                        );
                    })
                    .pointer_input((), {
                        move |scope: PointerInputScope| async move {
                            scope
                                .await_pointer_event_scope(|await_scope| async move {
                                    let mut dragging = false;
                                    loop {
                                        let event = await_scope.await_pointer_event().await;
                                        match event.kind {
                                            PointerEventKind::Down => {
                                                dragging = true;
                                                let raw =
                                                    (event.position.x / width).clamp(0.0, 1.0);
                                                value.set(slider_value(raw, min, max));
                                                event.consume();
                                            }
                                            PointerEventKind::Move if dragging => {
                                                let raw =
                                                    (event.position.x / width).clamp(0.0, 1.0);
                                                value.set(slider_value(raw, min, max));
                                                event.consume();
                                            }
                                            PointerEventKind::Up | PointerEventKind::Cancel => {
                                                dragging = false;
                                            }
                                            _ => {}
                                        }
                                    }
                                })
                                .await;
                        }
                    }),
                BoxSpec::default(),
                move || {
                    Box(
                        Modifier::empty()
                            .size_points((width * fraction).max(0.0), 16.0)
                            .draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(0.45, 0.72, 1.0, 0.8)),
                                    CornerRadii::uniform(8.0),
                                );
                            }),
                        BoxSpec::default(),
                        || {},
                    );
                    Box(
                        Modifier::empty()
                            .absolute_offset(thumb_x, 0.0)
                            .size_points(thumb_w, 16.0)
                            .draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(0.95, 0.98, 1.0, 0.95)),
                                    CornerRadii::uniform(8.0),
                                );
                            }),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

// ── Composables ─────────────────────────────────────────────────────────────

/// Main shaders demo tab composable.
#[composable]
pub(crate) fn ShadersTab(initial_section: Option<ShaderSection>) {
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
        move || {
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
            if let Some(section) = initial_section {
                Text(
                    format!("Focused Section: {}", section.label()),
                    Modifier::empty().padding(8.0).draw_behind(|scope| {
                        scope.draw_round_rect(
                            Brush::solid(Color(0.18, 0.24, 0.38, 0.85)),
                            CornerRadii::uniform(12.0),
                        );
                    }),
                    TextStyle::default(),
                );
                RenderShaderSection(section);
            } else {
                RenderShaderSection(ShaderSection::SweepGradient);
                RenderShaderSection(ShaderSection::InteractiveEffects);
                RenderShaderSection(ShaderSection::EffectSemantics);
                RenderShaderSection(ShaderSection::GraphicsLayerFields);
                RenderShaderSection(ShaderSection::MaskApi);
            }
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

            let blur_fx = smooth_rect_blur_effect(rect_w, rect_h, corner, 12.0);
            let glass_fx = liquid_glass_local_effect(rect_w, rect_h, corner);

            let checkerboard: ImageBitmap =
                cranpose_core::remember(|| generate_chessboard_bitmap(24, 17)).with(|b| b.clone());

            // Clipping parent — everything is positioned inside this box
            Box(
                Modifier::empty()
                    .size_points(area_w, area_h)
                    .clip_to_bounds()
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
                                                    move |scope| {
                                                        scope.draw_round_rect(
                                                            Brush::solid(Color(
                                                                color.0 * 0.3,
                                                                color.1 * 0.3,
                                                                color.2 * 0.3,
                                                                0.8,
                                                            )),
                                                            CornerRadii::uniform(6.0),
                                                        );
                                                    }
                                                }),
                                                TextStyle {
                                                    span_style: SpanStyle {
                                                        color: Some(color),
                                                        ..Default::default()
                                                    },
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

#[composable]
fn EffectSemanticsDemo() {
    let preview_w = 172.0;
    let preview_h = 80.0;
    let offset_x = cranpose_core::useState(|| 20.0f32);
    let offset_y = cranpose_core::useState(|| 12.0f32);
    let blur_clamp_radius = cranpose_core::useState(|| 10.0f32);
    let blur_decal_radius = cranpose_core::useState(|| 10.0f32);
    let nested_parent_blur = cranpose_core::useState(|| 8.0f32);
    let nested_child_backdrop_blur = cranpose_core::useState(|| 12.0f32);
    let checkerboard: ImageBitmap =
        cranpose_core::remember(|| generate_chessboard_bitmap(18, 10)).with(|b| b.clone());

    Column(
        Modifier::empty(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                "Effect Semantics Checks",
                Modifier::empty().padding(8.0).draw_behind(|scope| {
                    scope.draw_round_rect(
                        Brush::solid(Color(0.14, 0.18, 0.30, 0.8)),
                        CornerRadii::uniform(12.0),
                    );
                }),
                TextStyle::default(),
            );

            EffectPreviewCard(
                "Offset",
                "RenderEffect::offset(x, y)",
                Some(RenderEffect::offset(offset_x.get(), offset_y.get())),
                checkerboard.clone(),
                preview_w,
                preview_h,
            );
            Column(
                Modifier::empty(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                move || {
                    ValueSlider("offset_x", offset_x, -48.0, 48.0, 220.0);
                    ValueSlider("offset_y", offset_y, -32.0, 32.0, 220.0);
                },
            );
            EffectPreviewCard(
                "Blur Clamp",
                "blur_xy(radius, radius, TileMode::Clamp)",
                Some(RenderEffect::blur_xy(
                    blur_clamp_radius.get(),
                    blur_clamp_radius.get(),
                    TileMode::Clamp,
                )),
                checkerboard.clone(),
                preview_w,
                preview_h,
            );
            ValueSlider("blur_clamp_radius", blur_clamp_radius, 0.0, 22.0, 220.0);
            EffectPreviewCard(
                "Blur Decal",
                "blur_xy(radius, radius, TileMode::Decal)",
                Some(RenderEffect::blur_xy(
                    blur_decal_radius.get(),
                    blur_decal_radius.get(),
                    TileMode::Decal,
                )),
                checkerboard.clone(),
                preview_w,
                preview_h,
            );
            ValueSlider("blur_decal_radius", blur_decal_radius, 0.0, 22.0, 220.0);

            NestedLayerEventDemo(
                checkerboard.clone(),
                nested_parent_blur,
                nested_child_backdrop_blur,
            );
            Column(
                Modifier::empty(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                move || {
                    ValueSlider("nested_parent_blur", nested_parent_blur, 0.0, 14.0, 220.0);
                    ValueSlider(
                        "nested_child_backdrop_blur",
                        nested_child_backdrop_blur,
                        0.0,
                        18.0,
                        220.0,
                    );
                },
            );

            Text(
                "Smooth blur now uses framework blur + rounded alpha mask, and cut-mask demos show dev-facing graphics APIs.",
                Modifier::empty().padding(6.0).draw_behind(|scope| {
                    scope.draw_round_rect(
                        Brush::solid(Color(1.0, 1.0, 1.0, 0.05)),
                        CornerRadii::uniform(8.0),
                    );
                }),
                TextStyle { span_style: SpanStyle { color: Some(Color(0.9, 0.9, 1.0, 0.75)), ..Default::default() }, ..Default::default() },
            );
        },
    );
}

#[composable]
fn GraphicsLayerFieldsDemo() {
    let preview_w = 188.0;
    let preview_h = 96.0;
    let checkerboard: ImageBitmap =
        cranpose_core::remember(|| generate_chessboard_bitmap(18, 9)).with(|b| b.clone());

    Column(
        Modifier::empty(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                "GraphicsLayer Fields",
                Modifier::empty().padding(8.0).draw_behind(|scope| {
                    scope.draw_round_rect(
                        Brush::solid(Color(0.14, 0.18, 0.30, 0.8)),
                        CornerRadii::uniform(12.0),
                    );
                }),
                TextStyle::default(),
            );

            RotationAndTransformOriginCard(checkerboard.clone(), preview_w, preview_h);
            RotationAndCameraDistanceCard(checkerboard.clone(), preview_w, preview_h);
            ShapeAndClipCard(checkerboard.clone(), preview_w, preview_h);
            ShadowFieldsCard(checkerboard.clone(), preview_w, preview_h);
            ComposeShadowApiCard(preview_w, preview_h);
        },
    );
}

#[composable]
fn RotationAndTransformOriginCard(checkerboard: ImageBitmap, preview_w: f32, preview_h: f32) {
    let rotation_z = cranpose_core::useState(|| 18.0f32);
    let origin_x = cranpose_core::useState(|| 0.0f32);
    let origin_y = cranpose_core::useState(|| 0.0f32);
    let panel_bitmap: ImageBitmap =
        cranpose_core::remember(|| solid_color_bitmap(124, 62, Color(0.25, 0.45, 0.9, 0.88)))
            .with(|b| b.clone());
    let card_h = preview_h + 96.0;

    Box(
        Modifier::empty()
            .size_points(420.0, card_h)
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::solid(Color(1.0, 1.0, 1.0, 0.05)),
                    CornerRadii::uniform(12.0),
                );
            })
            .padding(8.0),
        BoxSpec::default(),
        move || {
            let panel_bitmap_outer = panel_bitmap.clone();
            let rotation_z_value = rotation_z.get();
            let origin_x_value = origin_x.get();
            let origin_y_value = origin_y.get();
            Box(
                Modifier::empty()
                    .size_points(preview_w, preview_h)
                    .rounded_corners(12.0),
                BoxSpec::default(),
                {
                    let board = checkerboard.clone();
                    let panel_bitmap = panel_bitmap_outer.clone();
                    move || {
                        Image(
                            board.clone(),
                            None,
                            Modifier::empty().size_points(preview_w, preview_h),
                            Alignment::CENTER,
                            ContentScale::Crop,
                            1.0,
                            None,
                        );
                        Text(
                            format!("pivot({origin_x_value:.2}, {origin_y_value:.2})"),
                            Modifier::empty()
                                .absolute_offset(6.0, 6.0)
                                .draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.0, 0.0, 0.0, 0.5)),
                                        CornerRadii::uniform(6.0),
                                    );
                                }),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(0.92, 0.95, 1.0, 1.0)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                        Image(
                            panel_bitmap.clone(),
                            None,
                            Modifier::empty()
                                .absolute_offset(28.0, 16.0)
                                .size_points(124.0, 62.0)
                                .graphics_layer(move || GraphicsLayer {
                                    rotation_z: rotation_z.get(),
                                    transform_origin: TransformOrigin::new(
                                        origin_x.get(),
                                        origin_y.get(),
                                    ),
                                    ..Default::default()
                                })
                                .rounded_corners(12.0),
                            Alignment::CENTER,
                            ContentScale::Crop,
                            1.0,
                            None,
                        );
                        Text(
                            "rotationZ + transformOrigin",
                            Modifier::empty()
                                .absolute_offset(34.0, 38.0)
                                .draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.0, 0.0, 0.0, 0.35)),
                                        CornerRadii::uniform(6.0),
                                    );
                                }),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(1.0, 1.0, 1.0, 0.95)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    }
                },
            );
            Column(
                Modifier::empty().absolute_offset(preview_w + 10.0, 4.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                move || {
                    Text(
                        "Rotation Z + Transform Origin",
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.95, 0.95, 1.0, 1.0)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                    Text(
                        format!(
                            "GraphicsLayer{{ rotation_z={rotation_z_value:.1}, transform_origin=({origin_x_value:.2},{origin_y_value:.2}) }}"
                        ),
                        Modifier::empty(),
                        TextStyle { span_style: SpanStyle { color: Some(Color(0.75, 0.82, 0.95, 0.95)), ..Default::default() }, ..Default::default() },
                    );
                    ValueSlider("rotation_z", rotation_z, -180.0, 180.0, 196.0);
                    ValueSlider("origin_x", origin_x, 0.0, 1.0, 196.0);
                    ValueSlider("origin_y", origin_y, 0.0, 1.0, 196.0);
                },
            );
        },
    );
}

#[composable]
fn RotationAndCameraDistanceCard(checkerboard: ImageBitmap, preview_w: f32, preview_h: f32) {
    let rotation_x = cranpose_core::useState(|| 26.0f32);
    let rotation_y = cranpose_core::useState(|| -18.0f32);
    let near_distance = cranpose_core::useState(|| 8.0f32);
    let far_distance = cranpose_core::useState(|| 24.0f32);
    let near_bitmap: ImageBitmap =
        cranpose_core::remember(|| solid_color_bitmap(74, 56, Color(0.55, 0.35, 0.9, 0.88)))
            .with(|b| b.clone());
    let far_bitmap: ImageBitmap =
        cranpose_core::remember(|| solid_color_bitmap(74, 56, Color(0.28, 0.6, 0.95, 0.88)))
            .with(|b| b.clone());
    let card_h = preview_h + 116.0;

    Box(
        Modifier::empty()
            .size_points(420.0, card_h)
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::solid(Color(1.0, 1.0, 1.0, 0.05)),
                    CornerRadii::uniform(12.0),
                );
            })
            .padding(8.0),
        BoxSpec::default(),
        move || {
            let near_bitmap_outer = near_bitmap.clone();
            let far_bitmap_outer = far_bitmap.clone();
            let near_distance_value = near_distance.get();
            let far_distance_value = far_distance.get();
            Box(
                Modifier::empty()
                    .size_points(preview_w, preview_h)
                    .rounded_corners(12.0),
                BoxSpec::default(),
                {
                    let board = checkerboard.clone();
                    let near_bitmap = near_bitmap_outer.clone();
                    let far_bitmap = far_bitmap_outer.clone();
                    move || {
                        Image(
                            board.clone(),
                            None,
                            Modifier::empty().size_points(preview_w, preview_h),
                            Alignment::CENTER,
                            ContentScale::Crop,
                            1.0,
                            None,
                        );
                        Image(
                            near_bitmap.clone(),
                            None,
                            Modifier::empty()
                                .absolute_offset(10.0, 16.0)
                                .size_points(74.0, 56.0)
                                .graphics_layer(move || GraphicsLayer {
                                    rotation_x: rotation_x.get(),
                                    rotation_y: rotation_y.get(),
                                    camera_distance: near_distance.get().max(1.0),
                                    ..Default::default()
                                })
                                .rounded_corners(10.0),
                            Alignment::CENTER,
                            ContentScale::Crop,
                            1.0,
                            None,
                        );
                        Text(
                            format!("d={near_distance_value:.1}"),
                            Modifier::empty()
                                .absolute_offset(34.0, 34.0)
                                .draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.0, 0.0, 0.0, 0.35)),
                                        CornerRadii::uniform(6.0),
                                    );
                                }),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(1.0, 1.0, 1.0, 0.95)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                        Image(
                            far_bitmap.clone(),
                            None,
                            Modifier::empty()
                                .absolute_offset(102.0, 16.0)
                                .size_points(74.0, 56.0)
                                .graphics_layer(move || GraphicsLayer {
                                    rotation_x: rotation_x.get(),
                                    rotation_y: rotation_y.get(),
                                    camera_distance: far_distance.get().max(1.0),
                                    ..Default::default()
                                })
                                .rounded_corners(10.0),
                            Alignment::CENTER,
                            ContentScale::Crop,
                            1.0,
                            None,
                        );
                        Text(
                            format!("d={far_distance_value:.1}"),
                            Modifier::empty()
                                .absolute_offset(124.0, 34.0)
                                .draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.0, 0.0, 0.0, 0.35)),
                                        CornerRadii::uniform(6.0),
                                    );
                                }),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(1.0, 1.0, 1.0, 0.95)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    }
                },
            );
            Column(
                Modifier::empty().absolute_offset(preview_w + 10.0, 4.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                move || {
                    Text(
                        "Rotation X/Y + Camera Distance",
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.95, 0.95, 1.0, 1.0)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                    Text(
                        "Same rotation_x/y, different camera_distance values.",
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.75, 0.82, 0.95, 0.95)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                    ValueSlider("rotation_x", rotation_x, -75.0, 75.0, 196.0);
                    ValueSlider("rotation_y", rotation_y, -75.0, 75.0, 196.0);
                    ValueSlider("camera_near", near_distance, 1.0, 28.0, 196.0);
                    ValueSlider("camera_far", far_distance, 1.0, 52.0, 196.0);
                },
            );
        },
    );
}

#[composable]
fn ShapeAndClipCard(checkerboard: ImageBitmap, preview_w: f32, preview_h: f32) {
    let corner_radius = cranpose_core::useState(|| 22.0f32);
    let overflow_x = cranpose_core::useState(|| 46.0f32);
    let overflow_y = cranpose_core::useState(|| 36.0f32);
    let panel_bitmap: ImageBitmap =
        cranpose_core::remember(|| solid_color_bitmap(84, 66, Color(0.15, 0.76, 0.44, 0.88)))
            .with(|b| b.clone());
    let overflow_badge: ImageBitmap =
        cranpose_core::remember(|| solid_color_bitmap(50, 20, Color(0.95, 0.25, 0.2, 0.98)))
            .with(|b| b.clone());
    let card_h = preview_h + 96.0;

    Box(
        Modifier::empty()
            .size_points(420.0, card_h)
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::solid(Color(1.0, 1.0, 1.0, 0.05)),
                    CornerRadii::uniform(12.0),
                );
            })
            .padding(8.0),
        BoxSpec::default(),
        move || {
            let panel_bitmap_outer = panel_bitmap.clone();
            let overflow_badge_outer = overflow_badge.clone();
            let corner_radius_value = corner_radius.get();
            let overflow_x_value = overflow_x.get();
            let overflow_y_value = overflow_y.get();
            Box(
                Modifier::empty()
                    .size_points(preview_w, preview_h)
                    .rounded_corners(12.0),
                BoxSpec::default(),
                {
                    let board = checkerboard.clone();
                    let panel_bitmap = panel_bitmap_outer.clone();
                    let overflow_badge = overflow_badge_outer.clone();
                    move || {
                        Image(
                            board.clone(),
                            None,
                            Modifier::empty().size_points(preview_w, preview_h),
                            Alignment::CENTER,
                            ContentScale::Crop,
                            1.0,
                            None,
                        );
                        Box(
                            Modifier::empty()
                                .absolute_offset(8.0, 14.0)
                                .size_points(84.0, 66.0)
                                .graphics_layer(move || GraphicsLayer {
                                    shape: LayerShape::Rounded(RoundedCornerShape::uniform(
                                        corner_radius.get(),
                                    )),
                                    clip: false,
                                    ..Default::default()
                                }),
                            BoxSpec::default(),
                            {
                                let panel_bitmap = panel_bitmap.clone();
                                let overflow_badge = overflow_badge.clone();
                                move || {
                                    Image(
                                        panel_bitmap.clone(),
                                        None,
                                        Modifier::empty()
                                            .size_points(84.0, 66.0)
                                            .rounded_corners(corner_radius_value),
                                        Alignment::CENTER,
                                        ContentScale::Crop,
                                        1.0,
                                        None,
                                    );
                                    Image(
                                        overflow_badge.clone(),
                                        None,
                                        Modifier::empty()
                                            .absolute_offset(overflow_x_value, overflow_y_value)
                                            .size_points(50.0, 20.0)
                                            .rounded_corners(8.0),
                                        Alignment::CENTER,
                                        ContentScale::Crop,
                                        1.0,
                                        None,
                                    );
                                    Text(
                                        "clip=false",
                                        Modifier::empty().absolute_offset(6.0, 4.0),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(0.05, 0.15, 0.08, 0.98)),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                    );
                                    Text(
                                        "overflow",
                                        Modifier::empty().absolute_offset(50.0, 38.0),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(1.0, 1.0, 1.0, 1.0)),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                    );
                                }
                            },
                        );
                        Box(
                            Modifier::empty()
                                .absolute_offset(98.0, 14.0)
                                .size_points(84.0, 66.0)
                                .graphics_layer(move || GraphicsLayer {
                                    shape: LayerShape::Rounded(RoundedCornerShape::uniform(
                                        corner_radius.get(),
                                    )),
                                    clip: true,
                                    ..Default::default()
                                }),
                            BoxSpec::default(),
                            {
                                let panel_bitmap = panel_bitmap.clone();
                                let overflow_badge = overflow_badge.clone();
                                move || {
                                    Image(
                                        panel_bitmap.clone(),
                                        None,
                                        Modifier::empty()
                                            .size_points(84.0, 66.0)
                                            .rounded_corners(corner_radius_value),
                                        Alignment::CENTER,
                                        ContentScale::Crop,
                                        1.0,
                                        None,
                                    );
                                    Image(
                                        overflow_badge.clone(),
                                        None,
                                        Modifier::empty()
                                            .absolute_offset(overflow_x_value, overflow_y_value)
                                            .size_points(50.0, 20.0)
                                            .rounded_corners(8.0),
                                        Alignment::CENTER,
                                        ContentScale::Crop,
                                        1.0,
                                        None,
                                    );
                                    Text(
                                        "clip=true",
                                        Modifier::empty().absolute_offset(8.0, 4.0),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(0.05, 0.15, 0.08, 0.98)),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                    );
                                    Text(
                                        "overflow",
                                        Modifier::empty().absolute_offset(50.0, 38.0),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(1.0, 1.0, 1.0, 1.0)),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                    );
                                }
                            },
                        );
                    }
                },
            );
            Column(
                Modifier::empty().absolute_offset(preview_w + 10.0, 4.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                move || {
                    Text(
                        "Shape + Clip",
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.95, 0.95, 1.0, 1.0)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                    Text(
                        "Rounded shape + clip toggle. Move overflow to inspect clipping.",
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.75, 0.82, 0.95, 0.95)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                    ValueSlider("corner_radius", corner_radius, 0.0, 32.0, 196.0);
                    ValueSlider("overflow_x", overflow_x, 18.0, 60.0, 196.0);
                    ValueSlider("overflow_y", overflow_y, 20.0, 48.0, 196.0);
                },
            );
        },
    );
}

#[composable]
fn ShadowFieldsCard(checkerboard: ImageBitmap, preview_w: f32, preview_h: f32) {
    let shadow_elevation = cranpose_core::useState(|| 20.0f32);
    let ambient_alpha = cranpose_core::useState(|| 0.68f32);
    let spot_alpha = cranpose_core::useState(|| 0.84f32);
    let corner_radius = cranpose_core::useState(|| 12.0f32);
    let panel_bitmap: ImageBitmap =
        cranpose_core::remember(|| solid_color_bitmap(68, 56, Color(0.88, 0.31, 0.26, 1.0)))
            .with(|b| b.clone());
    let card_h = preview_h + 116.0;

    Box(
        Modifier::empty()
            .size_points(420.0, card_h)
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::solid(Color(1.0, 1.0, 1.0, 0.05)),
                    CornerRadii::uniform(12.0),
                );
            })
            .padding(8.0),
        BoxSpec::default(),
        move || {
            let panel_bitmap_outer = panel_bitmap.clone();
            let corner_radius_value = corner_radius.get();
            Box(
                Modifier::empty()
                    .size_points(preview_w, preview_h)
                    .rounded_corners(12.0),
                BoxSpec::default(),
                {
                    let board = checkerboard.clone();
                    let panel_bitmap = panel_bitmap_outer.clone();
                    move || {
                        Image(
                            board.clone(),
                            None,
                            Modifier::empty().size_points(preview_w, preview_h),
                            Alignment::CENTER,
                            ContentScale::Crop,
                            1.0,
                            None,
                        );
                        Box(
                            Modifier::empty()
                                .size_points(preview_w, preview_h)
                                .draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.07, 0.09, 0.14, 0.18)),
                                        CornerRadii::uniform(12.0),
                                    );
                                }),
                            BoxSpec::default(),
                            move || {},
                        );
                        Image(
                            panel_bitmap.clone(),
                            None,
                            Modifier::empty()
                                .absolute_offset(14.0, 18.0)
                                .size_points(68.0, 56.0)
                                .rounded_corners(corner_radius_value),
                            Alignment::CENTER,
                            ContentScale::Crop,
                            1.0,
                            None,
                        );
                        Text(
                            "none",
                            Modifier::empty().absolute_offset(34.0, 38.0),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(1.0, 1.0, 1.0, 0.95)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                        Image(
                            panel_bitmap.clone(),
                            None,
                            Modifier::empty()
                                .absolute_offset(104.0, 18.0)
                                .size_points(68.0, 56.0)
                                .graphics_layer(move || GraphicsLayer {
                                    shadow_elevation: shadow_elevation.get(),
                                    ambient_shadow_color: Color(0.0, 0.0, 0.0, ambient_alpha.get()),
                                    spot_shadow_color: Color(0.0, 0.0, 0.0, spot_alpha.get()),
                                    shape: LayerShape::Rounded(RoundedCornerShape::uniform(
                                        corner_radius.get(),
                                    )),
                                    ..Default::default()
                                })
                                .rounded_corners(corner_radius_value),
                            Alignment::CENTER,
                            ContentScale::Crop,
                            1.0,
                            None,
                        );
                        Text(
                            "shadow",
                            Modifier::empty().absolute_offset(120.0, 38.0),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(1.0, 1.0, 1.0, 0.95)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    }
                },
            );
            Column(
                Modifier::empty().absolute_offset(preview_w + 10.0, 4.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                move || {
                    Text(
                        "Shadow Fields",
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.95, 0.95, 1.0, 1.0)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                    Text(
                        "shadow_elevation + ambient/spot colors + rounded shape",
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.75, 0.82, 0.95, 0.95)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                    ValueSlider("shadow_elevation", shadow_elevation, 0.0, 32.0, 196.0);
                    ValueSlider("ambient_alpha", ambient_alpha, 0.0, 1.0, 196.0);
                    ValueSlider("spot_alpha", spot_alpha, 0.0, 1.0, 196.0);
                    ValueSlider("corner_radius", corner_radius, 0.0, 20.0, 196.0);
                },
            );
        },
    );
}

#[composable]
fn ComposeShadowApiCard(preview_w: f32, preview_h: f32) {
    let drop_radius = cranpose_core::useState(|| 24.0f32);
    let drop_spread = cranpose_core::useState(|| 4.0f32);
    let drop_offset_x = cranpose_core::useState(|| 8.0f32);
    let drop_offset_y = cranpose_core::useState(|| 10.0f32);
    let drop_alpha = cranpose_core::useState(|| 0.88f32);
    let drop_color_mix = cranpose_core::useState(|| 0.15f32);
    let drop_brush_mix = cranpose_core::useState(|| 0.0f32);
    let drop_blend_mode = cranpose_core::useState(|| 0.0f32);

    let inner_radius = cranpose_core::useState(|| 10.0f32);
    let inner_spread = cranpose_core::useState(|| 1.0f32);
    let inner_offset_x = cranpose_core::useState(|| 6.0f32);
    let inner_offset_y = cranpose_core::useState(|| 5.0f32);
    let inner_alpha = cranpose_core::useState(|| 0.62f32);
    let inner_color_mix = cranpose_core::useState(|| 0.15f32);
    let inner_brush_mix = cranpose_core::useState(|| 0.0f32);
    let inner_blend_mode = cranpose_core::useState(|| 0.0f32);

    let corner_radius = cranpose_core::useState(|| 12.0f32);
    let controls_state = remember_lazy_list_state();
    let card_h = preview_h + 196.0;

    Box(
        Modifier::empty()
            .size_points(452.0, card_h)
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::solid(Color(1.0, 1.0, 1.0, 0.05)),
                    CornerRadii::uniform(12.0),
                );
            })
            .padding(8.0),
        BoxSpec::default(),
        move || {
            let corner_radius_value = corner_radius.get();

            Box(
                Modifier::empty()
                    .size_points(preview_w, preview_h)
                    .draw_behind(move |scope| {
                        scope.draw_round_rect(
                            Brush::linear_gradient_range(
                                vec![Color(0.08, 0.10, 0.16, 1.0), Color(0.14, 0.17, 0.24, 1.0)],
                                Point::new(0.0, 0.0),
                                Point::new(preview_w, preview_h),
                            ),
                            CornerRadii::uniform(12.0),
                        );
                    })
                    .rounded_corners(12.0),
                BoxSpec::default(),
                move || {
                    let shape =
                        LayerShape::Rounded(RoundedCornerShape::uniform(corner_radius_value));

                    Box(
                        Modifier::empty()
                            .absolute_offset(10.0, 20.0)
                            .size_points(50.0, 42.0)
                            .drop_shadow(shape, move |shadow| {
                                shadow.radius = drop_radius.get();
                                shadow.spread = drop_spread.get();
                                shadow.offset =
                                    Point::new(drop_offset_x.get(), drop_offset_y.get());
                                shadow.alpha = drop_alpha.get();
                                shadow.color = mix_color(
                                    Color(0.0, 0.0, 0.0, 1.0),
                                    Color(0.08, 0.15, 0.36, 1.0),
                                    drop_color_mix.get(),
                                );
                                let brush_mix = drop_brush_mix.get().clamp(0.0, 1.0);
                                shadow.brush =
                                    (brush_mix > 0.01).then_some(Brush::vertical_gradient(
                                        vec![
                                            shadow.color.with_alpha(0.2 + brush_mix * 0.5),
                                            shadow.color.with_alpha(0.95),
                                        ],
                                        0.0,
                                        42.0,
                                    ));
                                shadow.blend_mode = shadow_blend_mode(drop_blend_mode.get());
                            })
                            .background(Color(0.95, 0.36, 0.32, 1.0))
                            .rounded_corners(corner_radius_value),
                        BoxSpec::default(),
                        move || {},
                    );
                    Text(
                        "drop",
                        Modifier::empty().absolute_offset(20.0, 70.0),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.92, 0.95, 1.0, 0.95)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );

                    Box(
                        Modifier::empty()
                            .absolute_offset(70.0, 20.0)
                            .size_points(50.0, 42.0)
                            .background(Color(0.95, 0.36, 0.32, 1.0))
                            .rounded_corners(corner_radius_value)
                            .inner_shadow(shape, move |shadow| {
                                shadow.radius = inner_radius.get();
                                shadow.spread = inner_spread.get();
                                shadow.offset =
                                    Point::new(inner_offset_x.get(), inner_offset_y.get());
                                shadow.alpha = inner_alpha.get();
                                shadow.color = mix_color(
                                    Color(0.02, 0.02, 0.04, 1.0),
                                    Color(0.05, 0.14, 0.36, 1.0),
                                    inner_color_mix.get(),
                                );
                                let brush_mix = inner_brush_mix.get().clamp(0.0, 1.0);
                                shadow.brush =
                                    (brush_mix > 0.01).then_some(Brush::vertical_gradient(
                                        vec![
                                            shadow.color.with_alpha(0.35 + brush_mix * 0.45),
                                            shadow
                                                .color
                                                .with_alpha(0.04 + (1.0 - brush_mix) * 0.18),
                                        ],
                                        0.0,
                                        42.0,
                                    ));
                                shadow.blend_mode = shadow_blend_mode(inner_blend_mode.get());
                            }),
                        BoxSpec::default(),
                        move || {},
                    );
                    Text(
                        "inner",
                        Modifier::empty().absolute_offset(79.0, 70.0),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.92, 0.95, 1.0, 0.95)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );

                    Box(
                        Modifier::empty()
                            .absolute_offset(130.0, 20.0)
                            .size_points(50.0, 42.0)
                            .drop_shadow_value(
                                shape,
                                cranpose_ui::Shadow {
                                    radius: cranpose_ui::Dp(10.0),
                                    spread: cranpose_ui::Dp(3.0),
                                    offset: cranpose_ui::DpOffset::new(
                                        cranpose_ui::Dp(4.0),
                                        cranpose_ui::Dp(6.0),
                                    ),
                                    color: Color(0.95, 0.85, 0.15, 1.0),
                                    brush: Some(Brush::vertical_gradient(
                                        vec![
                                            Color(0.98, 0.93, 0.42, 0.45),
                                            Color(0.96, 0.54, 0.20, 0.95),
                                        ],
                                        0.0,
                                        42.0,
                                    )),
                                    alpha: 0.65,
                                    blend_mode: BlendMode::SrcOver,
                                },
                            )
                            .background(Color(0.16, 0.68, 0.95, 1.0))
                            .rounded_corners(corner_radius_value),
                        BoxSpec::default(),
                        move || {},
                    );
                    Text(
                        "static(dp)",
                        Modifier::empty().absolute_offset(136.0, 70.0),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.92, 0.95, 1.0, 0.95)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                },
            );

            LazyColumn(
                Modifier::empty()
                    .absolute_offset(preview_w + 10.0, 4.0)
                    .size_points(232.0, (card_h - 16.0).max(0.0))
                    .draw_behind(|scope| {
                        scope.draw_round_rect(
                            Brush::solid(Color(1.0, 1.0, 1.0, 0.04)),
                            CornerRadii::uniform(8.0),
                        );
                    })
                    .rounded_corners(8.0)
                    .padding(6.0),
                controls_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                move |scope| {
                    scope.item(Some(0), None, move || {
                        Text(
                            "Compose 1.9 Shadow API",
                            Modifier::empty(),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(0.95, 0.95, 1.0, 1.0)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    });
                    scope.item(Some(1), None, move || {
                        Text(
                            "radius/spread/offset/alpha/color/brush/blend + static Shadow(dp)",
                            Modifier::empty(),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(0.75, 0.82, 0.95, 0.95)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    });

                    scope.item(Some(2), None, move || {
                        ValueSlider("drop_radius", drop_radius, 0.0, 40.0, 220.0);
                    });
                    scope.item(Some(3), None, move || {
                        ValueSlider("drop_spread", drop_spread, -8.0, 18.0, 220.0);
                    });
                    scope.item(Some(4), None, move || {
                        ValueSlider("drop_offset_x", drop_offset_x, -20.0, 20.0, 220.0);
                    });
                    scope.item(Some(5), None, move || {
                        ValueSlider("drop_offset_y", drop_offset_y, -20.0, 24.0, 220.0);
                    });
                    scope.item(Some(6), None, move || {
                        ValueSlider("drop_alpha", drop_alpha, 0.0, 1.0, 220.0);
                    });
                    scope.item(Some(7), None, move || {
                        ValueSlider("drop_color_mix", drop_color_mix, 0.0, 1.0, 220.0);
                    });
                    scope.item(Some(8), None, move || {
                        ValueSlider("drop_brush_mix", drop_brush_mix, 0.0, 1.0, 220.0);
                    });
                    scope.item(Some(9), None, move || {
                        ValueSlider("drop_blend_mode", drop_blend_mode, 0.0, 3.0, 220.0);
                    });
                    scope.item(Some(10), None, move || {
                        Text(
                            format!(
                                "drop blend: {}",
                                shadow_blend_mode_label(shadow_blend_mode(drop_blend_mode.get()))
                            ),
                            Modifier::empty(),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(0.75, 0.82, 0.95, 0.95)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    });

                    scope.item(Some(11), None, move || {
                        ValueSlider("inner_radius", inner_radius, 0.0, 24.0, 220.0);
                    });
                    scope.item(Some(12), None, move || {
                        ValueSlider("inner_spread", inner_spread, -10.0, 10.0, 220.0);
                    });
                    scope.item(Some(13), None, move || {
                        ValueSlider("inner_offset_x", inner_offset_x, -18.0, 18.0, 220.0);
                    });
                    scope.item(Some(14), None, move || {
                        ValueSlider("inner_offset_y", inner_offset_y, -18.0, 18.0, 220.0);
                    });
                    scope.item(Some(15), None, move || {
                        ValueSlider("inner_alpha", inner_alpha, 0.0, 1.0, 220.0);
                    });
                    scope.item(Some(16), None, move || {
                        ValueSlider("inner_color_mix", inner_color_mix, 0.0, 1.0, 220.0);
                    });
                    scope.item(Some(17), None, move || {
                        ValueSlider("inner_brush_mix", inner_brush_mix, 0.0, 1.0, 220.0);
                    });
                    scope.item(Some(18), None, move || {
                        ValueSlider("inner_blend_mode", inner_blend_mode, 0.0, 3.0, 220.0);
                    });
                    scope.item(Some(19), None, move || {
                        Text(
                            format!(
                                "inner blend: {}",
                                shadow_blend_mode_label(shadow_blend_mode(inner_blend_mode.get()))
                            ),
                            Modifier::empty(),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(0.75, 0.82, 0.95, 0.95)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    });
                    scope.item(Some(20), None, move || {
                        ValueSlider("corner_radius", corner_radius, 0.0, 20.0, 220.0);
                    });
                },
            );
        },
    );
}

#[composable]
fn MaskApiDemo() {
    let preview_w = 188.0;
    let preview_h = 88.0;
    let half_progress = cranpose_core::useState(|| 0.5f32);
    let half_feather = cranpose_core::useState(|| 26.0f32);
    let half_corner = cranpose_core::useState(|| 20.0f32);
    let top_progress = cranpose_core::useState(|| 0.62f32);
    let top_feather = cranpose_core::useState(|| 20.0f32);
    let top_corner = cranpose_core::useState(|| 14.0f32);
    let dst_alpha = cranpose_core::useState(|| 1.0f32);
    let dst_fade_start = cranpose_core::useState(|| 24.0f32);
    let dst_fade_end = cranpose_core::useState(|| 60.0f32);
    let checkerboard: ImageBitmap =
        cranpose_core::remember(|| generate_chessboard_bitmap(18, 9)).with(|b| b.clone());

    Column(
        Modifier::empty(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                "Cut / Opacity Mask APIs",
                Modifier::empty().padding(8.0).draw_behind(|scope| {
                    scope.draw_round_rect(
                        Brush::solid(Color(0.14, 0.18, 0.30, 0.8)),
                        CornerRadii::uniform(12.0),
                    );
                }),
                TextStyle::default(),
            );

            MaskPreviewCard(
                "Half screen cut",
                "gradient_cut_mask_effect(progress/feather/corner, LeftToRight)",
                gradient_cut_mask_effect(
                    &GradientCutMaskSpec {
                        progress: half_progress.get(),
                        feather: half_feather.get(),
                        corner_radius: half_corner.get(),
                        direction: CutDirection::LeftToRight,
                    },
                    preview_w,
                    preview_h,
                ),
                checkerboard.clone(),
                preview_w,
                preview_h,
            );
            Column(
                Modifier::empty(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                move || {
                    ValueSlider("half_progress", half_progress, 0.0, 1.0, 220.0);
                    ValueSlider("half_feather", half_feather, 0.0, 40.0, 220.0);
                    ValueSlider("half_corner", half_corner, 0.0, 32.0, 220.0);
                },
            );
            MaskPreviewCard(
                "Top fade cut",
                "gradient_cut_mask_effect(progress/feather/corner, TopToBottom)",
                gradient_cut_mask_effect(
                    &GradientCutMaskSpec {
                        progress: top_progress.get(),
                        feather: top_feather.get(),
                        corner_radius: top_corner.get(),
                        direction: CutDirection::TopToBottom,
                    },
                    preview_w,
                    preview_h,
                ),
                checkerboard.clone(),
                preview_w,
                preview_h,
            );
            Column(
                Modifier::empty(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                move || {
                    ValueSlider("top_progress", top_progress, 0.0, 1.0, 220.0);
                    ValueSlider("top_feather", top_feather, 0.0, 40.0, 220.0);
                    ValueSlider("top_corner", top_corner, 0.0, 32.0, 220.0);
                },
            );
            DstOutDrawWithContentCard(
                checkerboard.clone(),
                preview_w,
                preview_h,
                dst_alpha,
                dst_fade_start,
                dst_fade_end,
            );
        },
    );
}

#[composable]
fn MaskPreviewCard(
    title: &'static str,
    subtitle: &'static str,
    effect: RenderEffect,
    checkerboard: ImageBitmap,
    preview_w: f32,
    preview_h: f32,
) {
    Box(
        Modifier::empty()
            .size_points(360.0, preview_h + 18.0)
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::solid(Color(1.0, 1.0, 1.0, 0.05)),
                    CornerRadii::uniform(12.0),
                );
            })
            .padding(8.0),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .size_points(preview_w, preview_h)
                    .graphics_layer({
                        let effect_for_layer = effect.clone();
                        move || GraphicsLayer {
                            render_effect: Some(effect_for_layer.clone()),
                            ..Default::default()
                        }
                    })
                    .rounded_corners(12.0),
                BoxSpec::default(),
                {
                    let board = checkerboard.clone();
                    move || {
                        Image(
                            board.clone(),
                            None,
                            Modifier::empty().size_points(preview_w, preview_h),
                            Alignment::CENTER,
                            ContentScale::Crop,
                            1.0,
                            None,
                        );
                        Text(
                            "MASK",
                            Modifier::empty()
                                .absolute_offset(preview_w * 0.5 - 20.0, preview_h * 0.45)
                                .draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.0, 0.0, 0.0, 0.42)),
                                        CornerRadii::uniform(6.0),
                                    );
                                }),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(1.0, 1.0, 1.0, 0.95)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    }
                },
            );
            Column(
                Modifier::empty().absolute_offset(preview_w + 10.0, 4.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                move || {
                    Text(
                        title,
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.95, 0.95, 1.0, 1.0)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                    Text(
                        subtitle,
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.75, 0.82, 0.95, 0.95)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn DstOutDrawWithContentCard(
    checkerboard: ImageBitmap,
    preview_w: f32,
    preview_h: f32,
    preview_alpha: cranpose_core::MutableState<f32>,
    fade_start: cranpose_core::MutableState<f32>,
    fade_end: cranpose_core::MutableState<f32>,
) {
    Box(
        Modifier::empty()
            .size_points(360.0, preview_h + 18.0)
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::solid(Color(1.0, 1.0, 1.0, 0.05)),
                    CornerRadii::uniform(12.0),
                );
            })
            .padding(8.0),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .size_points(preview_w, preview_h)
                    .rounded_corners(12.0)
                    .draw_behind(|scope| {
                        scope.draw_round_rect(
                            Brush::linear_gradient(vec![
                                Color(0.55, 0.25, 0.22, 1.0),
                                Color(0.16, 0.32, 0.65, 1.0),
                            ]),
                            CornerRadii::uniform(12.0),
                        );
                    }),
                BoxSpec::default(),
                move || {
                    Text(
                        "UNDERLAY CONTENT",
                        Modifier::empty()
                            .absolute_offset(12.0, 8.0)
                            .draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(0.0, 0.0, 0.0, 0.5)),
                                    CornerRadii::uniform(6.0),
                                );
                            }),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(1.0, 0.95, 0.75, 1.0)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                    Text(
                        "Top fade should reveal this",
                        Modifier::empty()
                            .absolute_offset(12.0, preview_h - 24.0)
                            .draw_behind(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(0.0, 0.0, 0.0, 0.45)),
                                    CornerRadii::uniform(6.0),
                                );
                            }),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.95, 0.95, 1.0, 1.0)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                },
            );

            Box(
                Modifier::empty()
                    .size_points(preview_w, preview_h)
                    // Compose parity:
                    // graphicsLayer { compositingStrategy = Offscreen } +
                    // drawWithContent { drawContent(); drawRect(Brush.verticalGradient(...), DstOut) }
                    .graphics_layer({
                        move || GraphicsLayer {
                            alpha: preview_alpha.get().clamp(0.0, 1.0),
                            compositing_strategy: CompositingStrategy::Offscreen,
                            ..Default::default()
                        }
                    })
                    .draw_with_content({
                        move |scope| {
                            scope.draw_content();
                            if preview_alpha.get() > 0.0 {
                                let start = fade_start.get().clamp(0.0, preview_h);
                                let mut end = fade_end.get().clamp(0.0, preview_h);
                                if end <= start {
                                    end = (start + 1.0).min(preview_h);
                                }
                                scope.draw_rect_blend(
                                    Brush::vertical_gradient(
                                        vec![Color::BLACK, Color::from_rgba_u8(0, 0, 0, 0)],
                                        start,
                                        end,
                                    ),
                                    BlendMode::DstOut,
                                );
                            }
                        }
                    })
                    .rounded_corners(12.0),
                BoxSpec::default(),
                {
                    let board = checkerboard.clone();
                    move || {
                        Image(
                            board.clone(),
                            None,
                            Modifier::empty().size_points(preview_w, preview_h),
                            Alignment::CENTER,
                            ContentScale::Crop,
                            1.0,
                            None,
                        );
                        Text(
                            "TOP LAYER",
                            Modifier::empty()
                                .absolute_offset(preview_w * 0.5 - 20.0, preview_h * 0.45)
                                .draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.0, 0.0, 0.0, 0.42)),
                                        CornerRadii::uniform(6.0),
                                    );
                                }),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(1.0, 1.0, 1.0, 0.95)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    }
                },
            );
            Column(
                Modifier::empty().absolute_offset(preview_w + 10.0, 4.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                move || {
                    let preview_alpha_value = preview_alpha.get();
                    let fade_start_value = fade_start.get();
                    let fade_end_value = fade_end.get();
                    Text(
                        "DstOut vertical fade",
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.95, 0.95, 1.0, 1.0)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                    Text(
                        "graphics_layer(Offscreen) + draw_with_content + DstOut over visible underlay",
                        Modifier::empty(),
                        TextStyle { span_style: SpanStyle { color: Some(Color(0.75, 0.82, 0.95, 0.95)), ..Default::default() }, ..Default::default() },
                    );
                    Text(
                        format!(
                            "alpha={preview_alpha_value:.2}, fade=[{fade_start_value:.1}..{fade_end_value:.1}]"
                        ),
                        Modifier::empty(),
                        TextStyle { span_style: SpanStyle { color: Some(Color(0.82, 0.88, 0.98, 0.92)), ..Default::default() }, ..Default::default() },
                    );
                    ValueSlider("preview_alpha", preview_alpha, 0.0, 1.0, 156.0);
                    ValueSlider("fade_start", fade_start, 0.0, preview_h - 4.0, 156.0);
                    ValueSlider("fade_end", fade_end, 4.0, preview_h, 156.0);
                },
            );
        },
    );
}

#[composable]
fn EffectPreviewCard(
    title: &'static str,
    subtitle: &'static str,
    effect: Option<RenderEffect>,
    checkerboard: ImageBitmap,
    preview_w: f32,
    preview_h: f32,
) {
    Box(
        Modifier::empty()
            .size_points(360.0, preview_h + 18.0)
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::solid(Color(1.0, 1.0, 1.0, 0.05)),
                    CornerRadii::uniform(12.0),
                );
            })
            .padding(8.0),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .size_points(preview_w, preview_h)
                    .graphics_layer({
                        let effect_for_layer = effect.clone();
                        move || GraphicsLayer {
                            render_effect: effect_for_layer.clone(),
                            ..Default::default()
                        }
                    })
                    .rounded_corners(12.0),
                BoxSpec::default(),
                {
                    let board = checkerboard.clone();
                    move || {
                        Image(
                            board.clone(),
                            None,
                            Modifier::empty().size_points(preview_w, preview_h),
                            Alignment::CENTER,
                            ContentScale::Crop,
                            1.0,
                            None,
                        );
                        Text(
                            "EDGE",
                            Modifier::empty()
                                .absolute_offset(8.0, 8.0)
                                .draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.0, 0.0, 0.0, 0.55)),
                                        CornerRadii::uniform(6.0),
                                    );
                                }),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(1.0, 0.9, 0.2, 1.0)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                        Text(
                            "MID",
                            Modifier::empty()
                                .absolute_offset(preview_w * 0.5 - 16.0, preview_h * 0.45)
                                .draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.0, 0.0, 0.0, 0.45)),
                                        CornerRadii::uniform(6.0),
                                    );
                                }),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(0.6, 1.0, 0.9, 1.0)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                    }
                },
            );

            Column(
                Modifier::empty().absolute_offset(preview_w + 10.0, 4.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                move || {
                    Text(
                        title,
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.95, 0.95, 1.0, 1.0)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                    Text(
                        subtitle,
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.75, 0.82, 0.95, 0.95)),
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn NestedLayerEventDemo(
    checkerboard: ImageBitmap,
    parent_blur_radius: cranpose_core::MutableState<f32>,
    child_backdrop_blur_radius: cranpose_core::MutableState<f32>,
) {
    let area_w = 360.0;
    let area_h = 150.0;
    let parent_w = 214.0;
    let parent_h = 116.0;

    Box(
        Modifier::empty()
            .size_points(area_w, area_h)
            .rounded_corners(14.0)
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::solid(Color(1.0, 1.0, 1.0, 0.04)),
                    CornerRadii::uniform(14.0),
                );
            }),
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

                Text(
                    "Nested parent render_effect + child backdrop",
                    Modifier::empty()
                        .absolute_offset(8.0, 8.0)
                        .draw_behind(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color(0.0, 0.0, 0.0, 0.55)),
                                CornerRadii::uniform(7.0),
                            );
                        }),
                    TextStyle {
                        span_style: SpanStyle {
                            color: Some(Color(0.9, 0.95, 1.0, 0.95)),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                );

                Box(
                    Modifier::empty()
                        .absolute_offset(24.0, 24.0)
                        .size_points(parent_w, parent_h)
                        .graphics_layer({
                            move || GraphicsLayer {
                                render_effect: Some(RenderEffect::blur_xy(
                                    parent_blur_radius.get(),
                                    parent_blur_radius.get(),
                                    TileMode::Clamp,
                                )),
                                ..Default::default()
                            }
                        }),
                    BoxSpec::default(),
                    {
                        move || {
                            Box(
                                Modifier::empty().fill_max_size().draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.45, 0.75, 1.0, 0.25)),
                                        CornerRadii::uniform(14.0),
                                    );
                                }),
                                BoxSpec::default(),
                                move || {
                                    Text(
                                        "Parent render_effect",
                                        Modifier::empty().absolute_offset(10.0, 8.0).draw_behind(
                                            |scope| {
                                                scope.draw_round_rect(
                                                    Brush::solid(Color(0.0, 0.0, 0.0, 0.55)),
                                                    CornerRadii::uniform(7.0),
                                                );
                                            },
                                        ),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(1.0, 1.0, 1.0, 0.95)),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                    );

                                    Box(
                                        Modifier::empty()
                                            .absolute_offset(108.0, 36.0)
                                            .size_points(92.0, 64.0)
                                            .backdrop_effect(RenderEffect::blur(
                                                child_backdrop_blur_radius.get(),
                                            ))
                                            .draw_behind(|scope| {
                                                scope.draw_round_rect(
                                                    Brush::solid(Color(0.6, 1.0, 0.7, 0.28)),
                                                    CornerRadii::uniform(12.0),
                                                );
                                            }),
                                        BoxSpec::new().content_alignment(Alignment::CENTER),
                                        || {
                                            Text(
                                                "Child backdrop",
                                                Modifier::empty().draw_behind(|scope| {
                                                    scope.draw_round_rect(
                                                        Brush::solid(Color(0.0, 0.0, 0.0, 0.45)),
                                                        CornerRadii::uniform(6.0),
                                                    );
                                                }),
                                                TextStyle {
                                                    span_style: SpanStyle {
                                                        color: Some(Color(1.0, 1.0, 1.0, 0.95)),
                                                        ..Default::default()
                                                    },
                                                    ..Default::default()
                                                },
                                            );
                                        },
                                    );
                                },
                            );
                        }
                    },
                );
            }
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
            .graphics_layer(move || GraphicsLayer {
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
                                    PointerEventKind::Scroll
                                    | PointerEventKind::Enter
                                    | PointerEventKind::Exit => {}
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
                    span_style: SpanStyle {
                        color: Some(Color(1.0, 1.0, 1.0, 0.9)),
                        ..Default::default()
                    },
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
    fn smooth_rect_blur_effect_chains_blur_and_mask() {
        let effect = smooth_rect_blur_effect(140.0, 100.0, 20.0, 12.0);
        let RenderEffect::Chain { first, second } = effect else {
            panic!("expected chained effect");
        };
        assert!(matches!(
            *first,
            RenderEffect::Blur {
                radius_x: 12.0,
                radius_y: 12.0,
                edge_treatment: TileMode::Clamp
            }
        ));
        assert!(matches!(*second, RenderEffect::Shader { .. }));
    }

    #[test]
    fn half_cut_mask_effect_uses_half_progress_and_ltr_direction() {
        let effect = half_cut_mask_effect(140.0, 100.0);
        let RenderEffect::Shader { shader } = effect else {
            panic!("expected shader effect");
        };
        let u = shader.uniforms();

        assert_eq!(u[0], 140.0);
        assert_eq!(u[1], 100.0);
        assert_eq!(u[2], 0.5);
        assert_eq!(u[5], 0.0);
    }

    #[test]
    fn vertical_gradient_brush_encodes_requested_range() {
        let brush = Brush::vertical_gradient(
            vec![Color::BLACK, Color::from_rgba_u8(0, 0, 0, 0)],
            24.0,
            60.0,
        );
        match brush {
            Brush::LinearGradient { start, end, .. } => {
                assert_eq!(start, Point::new(0.0, 24.0));
                assert_eq!(end, Point::new(0.0, 60.0));
            }
            other => panic!("expected LinearGradient, got {other:?}"),
        }
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
    fn apply_drag_position_updates_state() {
        let runtime = cranpose_core::runtime::Runtime::new(Arc::new(
            cranpose_core::runtime::DefaultScheduler,
        ));
        let pos = cranpose_core::MutableState::with_runtime(Point::ZERO, runtime.handle());

        apply_drag_position(&pos, Point::new(42.0, 24.0));

        assert_eq!(pos.get(), Point::new(42.0, 24.0));
    }

    #[test]
    fn slider_fraction_clamps_and_handles_degenerate_range() {
        assert_eq!(slider_fraction(15.0, 10.0, 20.0), 0.5);
        assert_eq!(slider_fraction(30.0, 10.0, 20.0), 1.0);
        assert_eq!(slider_fraction(5.0, 10.0, 20.0), 0.0);
        assert_eq!(slider_fraction(10.0, 10.0, 10.0), 0.0);
    }

    #[test]
    fn slider_value_maps_fraction_and_clamps() {
        assert_eq!(slider_value(0.0, 2.0, 10.0), 2.0);
        assert_eq!(slider_value(0.5, 2.0, 10.0), 6.0);
        assert_eq!(slider_value(1.0, 2.0, 10.0), 10.0);
        assert_eq!(slider_value(2.0, 2.0, 10.0), 10.0);
        assert_eq!(slider_value(-1.0, 2.0, 10.0), 2.0);
    }

    #[test]
    fn shader_section_parser_accepts_aliases() {
        assert_eq!(
            ShaderSection::from_startup_name("interactive-effects"),
            Some(ShaderSection::InteractiveEffects)
        );
        assert_eq!(
            ShaderSection::from_startup_name("Effect Semantics"),
            Some(ShaderSection::EffectSemantics)
        );
        assert_eq!(
            ShaderSection::from_startup_name("graphics_layer_fields"),
            Some(ShaderSection::GraphicsLayerFields)
        );
        assert_eq!(
            ShaderSection::from_startup_name("mask"),
            Some(ShaderSection::MaskApi)
        );
    }

    #[test]
    fn shader_section_parser_rejects_unknown_names() {
        assert_eq!(ShaderSection::from_startup_name(""), None);
        assert_eq!(
            ShaderSection::from_startup_name("definitely-not-a-section"),
            None
        );
    }
}
