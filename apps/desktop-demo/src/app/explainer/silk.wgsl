// Silk: the content printed on a satin weave. The threads run down the
// cloth, so its sheen is a narrow streak across a fold and a long one down
// it (Ward's anisotropic highlight, sharp across the threads, broad along
// them). Pale ink lies on the weave and shines less. Gold in the content is
// leaf laid on the silk: a metal with its own crumpled grain, raised where
// it ends, glinting where a flake faces the light. Pale ink inside the leaf
// is pressed into it, dark. The print soaks into the cloth from the left,
// along the threads. The cloth's edge is a rolled hem. It follows
// `studio.wgsl` and `noise.wgsl`.
//
// params: 0 = how bright the sheen is; 1 = how far the light wraps round a
// fold; 2 = how bright a fold the light misses is; 3 = how far the print
// has soaked in, from 0 to 1; 4..6 = the gold the content marks leaf with;
// 8..10 = the cloth's own colour; 12..13 = the content's size in logical
// pixels, the unit of every length here.

const HEM: f32 = 8.0;
const THREAD: f32 = 1.1;

fn ward(n: vec3<f32>, h: vec3<f32>, thread: vec3<f32>, across: vec3<f32>, along_width: f32, across_width: f32) -> f32 {
    let h_n = max(dot(h, n), 1.0e-3);
    let t = dot(h, thread) / along_width;
    let b = dot(h, across) / across_width;
    return exp(-(t * t + b * b) / (h_n * h_n));
}

const PRESSED_INK: vec3<f32> = vec3<f32>(0.085, 0.003, 0.010);

fn param3(first: u32) -> vec3<f32> {
    return vec3<f32>(shaded_param(first), shaded_param(first + 1u), shaded_param(first + 2u));
}

// How much of `color` is the gold that marks leaf.
fn leaf(color: vec4<f32>) -> f32 {
    let rgb = color.rgb / max(color.a, 1.0 / 255.0);
    return (1.0 - smoothstep(0.08, 0.22, distance(rgb, param3(4u)))) * color.a;
}

fn leaf_at(uv: vec2<f32>) -> f32 {
    return leaf(content_color(uv));
}

// The most leaf in direction `step` from `uv`, 8, 20, 34 and 50 steps away:
// strokes run from thin to wide.
fn leaf_toward(uv: vec2<f32>, step: vec2<f32>) -> f32 {
    let near = max(leaf_at(uv + step * 8.0), leaf_at(uv + step * 20.0));
    let far = max(leaf_at(uv + step * 34.0), leaf_at(uv + step * 50.0));
    return max(near, far);
}

// How much leaf lies on both sides of `uv`, across or down, `pixel` being
// one pixel of the content in its coordinates.
fn enclosed_by_leaf(uv: vec2<f32>, pixel: vec2<f32>) -> f32 {
    let across = vec2<f32>(pixel.x, 0.0);
    let down = vec2<f32>(0.0, pixel.y);
    let beside = min(leaf_toward(uv, -across), leaf_toward(uv, across));
    let around = min(leaf_toward(uv, -down), leaf_toward(uv, down));
    return max(beside, around);
}

// How far the print has soaked into the cloth at `uv`, `at` pixels into
// the content: it creeps in from the left, unevenly, along the threads.
fn soaked(uv: vec2<f32>, at: vec2<f32>) -> f32 {
    let soak = shaded_param(3u);
    if (soak >= 1.0) {
        return 1.0;
    }
    let front = uv.x * 0.62 + (uv.y - 0.5) * 0.1
        + 0.1 * (value_noise(at * 0.008) - 0.5)
        + 0.25 * (value_noise(at * 0.04 + vec2<f32>(17.0)) - 0.5)
        + 0.05 * (value_noise(vec2<f32>(at.x * 0.1, at.y * 1.8)) - 0.5);
    return smoothstep(front - 0.02, front + 0.24, soak * 1.35 - 0.2);
}

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let alpha = surface.color.a;
    let content = vec2<f32>(shaded_param(12u), shaded_param(13u));
    let at = surface.uv * content;
    let print = soaked(surface.uv, at);
    let base = mix(param3(8u), surface.color.rgb / max(alpha, 1.0 / 255.0), print);
    let v = surface.to_eye;
    let l = surface.to_light;
    let down = vec3<f32>(0.0, 1.0, 0.0);
    var n = surface.normal;
    let up_thread = normalize(down - n * dot(down, n) + vec3<f32>(0.0, 0.0, 1.0e-4));
    let across = normalize(cross(n, up_thread));

    // The rolled hem: the cloth turns under along its outline.
    let edge = min(min(at.x, content.x - at.x), min(at.y, content.y - at.y));
    let roll = 1.0 - smoothstep(0.0, HEM, edge);
    let outward = normalize(vec2<f32>(
        select(1.0, -1.0, at.x < content.x * 0.5) * step(min(at.x, content.x - at.x), edge + 0.5),
        select(1.0, -1.0, at.y < content.y * 0.5) * step(min(at.y, content.y - at.y), edge + 0.5),
    ) + vec2<f32>(1.0e-4));
    n = normalize(n + (across * outward.x + up_thread * outward.y) * roll * 0.9);
    let stitch = smoothstep(0.8, 0.0, abs(edge - HEM * 0.7)) * 0.18;

    // The weave: warp threads down the cloth, weft across it.
    let warp = sin(at.x * 3.14159 / THREAD);
    let weft = sin(at.y * 3.14159 / THREAD);
    let weave = 1.0 + 0.018 * warp * warp - 0.012 * weft * weft;

    let n_l = dot(n, l);
    let n_v = max(dot(n, v), 0.0);
    let wrap = shaded_param(1u);
    let ambient = shaded_param(2u);
    let light = clamp((n_l + wrap) / (1.0 + wrap), 0.0, 1.0) * surface.lit;
    let hollow = mix(0.45, 1.0, smoothstep(0.15, 0.85, n_v));
    let diffuse = (ambient + (1.0 - ambient) * light) * hollow;

    let high = max(base.r, max(base.g, base.b));
    let low = min(base.r, min(base.g, base.b));
    let dyed = (high - low) / max(high, 1.0e-3);
    let gloss = mix(0.4, 1.0, smoothstep(0.55, 0.85, dyed));

    var sheen = vec3<f32>(0.0);
    if (n_l > 0.0) {
        let h = normalize(l + v);
        let sharp = ward(n, h, up_thread, across, 0.6, 0.055);
        let broad = ward(n, h, up_thread, across, 0.9, 0.22);
        let rise = smoothstep(0.0, 0.3, n_l) * surface.lit;
        sheen = (vec3<f32>(1.0, 0.62, 0.6) * sharp * 0.55 + base * broad * 0.7) * rise;
    }
    let rim = pow(1.0 - n_v, 4.0) * 0.25;
    let ink = 1.0 - smoothstep(0.55, 0.85, dyed);
    let shading = mix(diffuse, 0.3 + 0.7 * diffuse, ink);
    var cloth = (base * shading * weave + (sheen * shaded_param(0u) * (0.93 + 0.14 * warp * warp) + base * rim) * gloss) * (1.0 - stitch);

    // Gold leaf: raised where it ends, crumpled across, mirroring the room.
    // Ink with leaf on both sides of it lies in the leaf.
    let own = leaf(surface.color) * print;
    var gilded = own;
    if (own < 0.99 && ink > 0.01) {
        let enclosed = enclosed_by_leaf(surface.uv, 1.0 / max(content, vec2<f32>(1.0)));
        gilded = max(own, enclosed * print);
    }
    if (gilded > 0.0) {
        let texel = 0.75 / max(content, vec2<f32>(1.0));
        let rise_x = leaf_at(surface.uv + vec2<f32>(texel.x, 0.0)) - leaf_at(surface.uv - vec2<f32>(texel.x, 0.0));
        let rise_y = leaf_at(surface.uv + vec2<f32>(0.0, texel.y)) - leaf_at(surface.uv - vec2<f32>(0.0, texel.y));
        // Leaf beaten thin: fine flakes over a few soft wrinkles.
        let fine = vec2<f32>(
            value_noise(at * 1.8) - value_noise(at * 1.8 + vec2<f32>(7.1, 3.3)),
            value_noise(at * 1.8 + vec2<f32>(2.7, 9.2)) - value_noise(at * 1.8 + vec2<f32>(5.5, 1.9)),
        );
        let wrinkle = vec2<f32>(
            value_noise(at * 0.16) - value_noise(at * 0.16 + vec2<f32>(4.3, 8.8)),
            value_noise(at * 0.16 + vec2<f32>(9.4, 0.6)) - value_noise(at * 0.16 + vec2<f32>(1.2, 6.1)),
        );
        let tilt = fine * 0.1 + wrinkle * 0.22 - vec2<f32>(rise_x, rise_y) * 0.45;
        let leaf_normal = normalize(n + across * tilt.x + up_thread * tilt.y);
        let metal = mix(param3(4u), vec3<f32>(1.0, 0.86, 0.5), 0.25);
        let mirrored = studio(reflect(-v, leaf_normal));
        let h = normalize(l + v);
        let highlight = pow(max(dot(leaf_normal, h), 0.0), 90.0) * surface.lit;
        let flake = floor(at);
        let glint = step(0.975, hash(flake)) * pow(max(dot(leaf_normal, h), 0.0), 12.0) * 2.0 * surface.lit;
        let leafed = metal * (mirrored * 0.8 + diffuse * 0.3) + vec3<f32>(1.0, 0.95, 0.8) * (highlight * 1.4 + glint);
        let pressed = PRESSED_INK * (0.6 + 0.6 * diffuse);
        cloth = mix(cloth, mix(pressed, leafed, own / gilded), gilded / max(alpha, 1.0e-3));
    }
    return vec4<f32>(cloth * alpha, alpha);
}
