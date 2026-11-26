// Real-time WGSL RuntimeShader effects applied directly to live playing GPU video layers.
// Each shader reads Cranpose's offscreen video layer via `input_texture`/`input_sampler`
// with zero CPU readback.

use std::sync::OnceLock;

use cranpose_ui_graphics::{RenderEffect, RuntimeShader, RUNTIME_SHADER_PRELUDE_WGSL};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum VideoShaderPreset {
    None,
    ChromaticPrism,
    LiquidWave,
    CinemaHalation,
}

impl VideoShaderPreset {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::None => "Shader: Off",
            Self::ChromaticPrism => "Shader: Prism",
            Self::LiquidWave => "Shader: Wave",
            Self::CinemaHalation => "Shader: Cinema",
        }
    }

    pub(super) const fn next(self) -> Self {
        match self {
            Self::None => Self::ChromaticPrism,
            Self::ChromaticPrism => Self::LiquidWave,
            Self::LiquidWave => Self::CinemaHalation,
            Self::CinemaHalation => Self::None,
        }
    }
}

const PRISM_FS: &str = r"
@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let time = u[0u].x;
    let amount = u[0u].y;
    let phase = u[0u].z;
    let uv = input.uv;
    let centered = uv - vec2<f32>(0.5, 0.5);
    let dist = length(centered);
    let shift = (0.008 + 0.006 * sin(time * 2.2 + phase)) * amount * (0.35 + dist);
    let dir = select(vec2<f32>(1.0, 0.0), normalize(centered), dist > 0.001);
    let r = textureSample(input_texture, input_sampler, clamp(uv + dir * shift, vec2<f32>(0.0), vec2<f32>(1.0))).r;
    let g = textureSample(input_texture, input_sampler, uv).g;
    let b = textureSample(input_texture, input_sampler, clamp(uv - dir * shift, vec2<f32>(0.0), vec2<f32>(1.0))).b;
    let a = textureSample(input_texture, input_sampler, uv).a;
    let rim = smoothstep(0.32, 0.72, dist) * 0.16 * amount;
    let glow = vec3<f32>(0.22, 0.62, 1.0) * rim;
    return vec4<f32>(vec3<f32>(r, g, b) + glow, a);
}
";

const WAVE_FS: &str = r"
@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let time = u[0u].x;
    let amount = u[0u].y;
    let phase = u[0u].z;
    let uv = input.uv;
    let wave_x = sin(uv.y * 16.0 + time * 3.1 + phase) * 0.012 * amount;
    let wave_y = cos(uv.x * 14.0 - time * 2.4 + phase) * 0.010 * amount;
    let warped = clamp(uv + vec2<f32>(wave_x, wave_y), vec2<f32>(0.001), vec2<f32>(0.999));
    let sample = textureSample(input_texture, input_sampler, warped);
    let caustic = pow(max(0.0, sin((uv.x + uv.y) * 18.0 - time * 3.8 + phase)), 4.0) * 0.14 * amount;
    return vec4<f32>(sample.rgb + vec3<f32>(0.18, 0.48, 0.65) * caustic, sample.a);
}
";

const CINEMA_FS: &str = r"
@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let time = u[0u].x;
    let amount = u[0u].y;
    let uv = input.uv;
    let base = textureSample(input_texture, input_sampler, uv);
    let luma = dot(base.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let warm = vec3<f32>(1.08, 1.01, 0.92);
    let cool = vec3<f32>(0.90, 0.98, 1.09);
    let grade = mix(cool, warm, smoothstep(0.18, 0.82, luma));
    let centered = uv - vec2<f32>(0.5, 0.5);
    let vig = 1.0 - smoothstep(0.35, 0.85, length(centered)) * 0.36 * amount;
    let sweep = exp(-pow((uv.x - fract(time * 0.22)) * 10.0, 2.0)) * 0.10 * amount;
    let rgb = mix(base.rgb, base.rgb * grade, 0.65 * amount) * vig + vec3<f32>(1.0, 0.85, 0.65) * sweep;
    return vec4<f32>(rgb, base.a);
}
";

fn shader_source(preset: VideoShaderPreset) -> Option<&'static str> {
    match preset {
        VideoShaderPreset::None => None,
        VideoShaderPreset::ChromaticPrism => {
            static SRC: OnceLock<String> = OnceLock::new();
            Some(
                SRC.get_or_init(|| format!("{RUNTIME_SHADER_PRELUDE_WGSL}\n{PRISM_FS}"))
                    .as_str(),
            )
        }
        VideoShaderPreset::LiquidWave => {
            static SRC: OnceLock<String> = OnceLock::new();
            Some(
                SRC.get_or_init(|| format!("{RUNTIME_SHADER_PRELUDE_WGSL}\n{WAVE_FS}"))
                    .as_str(),
            )
        }
        VideoShaderPreset::CinemaHalation => {
            static SRC: OnceLock<String> = OnceLock::new();
            Some(
                SRC.get_or_init(|| format!("{RUNTIME_SHADER_PRELUDE_WGSL}\n{CINEMA_FS}"))
                    .as_str(),
            )
        }
    }
}

pub(super) fn render_effect(
    preset: VideoShaderPreset,
    time_s: f32,
    intensity: f32,
    phase: f32,
) -> Option<RenderEffect> {
    let source = shader_source(preset)?;
    let mut shader = RuntimeShader::new(source);
    shader.set_float(0, time_s);
    shader.set_float(1, intensity);
    shader.set_float(2, phase);
    Some(RenderEffect::runtime_shader(shader))
}
