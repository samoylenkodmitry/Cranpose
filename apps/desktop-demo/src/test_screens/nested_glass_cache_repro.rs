use cranpose_core::{MutableState, State};
use cranpose_ui::{
    composable, Box, BoxSpec, Color, GraphicsLayer, Modifier, RenderEffect, RuntimeShader, Size,
    Text, TextStyle,
};

pub const SCREEN_WIDTH: f32 = 600.0;
pub const SCREEN_HEIGHT: f32 = 400.0;

pub const TICK_RECT: [f32; 4] = [8.0, 8.0, 48.0, 24.0];
pub const BACKGROUND_TOGGLE_RECT: [f32; 4] = [72.0, 8.0, 48.0, 24.0];
pub const SHADER_TOGGLE_RECT: [f32; 4] = [136.0, 8.0, 48.0, 24.0];

pub const CARD_RECT: [f32; 4] = [60.0, 80.0, 420.0, 200.0];
pub const SHADER_BOX_RECT: [f32; 4] = [80.0, 150.0, 60.0, 60.0];
pub const NESTED_BUTTON_RECT: [f32; 4] = [400.0, 160.0, 40.0, 40.0];

pub const BACKGROUND_A: Color = Color(0.10, 0.16, 0.40, 1.0);
pub const BACKGROUND_B: Color = Color(0.55, 0.12, 0.10, 1.0);

const FLAT_COLOR_WGSL: &str = r#"
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

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let base = textureSample(input_texture, input_sampler, input.uv);
    let phase = u[0].x;
    return vec4<f32>(phase, 0.4, 1.0 - phase, 1.0) + base * 0.0;
}
"#;

pub fn shader_phase_color(phase: f32) -> [u8; 4] {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    [channel(phase), channel(0.4), channel(1.0 - phase), 255]
}

fn flat_color_shader(phase: f32) -> RuntimeShader {
    let mut shader = RuntimeShader::new(FLAT_COLOR_WGSL);
    shader.set_float(0, phase);
    shader
}

fn rect_modifier(rect: [f32; 4]) -> Modifier {
    Modifier::empty().offset(rect[0], rect[1]).size(Size {
        width: rect[2],
        height: rect[3],
    })
}

fn tick_color(tick: u32) -> Color {
    let step = (tick % 8) as f32 / 8.0;
    Color(0.2 + step * 0.6, 0.9 - step * 0.5, 0.3, 1.0)
}

/// A glass card holding a runtime-shader child and a nested glass button
/// over a switchable backdrop, with a tick strip that forces one presented
/// frame per click without touching the card.
#[allow(non_snake_case)]
#[composable]
pub fn NestedGlassCacheReproScreen() {
    let shader_phase = cranpose_core::rememberMutableStateOf(|| 0.2f32);
    NestedGlassScreen(shader_phase.as_state(), Some(shader_phase));
}

/// The same card, but the shader phase runs on the frame clock so the
/// runtime shader's uniforms change through draw repasses alone, with no
/// pointer event in between.
#[allow(non_snake_case)]
#[composable]
pub fn NestedGlassAnimatedReproScreen() {
    let infinite = cranpose_animation::prelude::rememberInfiniteTransition("nested-glass-phase");
    let shader_phase = infinite.animateFloat(
        0.0,
        1.0,
        cranpose_animation::prelude::infiniteRepeatable(
            cranpose_animation::prelude::AnimationSpec::linear(1_000),
            cranpose_animation::prelude::RepeatMode::Reverse,
            cranpose_animation::prelude::StartOffset::default(),
        ),
        "phase",
    );
    NestedGlassScreen(shader_phase, None);
}

#[allow(non_snake_case)]
#[composable]
fn NestedGlassScreen(shader_phase: State<f32>, toggled_phase: Option<MutableState<f32>>) {
    let tick = cranpose_core::rememberMutableStateOf(|| 0u32);
    let alternate_background = cranpose_core::rememberMutableStateOf(|| false);
    let background = if alternate_background.get() {
        BACKGROUND_B
    } else {
        BACKGROUND_A
    };
    Box(
        Modifier::empty()
            .size(Size {
                width: SCREEN_WIDTH,
                height: SCREEN_HEIGHT,
            })
            .background(background),
        BoxSpec::default(),
        move || {
            Box(
                rect_modifier(TICK_RECT)
                    .background(tick_color(tick.get()))
                    .clickable(move |_point| tick.set(tick.get().wrapping_add(1))),
                BoxSpec::default(),
                || {},
            );
            Box(
                rect_modifier(BACKGROUND_TOGGLE_RECT)
                    .background(Color(0.9, 0.9, 0.9, 1.0))
                    .clickable(move |_point| alternate_background.set(!alternate_background.get())),
                BoxSpec::default(),
                || {},
            );
            Box(
                rect_modifier(SHADER_TOGGLE_RECT)
                    .background(Color(0.6, 0.6, 0.6, 1.0))
                    .clickable(move |_point| {
                        if let Some(phase) = toggled_phase {
                            phase.set(if phase.get() > 0.5 { 0.2 } else { 0.8 });
                        }
                    }),
                BoxSpec::default(),
                || {},
            );
            Box(
                rect_modifier(CARD_RECT)
                    .backdrop_effect(RenderEffect::blur(10.0))
                    .background(Color(1.0, 1.0, 1.0, 0.18))
                    .rounded_corners(16.0),
                BoxSpec::default(),
                move || {
                    Text(
                        "Nested glass",
                        Modifier::empty().offset(20.0, 20.0),
                        TextStyle::default(),
                    );
                    Box(
                        rect_modifier([
                            SHADER_BOX_RECT[0] - CARD_RECT[0],
                            SHADER_BOX_RECT[1] - CARD_RECT[1],
                            SHADER_BOX_RECT[2],
                            SHADER_BOX_RECT[3],
                        ])
                        .graphics_layer(move || GraphicsLayer {
                            render_effect: Some(RenderEffect::runtime_shader(flat_color_shader(
                                shader_phase.get(),
                            ))),
                            ..Default::default()
                        }),
                        BoxSpec::default(),
                        || {},
                    );
                    Box(
                        rect_modifier([
                            NESTED_BUTTON_RECT[0] - CARD_RECT[0],
                            NESTED_BUTTON_RECT[1] - CARD_RECT[1],
                            NESTED_BUTTON_RECT[2],
                            NESTED_BUTTON_RECT[3],
                        ])
                        .backdrop_effect(RenderEffect::blur(6.0))
                        .background(Color(1.0, 1.0, 1.0, 0.22))
                        .rounded_corners(20.0),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}
