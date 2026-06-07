//! Shader Rect demo tab — SDF-based shader effects ported from Android Compose Playground.
//!
//! Features:
//! - Fire shader with multiple style variations (classic, blue ice, emerald, thin neon)
//! - Simple halo border with press-to-intensify
//! - Hover reactivity: flame intensity and smoke respond to mouse proximity

#![allow(non_snake_case)]

use cranpose_animation::{
    animateFloatAsState, infiniteRepeatable, rememberInfiniteTransition, AnimationSpec,
    AnimationType, RepeatMode, StartOffset,
};
use cranpose_ui::{
    composable,
    text::{SpanStyle, TextUnit},
    Alignment, Box, BoxSpec, Brush, Color, Column, ColumnSpec, CornerRadii, GraphicsLayer,
    LinearArrangement, Modifier, PointerEventKind, PointerInputScope, Row, RowSpec, Text,
    TextStyle,
};
use cranpose_ui_graphics::{CompositingStrategy, RenderEffect, RuntimeShader};
use std::sync::{Arc, OnceLock};

// ---------------------------------------------------------------------------
// Shared WGSL boilerplate
// ---------------------------------------------------------------------------

const WGSL_PREAMBLE: &str = r#"
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

fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn get_vec2(index: u32) -> vec2<f32> {
    return vec2<f32>(get_float(index), get_float(index + 1u));
}

fn sd_round_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}
"#;

// ---------------------------------------------------------------------------
// Fire shader WGSL
// ---------------------------------------------------------------------------
// Uniforms:
//   0,1   = resolution (dp)        2     = time [0..1]
//   3     = band width (dp)        4     = corner radius (dp)
//   5,6   = contour size (dp)      7     = smoke scale
//   8     = intensity              9     = smoke opacity
//   10    = core scale             11    = smoke blue tint [0..1]
//   12    = thin mode [0..1]       13-15 = color tint RGB

fn fire_halo_wgsl() -> Arc<str> {
    static SOURCE: OnceLock<Arc<str>> = OnceLock::new();
    SOURCE
        .get_or_init(|| {
            Arc::<str>::from(format!(
        r#"{preamble}

const PI: f32 = 3.14159265358979;
const TWO_PI: f32 = 6.28318530717959;

fn rand_f(n: vec2<f32>) -> f32 {{
    return fract(sin(dot(n, vec2<f32>(12.9898, 12.1414))) * 83758.5453);
}}

fn noise_f(n: vec2<f32>) -> f32 {{
    let b = floor(n);
    let f = fract(n);
    return mix(
        mix(rand_f(b), rand_f(b + vec2<f32>(1.0, 0.0)), f.x),
        mix(rand_f(b + vec2<f32>(0.0, 1.0)), rand_f(b + vec2<f32>(1.0, 1.0)), f.x),
        f.y
    );
}}

fn fire_f(n: vec2<f32>) -> f32 {{
    return noise_f(n) + noise_f(n * 2.1) * 0.6 + noise_f(n * 5.4) * 0.42;
}}

fn ramp(t_in: f32) -> vec3<f32> {{
    let t = max(t_in, 0.001);
    if (t <= 0.5) {{
        return vec3<f32>(1.0 - t * 1.4, 0.2, 1.05) / t;
    }}
    return vec3<f32>(0.3 * (1.0 - t) * 2.0, 0.2, 1.05) / t;
}}

fn shade(uv_in: vec2<f32>, t: f32) -> f32 {{
    var uv = uv_in;
    if (uv.y < 0.5) {{
        uv.x = uv.x + 23.0 + t * 0.035;
    }} else {{
        uv.x = uv.x - 11.0 + t * 0.03;
    }}
    uv.y = abs(uv.y - 0.5);
    uv.x = uv.x * 35.0;

    let q = fire_f(uv - t * 0.013) / 2.0;
    let rv = vec2<f32>(
        fire_f(uv + q / 2.0 + t - uv.x - uv.y),
        fire_f(uv + q - t)
    );
    return pow((rv.y + rv.y) * max(0.0, uv.y) + 0.1, 4.0);
}}

fn color_from_grad(grad: f32) -> vec3<f32> {{
    let g = sqrt(max(grad, 0.0));
    let c = ramp(g);
    return c / (vec3<f32>(1.15) + max(vec3<f32>(0.0), c));
}}

fn perimeter_s(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {{
    let inner = max(half_size - vec2<f32>(r), vec2<f32>(0.0001));
    let lh = 2.0 * inner.x;
    let lv = 2.0 * inner.y;
    let lc = 0.5 * PI * r;
    let a = abs(p);
    let is_corner = (a.x > inner.x) && (a.y > inner.y);

    if (!is_corner) {{
        if (a.x > inner.x) {{
            if (p.x >= 0.0) {{ return clamp(p.y + inner.y, 0.0, lv); }}
            else {{ return lv + lc + lh + lc + clamp(inner.y - p.y, 0.0, lv); }}
        }}
        if (a.y > inner.y) {{
            if (p.y >= 0.0) {{ return lv + lc + clamp(inner.x - p.x, 0.0, lh); }}
            else {{ return lv + lc + lh + lc + lv + lc + clamp(p.x + inner.x, 0.0, lh); }}
        }}
        return clamp(p.y + inner.y, 0.0, lv);
    }}

    if (p.x >= 0.0 && p.y >= 0.0) {{
        let c = vec2<f32>(inner.x, inner.y);
        let v = (p - c) / r;
        return lv + clamp(atan2(v.y, v.x), 0.0, 0.5 * PI) * r;
    }}
    if (p.x <= 0.0 && p.y >= 0.0) {{
        let c = vec2<f32>(-inner.x, inner.y);
        let v = (p - c) / r;
        return lv + lc + lh + (clamp(atan2(v.y, v.x), 0.5 * PI, PI) - 0.5 * PI) * r;
    }}
    if (p.x <= 0.0 && p.y <= 0.0) {{
        let c = vec2<f32>(-inner.x, -inner.y);
        let v = (p - c) / r;
        var ang = atan2(v.y, v.x);
        if (ang < 0.0) {{ ang = ang + TWO_PI; }}
        ang = clamp(ang, PI, PI + 0.5 * PI);
        return lv + lc + lh + lc + lv + (ang - PI) * r;
    }}
    let c = vec2<f32>(inner.x, -inner.y);
    let v = (p - c) / r;
    let base = lv + lc + lh + lc + lv + lc + lh;
    return base + (clamp(atan2(v.y, v.x), -0.5 * PI, 0.0) + 0.5 * PI) * r;
}}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
    let uv_screen = input.uv;
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let effect_rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let resolution = get_vec2(0u);
    let dp_scale = effect_rect.zw / max(resolution, vec2<f32>(1.0));
    let s = min(dp_scale.x, dp_scale.y);
    let local_px = uv_screen * tex_size - effect_rect.xy;

    let time_raw = get_float(2u);
    let band_px = get_float(3u) * s;
    let corner_raw = get_float(4u) * s;
    let contour_size = vec2<f32>(get_float(5u) * dp_scale.x, get_float(6u) * dp_scale.y);
    let smoke_scale = get_float(7u);
    let intensity = get_float(8u);
    let smoke_opacity = get_float(9u);
    let core_scale = get_float(10u);
    let smoke_blue_tint = clamp(get_float(11u), 0.0, 1.0);
    let thin_mode = clamp(get_float(12u), 0.0, 1.0);

    let size_px = resolution * dp_scale;
    let res = max(size_px, vec2<f32>(1.0));
    let t = time_raw * 60.0;
    let p = local_px - 0.5 * res;
    let p_norm = p / max(res.y, 1.0);

    let min_thickness = mix(1.5, 0.55, thin_mode);
    let thickness = max(min_thickness, band_px * mix(0.30, 0.16, thin_mode));
    let smoke_w = max(thickness * 4.8, band_px * 2.2) * max(smoke_scale, 0.3) * mix(1.0, 0.30, thin_mode);

    let half_size = contour_size * 0.5;
    var radius = min(corner_raw, min(half_size.x, half_size.y) - 0.1);
    radius = max(radius, 1.0);

    let d = sd_round_box(p, half_size, radius);

    let inner = max(half_size - vec2<f32>(radius), vec2<f32>(0.0001));
    let lh = 2.0 * inner.x;
    let lv = 2.0 * inner.y;
    let perimeter = 2.0 * (lh + lv) + TWO_PI * radius;

    let s_param = perimeter_s(p, half_size, radius);
    let u_coord = fract(s_param / perimeter);
    let v_coord = 0.5 + d / (thickness * 2.0);

    let core_mask = 1.0 - smoothstep(thickness, thickness * 2.0, abs(d));
    let smoke_mask = 1.0 - smoothstep(smoke_w, smoke_w * 3.2, abs(d));
    let ff = smoothstep(-0.15, 0.25, -p_norm.y);

    let overlap_px = 10.0;
    let seam_width = clamp(overlap_px / max(perimeter, 1.0), 0.001, 0.20);
    let seam_blend = smoothstep(0.0, seam_width, u_coord) * smoothstep(0.0, seam_width, 1.0 - u_coord);

    let uv_a = vec2<f32>(u_coord + 1.30, v_coord);
    let uv_a2 = vec2<f32>(u_coord + 1.90, 1.0 - v_coord);
    let a1 = color_from_grad(shade(uv_a, t)) * ff;
    let a2 = color_from_grad(shade(uv_a2, t)) * (1.0 - ff);
    let flame_a = a1 + a2;

    let u_b = u_coord + 1.0;
    let uv_b = vec2<f32>(u_b + 1.30, v_coord);
    let uv_b2 = vec2<f32>(u_b + 1.90, 1.0 - v_coord);
    let b1 = color_from_grad(shade(uv_b, t)) * ff;
    let b2 = color_from_grad(shade(uv_b2, t)) * (1.0 - ff);
    let flame_b = b1 + b2;

    let flame = mix(flame_b, flame_a, seam_blend) * intensity;
    let smoke_tinted = mix(
        flame,
        flame * vec3<f32>(0.50, 0.72, 1.35) + vec3<f32>(0.00, 0.02, 0.10),
        smoke_blue_tint
    );

    var col = flame * core_mask * max(core_scale, 0.0);
    col = col + smoke_tinted * (0.55 * max(smoke_opacity, 0.0)) * smoke_mask;
    col = col * smoke_mask;

    let crisp_edge = smoothstep(thickness * 0.85, thickness * 0.10, abs(d));
    col = col + flame * crisp_edge * thin_mode * 0.55;

    let tint = vec3<f32>(get_float(13u), get_float(14u), get_float(15u));
    col = col * tint;

    var alpha = clamp(max(max(col.r, col.g), col.b), 0.0, 1.0);
    alpha = max(alpha * 0.95, core_mask * 0.35);
    let halo = vec4<f32>(col, alpha);

    let base = textureSample(input_texture, input_sampler, uv_screen);
    let out_a = base.a + halo.a * (1.0 - base.a);
    let out_rgb = base.rgb + halo.rgb * (1.0 - base.a);
    return vec4<f32>(out_rgb, out_a);
}}
"#,
        preamble = WGSL_PREAMBLE
            ))
        })
        .clone()
}

// ---------------------------------------------------------------------------
// Halo border shader WGSL
// ---------------------------------------------------------------------------

fn halo_border_wgsl() -> Arc<str> {
    static SOURCE: OnceLock<Arc<str>> = OnceLock::new();
    SOURCE
        .get_or_init(|| {
            Arc::<str>::from(format!(
        r#"{preamble}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
    let uv = input.uv;
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let effect_rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let resolution = get_vec2(0u);
    let dp_scale = effect_rect.zw / max(resolution, vec2<f32>(1.0));
    let s = min(dp_scale.x, dp_scale.y);
    let local_px = uv * tex_size - effect_rect.xy;

    let corner_px = get_float(2u) * s;
    let core_w = max(get_float(3u) * s, 0.001);
    let idle_fade_end = max(get_float(4u) * s, core_w + 0.001);
    let max_halo_w = max(get_float(5u) * s, 0.001);
    let press = clamp(get_float(6u), 0.0, 1.0);
    let intensity = get_float(7u);
    let color = vec4<f32>(get_float(8u), get_float(9u), get_float(10u), get_float(11u));

    let size_px = resolution * dp_scale;
    let p = local_px - size_px * 0.5;
    let half_size = size_px * 0.5 - vec2<f32>(max_halo_w + 1.0);
    let radius = clamp(corner_px, 0.0, min(half_size.x, half_size.y) - 1.0);
    let signed_d = sd_round_box(p, half_size, radius);
    let d = max(signed_d, 0.0);
    let outside_mask = step(0.0, signed_d);

    var idle_band = 0.0;
    if (d <= core_w) {{ idle_band = 0.5; }}
    else if (d <= idle_fade_end) {{
        idle_band = mix(0.5, 0.0, (d - core_w) / max(idle_fade_end - core_w, 0.001));
    }}

    let press_drop_end = core_w * 3.0;
    let press_tail_end = max(max_halo_w, press_drop_end + 0.001);
    var press_band = 0.0;
    if (d <= core_w) {{ press_band = 1.0; }}
    else if (d <= press_drop_end) {{
        press_band = mix(1.0, 0.5, (d - core_w) / max(press_drop_end - core_w, 0.001));
    }} else {{
        press_band = mix(0.5, 0.0, clamp((d - press_drop_end) / max(press_tail_end - press_drop_end, 0.001), 0.0, 1.0));
    }}

    let band = mix(idle_band, press_band, press) * outside_mask;
    let a = clamp(band, 0.0, 1.0);
    let rgb = color.rgb * (a * intensity);
    let halo = vec4<f32>(rgb, a * color.a);
    let base = textureSample(input_texture, input_sampler, uv);
    let out_a = base.a + halo.a * (1.0 - base.a);
    let out_rgb = base.rgb + halo.rgb * (1.0 - base.a);
    return vec4<f32>(out_rgb, out_a);
}}
"#,
        preamble = WGSL_PREAMBLE
            ))
        })
        .clone()
}

// ---------------------------------------------------------------------------
// Effect builders
// ---------------------------------------------------------------------------

struct FireShaderParams {
    resolution_w: f32,
    resolution_h: f32,
    time: f32,
    band_width: f32,
    corner_radius: f32,
    contour_w: f32,
    contour_h: f32,
    smoke_scale: f32,
    intensity: f32,
    smoke_opacity: f32,
    core_scale: f32,
    smoke_blue_tint: f32,
    thin_mode: f32,
    color_tint: [f32; 3],
}

fn fire_shader_effect(p: &FireShaderParams) -> RenderEffect {
    let mut shader = RuntimeShader::from_shared_source(fire_halo_wgsl());
    shader.set_float2(0, p.resolution_w, p.resolution_h);
    shader.set_float(2, p.time);
    shader.set_float(3, p.band_width);
    shader.set_float(4, p.corner_radius);
    shader.set_float2(5, p.contour_w, p.contour_h);
    shader.set_float(7, p.smoke_scale);
    shader.set_float(8, p.intensity);
    shader.set_float(9, p.smoke_opacity);
    shader.set_float(10, p.core_scale);
    shader.set_float(11, p.smoke_blue_tint);
    shader.set_float(12, p.thin_mode);
    shader.set_float(13, p.color_tint[0]);
    shader.set_float(14, p.color_tint[1]);
    shader.set_float(15, p.color_tint[2]);
    RenderEffect::runtime_shader(shader)
}

struct HaloBorderParams {
    width: f32,
    height: f32,
    corner_radius: f32,
    stroke_width: f32,
    idle_fade_end: f32,
    max_halo_border_width: f32,
    press: f32,
    intensity: f32,
    color: Color,
}

fn halo_border_effect(p: &HaloBorderParams) -> RenderEffect {
    let mut shader = RuntimeShader::from_shared_source(halo_border_wgsl());
    shader.set_float2(0, p.width, p.height);
    shader.set_float(2, p.corner_radius);
    shader.set_float(3, p.stroke_width);
    shader.set_float(4, p.idle_fade_end);
    shader.set_float(5, p.max_halo_border_width);
    shader.set_float(6, p.press);
    shader.set_float(7, p.intensity);
    shader.set_float4(8, p.color.0, p.color.1, p.color.2, p.color.3);
    RenderEffect::runtime_shader(shader)
}

// ---------------------------------------------------------------------------
// Fire style presets
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
struct FireStyle {
    label: &'static str,
    band_width: f32,
    corner_radius: f32,
    smoke_blue_tint: f32,
    thin_mode: f32,
    base_intensity: f32,
    base_smoke_scale: f32,
    base_smoke_opacity: f32,
    base_core_scale: f32,
    bg_color: Color,
    color_tint: [f32; 3],
    hover_intensity_mult: f32,
    hover_smoke_mult: f32,
    press_intensity_mult: f32,
    press_smoke_mult: f32,
    press_core_mult: f32,
}

const FIRE_CLASSIC: FireStyle = FireStyle {
    label: "Classic Fire",
    band_width: 14.0,
    corner_radius: 24.0,
    smoke_blue_tint: 0.0,
    thin_mode: 0.0,
    base_intensity: 1.0,
    base_smoke_scale: 1.0,
    base_smoke_opacity: 1.0,
    base_core_scale: 1.0,
    bg_color: Color(0.08, 0.05, 0.03, 1.0),
    color_tint: [1.3, 0.7, 0.25],
    hover_intensity_mult: 1.8,
    hover_smoke_mult: 1.6,
    press_intensity_mult: 2.8,
    press_smoke_mult: 2.5,
    press_core_mult: 1.5,
};

const FIRE_BLUE_ICE: FireStyle = FireStyle {
    label: "Blue Ice",
    band_width: 18.0,
    corner_radius: 32.0,
    smoke_blue_tint: 0.85,
    thin_mode: 0.0,
    base_intensity: 0.9,
    base_smoke_scale: 1.3,
    base_smoke_opacity: 1.4,
    base_core_scale: 0.8,
    bg_color: Color(0.03, 0.05, 0.10, 1.0),
    color_tint: [0.35, 0.65, 1.4],
    hover_intensity_mult: 1.6,
    hover_smoke_mult: 2.0,
    press_intensity_mult: 2.2,
    press_smoke_mult: 3.0,
    press_core_mult: 1.3,
};

const FIRE_EMERALD: FireStyle = FireStyle {
    label: "Emerald",
    band_width: 12.0,
    corner_radius: 16.0,
    smoke_blue_tint: 0.0,
    thin_mode: 0.0,
    base_intensity: 1.1,
    base_smoke_scale: 0.9,
    base_smoke_opacity: 1.2,
    base_core_scale: 1.1,
    bg_color: Color(0.03, 0.07, 0.05, 1.0),
    color_tint: [0.3, 1.3, 0.45],
    hover_intensity_mult: 1.5,
    hover_smoke_mult: 1.4,
    press_intensity_mult: 2.5,
    press_smoke_mult: 2.0,
    press_core_mult: 1.4,
};

const FIRE_NEON_THIN: FireStyle = FireStyle {
    label: "Neon Wire",
    band_width: 20.0,
    corner_radius: 28.0,
    smoke_blue_tint: 0.0,
    thin_mode: 1.0,
    base_intensity: 1.4,
    base_smoke_scale: 0.5,
    base_smoke_opacity: 0.6,
    base_core_scale: 1.6,
    bg_color: Color(0.04, 0.03, 0.06, 1.0),
    color_tint: [1.3, 0.3, 1.1],
    hover_intensity_mult: 2.0,
    hover_smoke_mult: 2.5,
    press_intensity_mult: 3.2,
    press_smoke_mult: 3.5,
    press_core_mult: 1.8,
};

// ---------------------------------------------------------------------------
// Tab layout
// ---------------------------------------------------------------------------

#[composable]
pub(crate) fn ShaderRectTab() {
    Column(
        Modifier::empty()
            .padding(32.0)
            .background(Color(0.06, 0.07, 0.12, 1.0))
            .fill_max_width(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(20.0)),
        move || {
            SectionHeader("Fire Shader — Animated Flame Border");

            SectionCaption(
                "Procedural fire using multi-octave noise and SDF perimeter mapping. \
                 Hover to intensify, press for full blaze.",
            );

            // Fire variations in rows of two
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default(),
                move || {
                    FireShaderBox(FIRE_CLASSIC);
                    FireShaderBox(FIRE_BLUE_ICE);
                },
            );

            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default(),
                move || {
                    FireShaderBox(FIRE_EMERALD);
                    FireShaderBox(FIRE_NEON_THIN);
                },
            );

            SectionHeader("SDF Halo Border");

            SectionCaption("Press boxes to see the halo glow intensify.");

            HaloBorderBox(Color(0.10, 0.30, 1.00, 1.0), 24.0, 32.0, "Box 1");
            HaloBorderBox(Color(0.0, 0.85, 0.85, 1.0), 16.0, 48.0, "Box 2");
        },
    );
}

#[composable]
fn SectionHeader(text: &'static str) {
    Text(
        text,
        Modifier::empty(),
        TextStyle {
            span_style: SpanStyle {
                color: Some(Color(1.0, 1.0, 1.0, 0.9)),
                font_size: TextUnit::Sp(22.0),
                ..Default::default()
            },
            ..Default::default()
        },
    );
}

#[composable]
fn SectionCaption(text: &'static str) {
    Text(
        text,
        Modifier::empty(),
        TextStyle {
            span_style: SpanStyle {
                color: Some(Color(1.0, 1.0, 1.0, 0.45)),
                font_size: TextUnit::Sp(13.0),
                ..Default::default()
            },
            ..Default::default()
        },
    );
}

// ---------------------------------------------------------------------------
// Fire shader box with hover + press reactivity
// ---------------------------------------------------------------------------

#[composable]
fn FireShaderBox(style: FireStyle) {
    let content_width = 220.0f32;
    let content_height = 100.0f32;
    let padding = 44.0f32;

    let outer_width = content_width + 2.0 * padding;
    let outer_height = content_height + 2.0 * padding;

    let transition = rememberInfiniteTransition("fire_time");
    let time = transition.animateFloat(
        0.0,
        1.0,
        infiniteRepeatable(
            AnimationSpec::linear(60_000),
            RepeatMode::Restart,
            StartOffset::default(),
        ),
        "fire_t",
    );

    let is_pressed = cranpose_core::useState(|| false);
    let is_hovered = cranpose_core::useState(|| false);

    // Hover smoothly boosts intensity/smoke; press goes further
    let target_intensity = if is_pressed.get() {
        style.base_intensity * style.press_intensity_mult
    } else if is_hovered.get() {
        style.base_intensity * style.hover_intensity_mult
    } else {
        style.base_intensity
    };

    let target_smoke = if is_pressed.get() {
        style.base_smoke_scale * style.press_smoke_mult
    } else if is_hovered.get() {
        style.base_smoke_scale * style.hover_smoke_mult
    } else {
        style.base_smoke_scale
    };

    let target_core = if is_pressed.get() {
        style.base_core_scale * style.press_core_mult
    } else {
        style.base_core_scale
    };

    let intensity_anim =
        animateFloatAsState(target_intensity, AnimationType::default(), "fire_intensity");
    let smoke_anim = animateFloatAsState(target_smoke, AnimationType::default(), "fire_smoke");
    let core_anim = animateFloatAsState(target_core, AnimationType::default(), "fire_core");

    Box(
        Modifier::empty()
            .size_points(outer_width, outer_height)
            .graphics_layer(move || {
                let effect = fire_shader_effect(&FireShaderParams {
                    resolution_w: outer_width,
                    resolution_h: outer_height,
                    time: time.get(),
                    band_width: style.band_width,
                    corner_radius: style.corner_radius,
                    contour_w: content_width,
                    contour_h: content_height,
                    smoke_scale: smoke_anim.get(),
                    intensity: intensity_anim.get(),
                    smoke_opacity: style.base_smoke_opacity,
                    core_scale: core_anim.get(),
                    smoke_blue_tint: style.smoke_blue_tint,
                    thin_mode: style.thin_mode,
                    color_tint: style.color_tint,
                });
                GraphicsLayer {
                    render_effect: Some(effect),
                    compositing_strategy: CompositingStrategy::Offscreen,
                    ..Default::default()
                }
            })
            .pointer_input((), {
                move |scope: PointerInputScope| async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                match event.kind {
                                    PointerEventKind::Enter => {
                                        is_hovered.set(true);
                                    }
                                    PointerEventKind::Exit => {
                                        is_hovered.set(false);
                                    }
                                    PointerEventKind::Down => {
                                        is_pressed.set(true);
                                        event.consume();
                                    }
                                    PointerEventKind::Up | PointerEventKind::Cancel => {
                                        is_pressed.set(false);
                                    }
                                    _ => {}
                                }
                            }
                        })
                        .await;
                }
            }),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            Box(
                Modifier::empty()
                    .size_points(content_width, content_height)
                    .draw_behind({
                        let bg = style.bg_color;
                        let cr = style.corner_radius;
                        move |scope| {
                            scope.draw_round_rect(Brush::solid(bg), CornerRadii::uniform(cr));
                        }
                    }),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                move || {
                    Text(
                        style.label,
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(1.0, 1.0, 1.0, 0.85)),
                                font_size: TextUnit::Sp(16.0),
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

// ---------------------------------------------------------------------------
// Halo border box
// ---------------------------------------------------------------------------

#[composable]
fn HaloBorderBox(color: Color, corner_radius: f32, max_halo_width: f32, label: &'static str) {
    let is_pressed = cranpose_core::useState(|| false);

    let target_press = if is_pressed.get() { 1.0f32 } else { 0.0f32 };
    let target_intensity = if is_pressed.get() { 1.6f32 } else { 1.0f32 };

    let press_anim = animateFloatAsState(target_press, AnimationType::default(), "halo_press");
    let intensity_anim =
        animateFloatAsState(target_intensity, AnimationType::default(), "halo_intensity");

    let content_width = 280.0f32;
    let content_height = 120.0f32;

    let outer_width = content_width + 2.0 * max_halo_width;
    let outer_height = content_height + 2.0 * max_halo_width;

    Box(
        Modifier::empty()
            .size_points(outer_width, outer_height)
            .graphics_layer(move || {
                let effect = halo_border_effect(&HaloBorderParams {
                    width: outer_width,
                    height: outer_height,
                    corner_radius,
                    stroke_width: 2.0,
                    idle_fade_end: 8.0,
                    max_halo_border_width: max_halo_width,
                    press: press_anim.get(),
                    intensity: intensity_anim.get(),
                    color,
                });
                GraphicsLayer {
                    render_effect: Some(effect),
                    compositing_strategy: CompositingStrategy::Offscreen,
                    ..Default::default()
                }
            })
            .pointer_input((), {
                move |scope: PointerInputScope| async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                match event.kind {
                                    PointerEventKind::Down => {
                                        is_pressed.set(true);
                                        event.consume();
                                    }
                                    PointerEventKind::Up | PointerEventKind::Cancel => {
                                        is_pressed.set(false);
                                    }
                                    _ => {}
                                }
                            }
                        })
                        .await;
                }
            }),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            Box(
                Modifier::empty()
                    .size_points(content_width, content_height)
                    .draw_behind({
                        let cr = corner_radius;
                        move |scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color(0.12, 0.14, 0.22, 1.0)),
                                CornerRadii::uniform(cr),
                            );
                        }
                    }),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                move || {
                    Text(
                        label,
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(1.0, 1.0, 1.0, 0.85)),
                                font_size: TextUnit::Sp(18.0),
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
