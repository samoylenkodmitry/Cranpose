// What both copies of the paragraph share, after `noise.wgsl`: colours
// between sRGB and linear light, the content with its guide lines taken out,
// and drafting film, the paper the paragraph is printed on.
//
// The content marks the boxes ruled round its lines and words in pure green,
// a colour nothing else in it takes: the flat copy shows them in blue, the
// bent copy leaves them out.

const INK_BLUE: vec3<f32> = vec3<f32>(0.10, 0.30, 0.92);

fn to_linear(color: vec3<f32>) -> vec3<f32> {
    return pow(max(color, vec3<f32>(0.0)), vec3<f32>(2.2));
}

fn to_srgb(color: vec3<f32>) -> vec3<f32> {
    return pow(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2));
}

// How much guide green lies on the content colour `color`.
fn guide_in(color: vec3<f32>) -> f32 {
    return clamp(color.g - max(color.r, color.b), 0.0, 1.0);
}

// The content colour with the guide green taken out, in linear light.
fn without_guides(color: vec3<f32>) -> vec3<f32> {
    let guide = guide_in(color);
    let under = (color - vec3<f32>(0.0, guide, 0.0)) / max(1.0 - guide, 1.0e-3);
    return to_linear(mix(under, vec3<f32>(1.0), smoothstep(0.85, 0.98, guide)));
}

// Drafting film at sheet point `q`, where a pixel spans `footprint`: warm
// white with fibres and a pale printed grid, 24 units a square, which fades
// where its lines grow thinner than a pixel.
fn drafting_film(q: vec2<f32>, u: f32, footprint: vec2<f32>) -> vec3<f32> {
    let at = q / u;
    let fibre = (value_noise(at * 0.5) * 0.6 + value_noise(at * 1.3) * 0.4 - 0.5) * 0.045;
    let color = vec3<f32>(0.905, 0.895, 0.862) * (1.0 + fibre);
    let to_line = abs(fract(at / 24.0 - 0.5) - 0.5) * 24.0;
    let width = max(footprint / u, vec2<f32>(1.0e-3));
    let line = clamp((vec2<f32>(0.45) - to_line) / width + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));
    let grid = max(line.x, line.y) * min(1.0, 0.9 / max(width.x, width.y));
    return mix(color, vec3<f32>(0.42, 0.66, 0.78), grid * 0.20);
}

// The distance to a box of half size `half` with corners rounded by
// `radius`, from `q` measured from its middle.
fn round_box(q: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let d = abs(q) - half + vec2<f32>(radius);
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - radius;
}
