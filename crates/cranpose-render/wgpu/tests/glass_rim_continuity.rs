mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_liquid::prelude::*;
use cranpose_macros::composable;
use cranpose_render_common::{
    Renderer,
    graph::{
        DrawPrimitiveNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform,
        RenderGraph, RenderNode,
    },
};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui::{
    Modifier,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{
    Brush, Color, DrawPrimitive, GraphicsLayer, Rect, RenderEffect, RuntimeShader,
};

const FRAME_WIDTH: u32 = 128;
const FRAME_HEIGHT: u32 = 96;

const TAB_VIEW_WIDTH: f32 = 400.0;
const TAB_VIEW_HEIGHT: f32 = 200.0;
const TAB_SCALE: f32 = 2.75;
const TAB_FRAME_WIDTH: u32 = (TAB_VIEW_WIDTH * TAB_SCALE) as u32;
const TAB_FRAME_HEIGHT: u32 = (TAB_VIEW_HEIGHT * TAB_SCALE) as u32;
const SEAM_TOLERANCE: i32 = 6;

const PANE: Rect = Rect {
    x: 16.0,
    y: 16.0,
    width: 96.0,
    height: 64.0,
};

fn solid_rect(rect: Rect, color: Color) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode {
            primitive: DrawPrimitive::Rect {
                rect,
                brush: Brush::solid(color),
                stroke: None,
            },
            clip: None,
        }),
    })
}

fn regular_capsule_glass_scene() -> RenderGraph {
    let mut tail = RuntimeShader::new(cranpose_ui_graphics::LIQUID_GLASS_WGSL);
    tail.set_float2(0, 0.0, 0.0);
    tail.set_float(6, -1.0);
    tail.set_float(11, 0.9);
    tail.set_float4(14, 1.0, 1.0, 1.0, 0.07);
    tail.set_float(18, 1.5);
    tail.set_float(20, 0.42);
    tail.set_float(24, 1.0);
    tail.set_float(100, 1.0);
    tail.set_float(111, 1.0);
    tail.set_float(121, 1.0);
    let glass = shared_test_support::layer_node(
        PANE,
        ProjectiveTransform::identity(),
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::blur(6.0).then(RenderEffect::runtime_shader(tail))),
            ..GraphicsLayer::default()
        },
        vec![],
    );
    RenderGraph::new(shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: FRAME_WIDTH as f32,
            height: FRAME_HEIGHT as f32,
        },
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![
            solid_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: FRAME_WIDTH as f32,
                    height: FRAME_HEIGHT as f32,
                },
                Color(0.45, 0.45, 0.47, 1.0),
            ),
            RenderNode::Layer(Box::new(glass)),
        ],
    ))
}

fn luma(frame: &CapturedFrame, x: i32, y: i32) -> i32 {
    if x < 0 || y < 0 || x >= frame.width as i32 || y >= frame.height as i32 {
        return 0;
    }
    let index = ((y as usize) * (frame.width as usize) + (x as usize)) * 4;
    frame.pixels[index] as i32 + frame.pixels[index + 1] as i32 + frame.pixels[index + 2] as i32
}

fn rim_peak_at_angle(frame: &CapturedFrame, cx: f32, cy: f32, radius: f32, theta: f32) -> i32 {
    let mut peak = 0;
    let mut r = radius - 3.0;
    while r <= radius + 1.5 {
        let x = (cx + r * theta.cos()).round() as i32;
        let y = (cy + r * theta.sin()).round() as i32;
        peak = peak.max(luma(frame, x, y));
        r += 0.5;
    }
    peak
}

#[composable]
#[allow(non_snake_case)]
fn TabBarScene() {
    LiquidTheme(
        LiquidThemeSpec {
            scheme: SchemeMode::Light,
            ..LiquidThemeSpec::default()
        },
        move || {
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .background(cranpose_ui::Color::from_rgb_u8(242, 242, 247)),
                BoxSpec::default(),
                move || {
                    Box(
                        Modifier::empty().offset(40.0, 80.0).width(320.0),
                        BoxSpec::default(),
                        || {
                            LiquidTabBar(
                                Modifier::empty(),
                                LiquidTabBarSpec::default(),
                                2,
                                |_| {},
                                |scope| {
                                    scope.tab(icons::STAR, "Discover");
                                    scope.tab(icons::SHOPPING_BAG, "Browse");
                                    scope.tab(icons::ACCOUNT_CIRCLE, "Account");
                                },
                            );
                        },
                    );
                },
            );
        },
    );
}

fn capture_tab_bar() -> Result<CapturedFrame, String> {
    let (_lock, mut renderer) = support::headless_renderer_parts()?;
    let app_context = cranpose_ui::AppContext::new();
    renderer.attach_app_context_services(&app_context);
    let mut shell = AppShell::new(
        renderer,
        location_key(file!(), line!(), column!()),
        TabBarScene,
    );
    shell.renderer().set_root_scale(TAB_SCALE);
    shell.set_density(TAB_SCALE);
    shell.set_buffer_size(TAB_FRAME_WIDTH, TAB_FRAME_HEIGHT);
    shell.set_viewport(TAB_VIEW_WIDTH, TAB_VIEW_HEIGHT);
    shell.update();
    shell.update();
    shell
        .renderer()
        .capture_frame(TAB_FRAME_WIDTH, TAB_FRAME_HEIGHT)
        .map_err(|err| format!("tab bar capture failed: {err:?}"))
}

fn median_luma(frame: &CapturedFrame, x_range: std::ops::Range<i32>, y: i32) -> i32 {
    let mut samples: Vec<i32> = x_range.map(|x| luma(frame, x, y)).collect();
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[test]
fn a_surface_glass_edge_carries_no_seam_darker_than_its_body() {
    let frame = match capture_tab_bar() {
        Ok(frame) => frame,
        Err(err) => {
            eprintln!("skipping glass edge assertions because headless WGPU init failed: {err}");
            return;
        }
    };

    let scan_y = ((80.0 + 25.0) * TAB_SCALE) as i32;
    let backdrop = median_luma(&frame, 20..60, scan_y);
    let edge_x = (20..TAB_FRAME_WIDTH as i32 - 60)
        .find(|x| luma(&frame, *x, scan_y) - backdrop > 15)
        .expect("the bar's left cap must be on the scanline; its drop shadow darkens the backdrop first");
    let interior = median_luma(&frame, edge_x + 40..edge_x + 90, scan_y);
    assert!(
        interior - backdrop > 10,
        "the pane must read as material over the backdrop: interior {interior} vs backdrop {backdrop}"
    );

    let seam_min = (edge_x + 4..edge_x + 30)
        .map(|x| luma(&frame, x, scan_y))
        .min()
        .expect("seam band");
    assert!(
        seam_min >= interior - SEAM_TOLERANCE,
        "no band inside the rim may fall below the body (reference: body dead \
         flat at 753 up to a single +9 rim line): seam minimum {seam_min} vs \
         interior {interior} at scan y {scan_y}, edge x {edge_x}"
    );
}

// The capsule's specular rim must read as one continuous line: a band
// narrower than the pixel grid renders as bright dashes with dropouts
// between them (the "staircase" rim), so along the cap arc the per-angle
// rim peak collapses at the angles that fall between pixel rows.
#[test]
fn a_capsule_rim_reads_as_a_continuous_line_around_its_cap() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping rim continuity assertions because headless WGPU init failed: {}",
                err
            );
            return;
        }
    };

    renderer.scene_mut().graph = Some(regular_capsule_glass_scene());
    let frame = renderer
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("rim continuity capture should succeed");

    let inradius = PANE.height * 0.5;
    let cap_center_x = PANE.x + inradius;
    let cap_center_y = PANE.y + inradius;
    let interior = luma(&frame, cap_center_x as i32 + 8, cap_center_y as i32);

    let mut peaks = Vec::new();
    let mut deg = 100.0_f32;
    while deg <= 260.0 {
        let theta = deg.to_radians();
        let rim = rim_peak_at_angle(&frame, cap_center_x, cap_center_y, inradius, theta);
        peaks.push(((rim - interior).max(0), deg));
        deg += 4.0;
    }

    let brightest = peaks.iter().map(|(value, _)| *value).max().unwrap_or(0);
    assert!(
        brightest >= 40,
        "the rim line must exist at all around the cap: brightest rim excess \
         {brightest} over interior {interior}"
    );
    let mut dashes = Vec::new();
    for pair in peaks.windows(2) {
        let (a, angle_a) = pair[0];
        let (b, angle_b) = pair[1];
        let top = a.max(b);
        if top >= 25 && a.min(b) * 5 < top * 2 {
            dashes.push((angle_a, angle_b, a, b));
        }
    }
    assert!(
        dashes.is_empty(),
        "the rim's brightness must vary smoothly along the cap arc — material \
         lighting is low-frequency, a collapse between neighboring angles is a \
         sampling dropout: {dashes:?} (angle, angle, peak, peak) — a sub-pixel \
         rim band renders as disconnected sparkles instead of a line"
    );
}
