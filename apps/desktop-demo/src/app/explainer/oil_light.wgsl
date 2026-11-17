// The lamp over the oil painting, after `noise.wgsl`. Its input is the
// painting `oil_bake.wgsl` made: the paint's colour, and in alpha its
// thickness. Each frame it only lights that relief: the slopes of the
// paint, the hollows the light reaches less, the shadows the ridges cast
// away from a lamp that hangs up and to the left, low enough to rake
// across, and the sheen of oil.
//
// Lengths are in the page's unit u. Uniforms: 0 = logical pixels per u;
// 1..2 = the lamp across and down, in the stage's widths and heights;
// 3 = the lamp's height in u.

const HEIGHT_FLOOR: f32 = 0.04;
const MAX_HEIGHT: f32 = 14.0;
const PI: f32 = 3.14159265;
// How far the shadow search walks toward the lamp, in u.
const REACH: f32 = 26.0;
const SHADOW_STEPS: i32 = 8;

var<private> texel: vec2<f32>;

fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn paint_at(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(input_texture, input_sampler, uv, 0.0);
}

fn thickness(uv: vec2<f32>) -> f32 {
    return (paint_at(uv).a - HEIGHT_FLOOR) / (1.0 - HEIGHT_FLOOR) * MAX_HEIGHT;
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    return pow(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2));
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

// GGX.
fn highlight(nh: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let d = nh * nh * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

// The room, mirrored in the varnish: dim walls, a brighter ceiling and a
// tall window up and to the left.
fn room(r: vec3<f32>) -> f32 {
    let ceiling = smoothstep(-0.1, 0.75, -r.y);
    let window = smoothstep(0.10, 0.34, -r.y - 0.1 * r.x) * smoothstep(0.95, 0.62, -r.y)
        * smoothstep(0.85, 0.25, abs(r.x + 0.35));
    return 0.16 + 0.42 * ceiling + 2.1 * window;
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let texels = vec2<f32>(textureDimensions(input_texture));
    let page = select(texels, u[63].xy, u[63].x > 0.0);
    let unit = max(get_float(0u), 0.05);
    let lamp_at = vec2<f32>(get_float(1u), get_float(2u));
    let lamp_height = get_float(3u);
    texel = 1.0 / texels;
    let uv = input.uv;
    let texels_per_unit = texels.x / page.x * unit;
    let point = uv * page;

    let here = paint_at(uv);
    let paint = to_linear(here.rgb);
    let h = (here.a - HEIGHT_FLOOR) / (1.0 - HEIGHT_FLOOR) * MAX_HEIGHT;

    // Slopes of the paint (Sobel), in u of height per u across.
    let nw = thickness(uv + texel * vec2<f32>(-1.0, -1.0));
    let n = thickness(uv + texel * vec2<f32>(0.0, -1.0));
    let ne = thickness(uv + texel * vec2<f32>(1.0, -1.0));
    let w = thickness(uv + texel * vec2<f32>(-1.0, 0.0));
    let e = thickness(uv + texel * vec2<f32>(1.0, 0.0));
    let sw = thickness(uv + texel * vec2<f32>(-1.0, 1.0));
    let s = thickness(uv + texel * vec2<f32>(0.0, 1.0));
    let se = thickness(uv + texel * vec2<f32>(1.0, 1.0));
    let slope = vec2<f32>((ne + 2.0 * e + se) - (nw + 2.0 * w + sw), (sw + 2.0 * s + se) - (nw + 2.0 * n + ne))
        / 8.0 * texels_per_unit;

    // How deep the point lies under its surroundings, at two widths.
    let u_step = texel * texels_per_unit;
    var around1 = 0.0;
    var around2 = 0.0;
    for (var i = 0; i < 4; i++) {
        let a = f32(i) * PI * 0.5 + 0.6;
        let o = vec2<f32>(cos(a), sin(a));
        around1 += thickness(uv + o * u_step * 2.5);
        around2 += thickness(uv + o.yx * vec2<f32>(1.0, -1.0) * u_step * 8.0);
    }
    around1 *= 0.25;
    around2 *= 0.25;
    var occlusion = exp(-max(0.0, 0.30 * (around1 - h)) - max(0.0, 0.20 * (around2 - h)));
    occlusion *= 1.0 + 0.05 * clamp(h - around2, 0.0, 2.0);

    // Toward the lamp, a point lamp, so the direction changes over the
    // picture.
    let flat_to_lamp = (lamp_at * page - point * 0.42) / unit;
    let lamp = normalize(vec3<f32>(flat_to_lamp, min(lamp_height, 0.70 * length(flat_to_lamp) + 30.0)));

    // Cast shadows: walk toward the lamp and see how far anything rises
    // above the ray.
    let towards = normalize(lamp.xy + vec2<f32>(1.0e-5, 0.0));
    let rise = lamp.z / max(length(lamp.xy), 1.0e-3);
    var shadow = 0.0;
    for (var i = 1; i <= SHADOW_STEPS; i++) {
        let f = f32(i) / f32(SHADOW_STEPS);
        let dist = REACH * f * sqrt(f) + 0.35;
        let there = thickness(uv + towards * dist * u_step);
        shadow = max(shadow, (there - h - dist * rise) / (0.28 + 0.085 * dist));
    }
    shadow = clamp(shadow, 0.0, 1.0);
    let unshadowed = 1.0 - smoothstep(0.02, 0.98, shadow) * 0.93;

    let normal = normalize(vec3<f32>(-slope * 0.92, 1.0));
    let gloss = smoothstep(0.9, 2.2, h);

    // Diffuse: a warm lamp that falls off away from its corner, and a cool
    // sky from above.
    let nl = dot(normal, lamp);
    let luma = dot(paint, vec3<f32>(0.3, 0.5, 0.2));
    let wrap = 0.03 + 0.06 * smoothstep(0.2, 0.8, luma);
    let diffuse = max(0.0, (nl + wrap) / (1.0 + wrap));
    let falloff = mix(1.12, 0.74, smoothstep(0.0, 1.15, length((uv - vec2<f32>(0.18, 0.05)) * vec2<f32>(1.0, 0.85))));
    let lamp_light = vec3<f32>(1.0, 0.95, 0.86) * 3.0 * falloff;
    let sky_light = mix(vec3<f32>(0.15, 0.19, 0.29), vec3<f32>(0.31, 0.35, 0.43), 0.5 - 0.5 * normal.y) * (0.82 + 0.18 * falloff);
    let tinted = mix(vec3<f32>(1.0), 0.55 + 0.9 * paint, 0.6);
    let in_shadow = mix(0.06, 0.16, luma) * tinted;
    var color = paint * (lamp_light * diffuse * mix(in_shadow, vec3<f32>(1.0), unshadowed) + sky_light * occlusion * tinted * 0.95);

    // Gloss: linen is matte, oil paint has a sheen.
    let view = vec3<f32>(0.0, 0.0, 1.0);
    let halfway = normalize(lamp + view);
    let nh = max(dot(normal, halfway), 0.0);
    let nv = max(normal.z, 0.05);
    let vh = max(dot(view, halfway), 0.0);
    let roughness = mix(0.62, 0.30, gloss);
    let fresnel = 0.04 + 0.96 * pow(1.0 - vh, 5.0);
    let nlc = max(nl, 0.0);
    let k = roughness * roughness * 0.5;
    let visibility = 1.0 / ((nlc * (1.0 - k) + k) * (nv * (1.0 - k) + k) * 4.0);
    let specular = min(highlight(nh, roughness) * visibility * fresnel * nlc, 9.0);
    let sheen = mix(0.12, 1.0, gloss);
    color += lamp_light * specular * unshadowed * sheen * 1.15;
    let mirrored = vec3<f32>(2.0 * normal.z * normal.xy, 2.0 * normal.z * normal.z - 1.0);
    let grazing = 0.04 + 0.32 * pow(1.0 - nv, 4.0);
    color += vec3<f32>(0.93, 0.97, 1.0) * room(mirrored) * grazing * occlusion * sheen * mix(0.6, 1.0, unshadowed) * 0.8;

    let dither = (hash(uv * texels) - 0.5) / 255.0;
    return vec4<f32>(to_srgb(aces(color * 0.61)) + dither, 1.0);
}
