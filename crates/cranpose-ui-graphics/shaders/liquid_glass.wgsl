
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

// Uniform accessors
fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn get_vec2(index: u32) -> vec2<f32> {
    return vec2<f32>(get_float(index), get_float(index + 1u));
}

fn get_vec4(index: u32) -> vec4<f32> {
    return vec4<f32>(get_float(index), get_float(index + 1u), get_float(index + 2u), get_float(index + 3u));
}

// SDF for rounded rectangle — matches Android sdRoundRect
fn sd_round_rect(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

// Height profile: glass surface curvature.
// Matches Android AGSL exactly:
//   circle:   x * x          (0 at edge, 1 deep inside)
//   squircle: 1 - pow(1-x,4) (0 at edge, 1 deep inside)
// `x` is normalized distance from edge inward (0=edge, 1=deep inside).
fn height_profile(x: f32, profile: f32) -> f32 {
    let xc = clamp(x, 0.0, 1.0);
    let h_circle = xc * xc;
    let h_squircle = 1.0 - pow(1.0 - xc, 4.0);
    return mix(h_circle, h_squircle, clamp(profile, 0.0, 1.0));
}

// Derivative of height profile — well-behaved, no singularities.
//   circle':   2 * x
//   squircle': 4 * pow(1-x, 3)
fn d_height_dx(x: f32, profile: f32) -> f32 {
    let xc = clamp(x, 0.0, 1.0);
    let d_circle = 2.0 * xc;
    let d_squircle = 4.0 * pow(1.0 - xc, 3.0);
    return mix(d_circle, d_squircle, clamp(profile, 0.0, 1.0));
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = input.uv;
    let tex_size = vec2<f32>(textureDimensions(input_texture));

    // Effect layer pixel rect injected by the renderer at uniform slot 62
    // (x_offset, y_offset, width, height) in viewport pixels.
    let effect_rect = get_vec4(248u);
    let container_dp = get_vec2(0u);

    // dp → pixel scale: effect area pixel size / container dp size
    let dp_scale = effect_rect.zw / max(container_dp, vec2<f32>(1.0));
    let s = min(dp_scale.x, dp_scale.y);

    // Fragment position in effect-local pixel coordinates
    let coord = uv * tex_size - effect_rect.xy;

    let center = get_vec2(2u) * dp_scale;
    let rect_size = get_vec2(4u) * dp_scale;
    let corner_radius = get_float(6u) * s;
    let bezel = get_float(7u) * s;
    let disp_scale = get_float(8u) * s;
    let ri = get_float(9u);
    let profile = get_float(10u);
    let highlight = get_float(11u);
    let tilt_angle = get_float(12u);
    let tilt_pitch = get_float(13u);
    let tint_color = get_vec4(14u);

    let half_size = rect_size * 0.5;

    // SDF distance from rounded rect edge (negative = inside)
    let p = coord - center;
    let d = sd_round_rect(p, half_size, corner_radius);

    // If outside the glass rect, pass through original
    if d > 0.0 {
        return textureSample(input_texture, input_sampler, uv);
    }

    // Normalized distance: 0 at rect edge, 1 at one bezel-width inside
    let x = clamp(-d / max(bezel, 0.001), 0.0, 1.0);

    // Height and slope from profile (bezel curvature)
    let slope = d_height_dx(x, profile);

    // Refraction displacement (matches Android AGSL):
    //   bend = slope * (1 - 1/ri)
    //   tilt is a raw vec2 (angle, pitch), NOT trigonometric
    //   displacement = tilt * bend * scale
    let bend = slope * (1.0 - 1.0 / max(ri, 1.0001));
    let tilt = vec2<f32>(tilt_angle, tilt_pitch);
    let disp = tilt * bend * disp_scale;
    let displaced_uv = clamp(uv + disp / tex_size, vec2<f32>(0.0), vec2<f32>(1.0));

    // Sample refracted background
    var outc = textureSample(input_texture, input_sampler, displaced_uv);

    // Tint color blending (matches Android: mix(outc.rgb, tint.rgb, tint.a))
    let tint_a = tint_color.a;
    outc = vec4<f32>(mix(outc.rgb, tint_color.rgb, tint_a), outc.a);

    // SDF gradient for specular normal computation
    let eps = 0.5;
    let d_dx = sd_round_rect(p + vec2<f32>(eps, 0.0), half_size, corner_radius);
    let d_dy = sd_round_rect(p + vec2<f32>(0.0, eps), half_size, corner_radius);
    let grad = vec2<f32>(d_dx - d, d_dy - d) / eps;
    let grad_len = length(grad);
    let inward_normal = select(vec2<f32>(0.0), -grad / grad_len, grad_len > 0.001);

    // Specular rim highlight (matches Android AGSL):
    //   rim = pow(1-x, 3), facing = 0.5 + 0.5 * dot(N, lightDir)
    //   spec added as: outc + spec * specA * 0.85
    let light_dir = normalize(tilt + vec2<f32>(0.0, 0.5));
    let rim = pow(max(1.0 - x, 0.0), 3.0);
    let facing = 0.5 + 0.5 * dot(inward_normal, light_dir);
    let spec_a = highlight * rim * facing;
    outc = vec4<f32>(outc.rgb + vec3<f32>(spec_a * 0.85), outc.a);

    return outc;
}
