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

const POISSON_OFFSETS: array<vec2<f32>, 36> = array<vec2<f32>, 36>(
    vec2<f32>(0.117851, 0.000000),
    vec2<f32>(-0.150515, 0.137884),
    vec2<f32>(0.023039, -0.262514),
    vec2<f32>(0.189714, 0.247449),
    vec2<f32>(-0.348149, -0.061583),
    vec2<f32>(0.329797, -0.209790),
    vec2<f32>(-0.110311, 0.410350),
    vec2<f32>(-0.210374, -0.405063),
    vec2<f32>(0.456428, 0.166687),
    vec2<f32>(-0.474837, 0.196006),
    vec2<f32>(0.228903, -0.489152),
    vec2<f32>(0.169153, 0.539288),
    vec2<f32>(-0.509831, -0.295457),
    vec2<f32>(0.598089, -0.131488),
    vec2<f32>(-0.365005, 0.519181),
    vec2<f32>(-0.084325, -0.650726),
    vec2<f32>(0.517670, 0.436293),
    vec2<f32>(-0.696621, 0.028807),
    vec2<f32>(0.508132, -0.505659),
    vec2<f32>(-0.033996, 0.735194),
    vec2<f32>(-0.483489, -0.579381),
    vec2<f32>(0.765900, 0.103051),
    vec2<f32>(-0.648945, 0.451519),
    vec2<f32>(0.177329, -0.788246),
    vec2<f32>(0.410153, 0.715772),
    vec2<f32>(-0.801810, -0.255799),
    vec2<f32>(0.778857, -0.359852),
    vec2<f32>(-0.337420, 0.806248),
    vec2<f32>(-0.301140, -0.837246),
    vec2<f32>(0.801302, 0.421142),
    vec2<f32>(-0.890044, 0.234613),
    vec2<f32>(0.505907, -0.786802),
    vec2<f32>(0.160932, 0.936418),
    vec2<f32>(-0.762677, -0.590660),
    vec2<f32>(0.975603, -0.080827),
    vec2<f32>(-0.674347, 0.728949),
);

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let texture_size = vec2<f32>(textureDimensions(input_texture));
    let effect_rect = vec4<f32>(
        get_float(248u), get_float(249u), get_float(250u), get_float(251u)
    );
    let local = clamp(
        (input.uv * texture_size - effect_rect.xy) / max(effect_rect.zw, vec2<f32>(1.0)),
        vec2<f32>(0.0),
        vec2<f32>(1.0)
    );
    let direction = get_float(2u);
    var axis = local.x;
    if direction >= 0.5 && direction < 1.5 {
        axis = 1.0 - local.x;
    } else if direction >= 1.5 && direction < 2.5 {
        axis = local.y;
    } else if direction >= 2.5 {
        axis = 1.0 - local.y;
    }

    // Smoothstep avoids a derivative discontinuity where the frosted region
    // meets the sharp scene. The radius itself—not output alpha—varies.
    let progress = smoothstep(0.0, 1.0, axis);
    let radius_px = mix(get_float(0u), get_float(1u), progress);
    if radius_px < 0.25 {
        return textureSample(input_texture, input_sampler, input.uv);
    }

    // A stable Vogel-disk kernel avoids the separated echo copies produced by
    // a sparse Cartesian grid at large radii. Radial weights approximate a
    // Gaussian while 37 taps keep the full-screen-bar pass mobile-friendly.
    let texel = vec2<f32>(radius_px) / max(texture_size, vec2<f32>(1.0));
    var color = textureSample(input_texture, input_sampler, input.uv) * 1.5;
    var total_weight = 1.5;
    for (var i: u32 = 0u; i < 36u; i = i + 1u) {
        let offset = POISSON_OFFSETS[i];
        let radius_squared = dot(offset, offset);
        let weight = 1.25 - 0.75 * radius_squared;
        let sample_uv = clamp(
            input.uv + offset * texel,
            vec2<f32>(0.0),
            vec2<f32>(1.0)
        );
        color = color + textureSample(input_texture, input_sampler, sample_uv) * weight;
        total_weight = total_weight + weight;
    }
    return color / max(total_weight, 0.00001);
}
