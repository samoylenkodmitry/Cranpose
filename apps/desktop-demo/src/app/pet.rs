use std::sync::{Arc, OnceLock};

use cranpose::{liquid::prelude::*, rememberWindowStateAt, WindowConfig, WindowModifierExt};
use cranpose_animation::{
    animateFloatAsState, infiniteRepeatable, rememberInfiniteTransition, AnimationSpec,
    AnimationType, RepeatMode, StartOffset,
};
use cranpose_ui::{
    composable, Alignment, Box, BoxSpec, Color, Column, ColumnSpec, GraphicsLayer,
    HorizontalAlignment, Modifier, Text, VerticalAlignment,
};
use cranpose_ui_graphics::{
    CompositingStrategy, RenderEffect, RuntimeShader, RUNTIME_SHADER_PRELUDE_WGSL,
};

use super::{
    chrome_tabs::{label_style, INK},
    demo_trace::trace,
    flame_window::style_after_clicks,
    floating_input::{rememberFloatingInput, FloatingInputModifierExt},
    shader_rect::{FIRE_FIELD_WGSL, WGSL_HELPERS},
};

pub const PET_WIDTH: f32 = 330.0;
pub const PET_HEIGHT: f32 = 400.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Prompt,
    Happy,
    Sleepy,
}

impl Mood {
    pub fn after_clicks(clicks: u32) -> Self {
        match clicks % 3 {
            0 => Mood::Prompt,
            1 => Mood::Happy,
            _ => Mood::Sleepy,
        }
    }

    pub fn caption(self) -> &'static str {
        match self {
            Mood::Prompt => "click me, drag me",
            Mood::Happy => "a window, no chrome",
            Mood::Sleepy => "zzz… floating on top",
        }
    }

    fn shader_code(self) -> f32 {
        match self {
            Mood::Prompt => 0.0,
            Mood::Happy => 1.0,
            Mood::Sleepy => 2.0,
        }
    }
}

#[composable]
pub fn pet_app() {
    let state = rememberWindowStateAt(160.0, 140.0, PET_WIDTH, PET_HEIGHT);
    Box(
        Modifier::empty().window(
            WindowConfig::borderless_for_state("Cranpose Pet", state)
                .with_transparent(true)
                .with_shadow(false)
                .with_always_on_top(true),
        ),
        BoxSpec::default(),
        move || LiquidTheme(LiquidThemeSpec::default(), Pet),
    );
}

#[composable]
fn Pet() {
    let clock = rememberInfiniteTransition("pet_clock");
    let time = clock.animateFloat(
        0.0,
        1.0,
        infiniteRepeatable(
            AnimationSpec::linear(CLOCK_PERIOD_MS),
            RepeatMode::Restart,
            StartOffset::default(),
        ),
        "pet_time",
    );
    let input = rememberFloatingInput();
    let mood = Mood::after_clicks(input.clicks());
    let fire = style_after_clicks(input.clicks());
    trace!(
        "pet mood={mood:?} fire={} pressed={} hovered={}",
        fire.label,
        input.pressed(),
        input.hovered()
    );
    let squash = animateFloatAsState(
        if input.pressed() { 1.0 } else { 0.0 },
        AnimationType::default(),
        "pet_squash",
    );
    let glow = animateFloatAsState(
        if input.hovered() { 1.0 } else { 0.0 },
        AnimationType::default(),
        "pet_glow",
    );
    Box(
        Modifier::empty()
            .size_points(PET_WIDTH, PET_HEIGHT)
            .floating_input(input)
            .graphics_layer(move || GraphicsLayer {
                render_effect: Some(pet_effect(&PetParams {
                    time: time.get() * CLOCK_PERIOD_MS as f32 / 1000.0,
                    squash: squash.get(),
                    glow: glow.get(),
                    mood: mood.shader_code(),
                    tint: fire.color_tint,
                    smoke_blue_tint: fire.smoke_blue_tint,
                })),
                compositing_strategy: CompositingStrategy::Offscreen,
                ..Default::default()
            }),
        BoxSpec::new().content_alignment(Alignment::new(
            HorizontalAlignment::CenterHorizontally,
            VerticalAlignment::Bottom,
        )),
        move || {
            Box(
                Modifier::empty().padding(CAPTION_BOTTOM).glass_effect(
                    Glass::regular()
                        .shape(LiquidShape::Capsule)
                        .tint(Color(0.35, 0.55, 1.0, 0.30)),
                ),
                BoxSpec::default(),
                move || {
                    Column(
                        Modifier::empty().padding(9.0),
                        ColumnSpec {
                            horizontal_alignment: HorizontalAlignment::CenterHorizontally,
                            ..ColumnSpec::default()
                        },
                        move || {
                            Text(mood.caption(), Modifier::empty(), label_style(12.0, INK));
                            Text(
                                fire.label,
                                Modifier::empty(),
                                label_style(11.0, Color(1.0, 1.0, 1.0, 0.62)),
                            );
                        },
                    );
                },
            );
        },
    );
}

struct PetParams {
    time: f32,
    squash: f32,
    glow: f32,
    mood: f32,
    tint: [f32; 3],
    smoke_blue_tint: f32,
}

fn pet_effect(p: &PetParams) -> RenderEffect {
    let mut shader = RuntimeShader::from_shared_source(pet_wgsl());
    shader.set_float2(0, PET_WIDTH, PET_HEIGHT);
    shader.set_float(2, p.time);
    shader.set_float(3, p.squash);
    shader.set_float(4, p.glow);
    shader.set_float(5, p.mood);
    shader.set_float(6, p.tint[0]);
    shader.set_float(7, p.tint[1]);
    shader.set_float(8, p.tint[2]);
    shader.set_float(9, p.smoke_blue_tint);
    RenderEffect::runtime_shader(shader)
}

fn pet_wgsl() -> Arc<str> {
    static SOURCE: OnceLock<Arc<str>> = OnceLock::new();
    SOURCE
        .get_or_init(|| {
            Arc::<str>::from(format!(
                "{RUNTIME_SHADER_PRELUDE_WGSL}{WGSL_HELPERS}{FIRE_FIELD_WGSL}{PET_FRAGMENT_WGSL}"
            ))
        })
        .clone()
}

const CLOCK_PERIOD_MS: u64 = 120_000;
const CAPTION_BOTTOM: f32 = 14.0;

const PET_FRAGMENT_WGSL: &str = r"

const BODY_SCALE: f32 = 1.55;
const FLAME_THICKNESS: f32 = 0.014;
const GLOW_SPREAD: f32 = 8.0;
const HALO_FALLOFF: f32 = 0.30;
const EDGE_FADE_PX: f32 = 26.0;
fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

fn sd_segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let from_a = p - a;
    let span = b - a;
    let h = clamp(dot(from_a, span) / dot(span, span), 0.0, 1.0);
    return length(from_a - span * h);
}

fn stroke(d: f32, w: f32, aa: f32) -> f32 {
    return 1.0 - smoothstep(w - aa, w + aa, d);
}

fn blink_f(t: f32) -> f32 {
    let cycle = fract(t / 3.7);
    let closed = smoothstep(0.90, 0.93, cycle) * (1.0 - smoothstep(0.96, 0.99, cycle));
    return 1.0 - closed;
}

fn caret(p: vec2<f32>, at: vec2<f32>, w: f32, aa: f32) -> f32 {
    let q = p - at;
    let d = min(
        sd_segment(q, vec2<f32>(-0.03, 0.02), vec2<f32>(0.0, -0.015)),
        sd_segment(q, vec2<f32>(0.0, -0.015), vec2<f32>(0.03, 0.02))
    );
    return stroke(d, w, aa);
}

fn prompt_glyph(p: vec2<f32>, t: f32, aa: f32) -> f32 {
    let w = 0.011;
    let chevron = min(
        sd_segment(p, vec2<f32>(-0.075, -0.035), vec2<f32>(-0.035, 0.0)),
        sd_segment(p, vec2<f32>(-0.035, 0.0), vec2<f32>(-0.075, 0.035))
    );
    let cursor_on = step(fract(t * 1.1), 0.62);
    let cursor = sd_segment(p, vec2<f32>(0.005, 0.03), vec2<f32>(0.06, 0.03));
    return max(stroke(chevron, w, aa), stroke(cursor, w, aa) * cursor_on);
}

fn happy_glyph(p: vec2<f32>, aa: f32) -> f32 {
    return max(
        caret(p, vec2<f32>(-0.05, -0.005), 0.011, aa),
        caret(p, vec2<f32>(0.05, -0.005), 0.011, aa)
    );
}

fn sleepy_glyph(p: vec2<f32>, aa: f32) -> f32 {
    let left = sd_segment(p, vec2<f32>(-0.08, 0.0), vec2<f32>(-0.025, 0.0));
    let right = sd_segment(p, vec2<f32>(0.025, 0.0), vec2<f32>(0.08, 0.0));
    return stroke(min(left, right), 0.011, aa);
}

fn body_sdf(q: vec2<f32>) -> f32 {
    var d = length(q - vec2<f32>(0.0, -0.13)) - 0.175;
    d = smin(d, length(q - vec2<f32>(-0.14, -0.06)) - 0.11, 0.05);
    d = smin(d, length(q - vec2<f32>(0.14, -0.06)) - 0.11, 0.05);
    d = smin(d, length(q - vec2<f32>(0.0, -0.27)) - 0.10, 0.05);
    d = smin(d, sd_round_box(q - vec2<f32>(0.0, 0.15), vec2<f32>(0.12, 0.11), 0.07), 0.04);
    d = smin(d, length(q - vec2<f32>(-0.185, 0.12)) - 0.045, 0.03);
    d = smin(d, length(q - vec2<f32>(0.185, 0.12)) - 0.045, 0.03);
    d = smin(d, length(q - vec2<f32>(-0.07, 0.30)) - 0.05, 0.03);
    d = smin(d, length(q - vec2<f32>(0.07, 0.30)) - 0.05, 0.03);
    return d;
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = input.uv;
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let effect_rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let size_px = max(effect_rect.zw, vec2<f32>(1.0));
    let local_px = uv * tex_size - effect_rect.xy;
    let t = get_float(2u);
    let squash = get_float(3u);
    let glow = get_float(4u);
    let mood = get_float(5u);
    let flame_tint = vec3<f32>(get_float(6u), get_float(7u), get_float(8u));
    let smoke_blue_tint = clamp(get_float(9u), 0.0, 1.0);

    let q0 = (local_px - size_px * 0.5) / size_px.y * BODY_SCALE;
    let aa = 1.5 / size_px.y;
    let bob = sin(t * 2.2) * 0.012;
    let feet_y = 0.34;
    var q = q0;
    q.y = (q.y - feet_y) / (1.0 - 0.14 * squash) + feet_y - bob;
    q.x = q.x / (1.0 + 0.10 * squash);

    let d = body_sdf(q);
    let body_a = 1.0 - smoothstep(-aa, aa, d);
    let light = clamp(0.55 + 0.6 * (0.35 - q.y), 0.35, 1.15);
    var body_rgb = vec3<f32>(0.24, 0.40, 0.98) * light;
    body_rgb = mix(body_rgb, vec3<f32>(0.10, 0.16, 0.55), smoothstep(-0.03, 0.0, d));

    let visor = sd_round_box(q - vec2<f32>(0.0, -0.12), vec2<f32>(0.125, 0.085), 0.035);
    let visor_a = 1.0 - smoothstep(-aa, aa, visor);
    body_rgb = mix(body_rgb, vec3<f32>(0.06, 0.07, 0.10), visor_a);

    let blink = blink_f(t);
    var face = q - vec2<f32>(0.0, -0.12);
    face.y = face.y / max(blink, 0.05);
    var glyph = 0.0;
    if (mood < 0.5) {
        glyph = prompt_glyph(face, t, aa);
    } else if (mood < 1.5) {
        glyph = happy_glyph(face, aa);
    } else {
        glyph = sleepy_glyph(face, aa);
    }
    glyph = glyph * visor_a;
    let ink = vec3<f32>(0.45, 0.98, 0.90) * (1.0 + 0.4 * glow);
    body_rgb = mix(body_rgb, ink, glyph);

    let out_d = max(d, 0.0);
    let dir = q - vec2<f32>(0.0, -0.05);
    let u_coord = fract(atan2(dir.y, dir.x) / TWO_PI + 0.5);
    let v_coord = 0.5 + d / (FLAME_THICKNESS * 2.0);
    let ff = smoothstep(-0.15, 0.25, -q.y);
    let a1 = color_from_grad(shade(vec2<f32>(u_coord + 1.30, v_coord), t)) * ff;
    let a2 = color_from_grad(shade(vec2<f32>(u_coord + 1.90, 1.0 - v_coord), t)) * (1.0 - ff);
    let u_b = u_coord + 1.0;
    let b1 = color_from_grad(shade(vec2<f32>(u_b + 1.30, v_coord), t)) * ff;
    let b2 = color_from_grad(shade(vec2<f32>(u_b + 1.90, 1.0 - v_coord), t)) * (1.0 - ff);
    let seam = smoothstep(0.0, 0.04, u_coord) * smoothstep(0.0, 0.04, 1.0 - u_coord);
    let blaze = 1.0 + 1.4 * glow;
    let flame = mix(b1 + b2, a1 + a2, seam) * blaze;

    let core_mask = 1.0 - smoothstep(FLAME_THICKNESS, FLAME_THICKNESS * 2.0, abs(d));
    let smoke_w = FLAME_THICKNESS * 4.8;
    let smoke_mask = 1.0 - smoothstep(smoke_w, smoke_w * 3.2, abs(d));
    let glow_mask = 1.0 - smoothstep(FLAME_THICKNESS, FLAME_THICKNESS * GLOW_SPREAD, abs(d));

    var col = flame * core_mask;
    let smoke_tinted = mix(
        flame,
        flame * vec3<f32>(0.50, 0.72, 1.35) + vec3<f32>(0.00, 0.02, 0.10),
        smoke_blue_tint
    );
    col = col + smoke_tinted * 0.55 * smoke_mask;
    col = col * smoke_mask * flame_tint;

    var halo_a = clamp(max(max(col.r, col.g), col.b), 0.0, 1.0);
    halo_a = max(halo_a * 0.95, glow_mask * 0.35);
    let reach = 1.0 - smoothstep(0.0, HALO_FALLOFF, out_d);
    let to_edge = min(min(local_px.x, size_px.x - local_px.x), min(local_px.y, size_px.y - local_px.y));
    let fade = reach * reach * smoothstep(0.0, EDGE_FADE_PX, to_edge) * (1.0 - body_a);
    var rgb = col * fade;
    var a = halo_a * fade;

    let shadow_p = (q0 - vec2<f32>(0.0, 0.40)) / vec2<f32>(0.16 - bob * 2.0, 0.035);
    let shadow_a = (1.0 - smoothstep(0.7, 1.0, length(shadow_p))) * 0.35 * (1.0 - body_a);
    rgb = rgb * (1.0 - shadow_a);
    a = a * (1.0 - shadow_a) + shadow_a;

    rgb = rgb * (1.0 - body_a) + body_rgb * body_a;
    a = a * (1.0 - body_a) + body_a;

    let base = textureSample(input_texture, input_sampler, uv);
    let out_a = base.a + a * (1.0 - base.a);
    let out_rgb = base.rgb + rgb * (1.0 - base.a);
    return vec4<f32>(out_rgb, out_a);
}
";

#[cfg(test)]
#[path = "tests/pet_tests.rs"]
mod tests;
