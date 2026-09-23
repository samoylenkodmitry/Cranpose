use cranpose::{rememberWindowStateAt, WindowConfig, WindowModifierExt};
use cranpose_animation::{
    animateFloatAsState, infiniteRepeatable, rememberInfiniteTransition, AnimationSpec,
    AnimationType, RepeatMode, StartOffset,
};
use cranpose_ui::{
    composable, Alignment, Box, BoxSpec, Brush, Color, Column, ColumnSpec, CornerRadii,
    GraphicsLayer, LinearArrangement, Modifier, Text,
};
use cranpose_ui_graphics::CompositingStrategy;

use super::{
    chrome_tabs::{label_style, INK},
    floating_input::{rememberFloatingInput, FloatingInputModifierExt},
    shader_rect::{
        fire_shader_effect, FireShaderParams, FireStyle, FIRE_BLUE_ICE, FIRE_CLASSIC, FIRE_EMERALD,
        FIRE_NEON_THIN,
    },
};

pub const CONTENT_WIDTH: f32 = 300.0;
pub const CONTENT_HEIGHT: f32 = 150.0;
pub const FLAME_REACH: f32 = 170.0;
pub const EDGE_FADE: f32 = 30.0;
pub const HALO_FALLOFF: f32 = 132.0;
pub const GLOW_SPREAD: f32 = 8.0;
pub const WINDOW_WIDTH: f32 = CONTENT_WIDTH + 2.0 * FLAME_REACH;
pub const WINDOW_HEIGHT: f32 = CONTENT_HEIGHT + 2.0 * FLAME_REACH;

const STYLES: [FireStyle; 4] = [FIRE_CLASSIC, FIRE_BLUE_ICE, FIRE_EMERALD, FIRE_NEON_THIN];
const CLOCK_PERIOD_MS: u64 = 60_000;

pub(crate) fn style_after_clicks(clicks: u32) -> FireStyle {
    STYLES[clicks as usize % STYLES.len()]
}

#[composable]
pub fn flame_window_app() {
    let state = rememberWindowStateAt(180.0, 160.0, WINDOW_WIDTH, WINDOW_HEIGHT);
    Box(
        Modifier::empty().window(
            WindowConfig::borderless_for_state("Cranpose Flame", state)
                .with_transparent(true)
                .with_shadow(false)
                .with_always_on_top(true),
        ),
        BoxSpec::default(),
        FlameWindow,
    );
}

#[composable]
fn FlameWindow() {
    let clock = rememberInfiniteTransition("flame_clock");
    let time = clock.animateFloat(
        0.0,
        1.0,
        infiniteRepeatable(
            AnimationSpec::linear(CLOCK_PERIOD_MS),
            RepeatMode::Restart,
            StartOffset::default(),
        ),
        "flame_time",
    );
    let input = rememberFloatingInput();
    let style = style_after_clicks(input.clicks());

    let intensity = animateFloatAsState(
        blaze(
            style.base_intensity,
            style.hover_intensity_mult,
            style.press_intensity_mult,
            input.hovered(),
            input.pressed(),
        ),
        AnimationType::default(),
        "flame_intensity",
    );
    let smoke = animateFloatAsState(
        blaze(
            style.base_smoke_scale,
            style.hover_smoke_mult,
            style.press_smoke_mult,
            input.hovered(),
            input.pressed(),
        ),
        AnimationType::default(),
        "flame_smoke",
    );
    let core = animateFloatAsState(
        if input.pressed() {
            style.base_core_scale * style.press_core_mult
        } else {
            style.base_core_scale
        },
        AnimationType::default(),
        "flame_core",
    );

    Box(
        Modifier::empty()
            .size_points(WINDOW_WIDTH, WINDOW_HEIGHT)
            .floating_input(input)
            .graphics_layer(move || GraphicsLayer {
                render_effect: Some(fire_shader_effect(&FireShaderParams {
                    resolution_w: WINDOW_WIDTH,
                    resolution_h: WINDOW_HEIGHT,
                    time: time.get(),
                    band_width: style.band_width,
                    corner_radius: style.corner_radius,
                    contour_w: CONTENT_WIDTH,
                    contour_h: CONTENT_HEIGHT,
                    smoke_scale: smoke.get(),
                    intensity: intensity.get(),
                    smoke_opacity: style.base_smoke_opacity,
                    core_scale: core.get(),
                    smoke_blue_tint: style.smoke_blue_tint,
                    thin_mode: style.thin_mode,
                    color_tint: style.color_tint,
                    edge_fade: EDGE_FADE,
                    halo_falloff: HALO_FALLOFF,
                    glow_spread: GLOW_SPREAD,
                })),
                compositing_strategy: CompositingStrategy::Offscreen,
                ..Default::default()
            }),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || Panel(style),
    );
}

#[composable]
fn Panel(style: FireStyle) {
    Box(
        Modifier::empty()
            .size_points(CONTENT_WIDTH, CONTENT_HEIGHT)
            .draw_behind(move |scope| {
                scope.draw_round_rect(
                    Brush::solid(style.bg_color),
                    CornerRadii::uniform(style.corner_radius),
                );
            }),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            Column(
                Modifier::empty().padding(18.0),
                ColumnSpec {
                    vertical_arrangement: LinearArrangement::spaced_by(8.0),
                    ..ColumnSpec::default()
                },
                move || {
                    Text(style.label, Modifier::empty(), label_style(18.0, INK));
                    Text(
                        "a borderless window with nothing behind it but the desktop",
                        Modifier::empty(),
                        label_style(12.0, Color(1.0, 1.0, 1.0, 0.72)),
                    );
                    Text(
                        "drag to move, hover to fan it, click to change the fire",
                        Modifier::empty(),
                        label_style(12.0, Color(1.0, 1.0, 1.0, 0.55)),
                    );
                },
            );
        },
    );
}

fn blaze(base: f32, hover_mult: f32, press_mult: f32, hovered: bool, pressed: bool) -> f32 {
    if pressed {
        base * press_mult
    } else if hovered {
        base * hover_mult
    } else {
        base
    }
}

#[cfg(test)]
#[path = "tests/flame_window_tests.rs"]
mod tests;
