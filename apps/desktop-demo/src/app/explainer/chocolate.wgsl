// Molten chocolate, after `runtime_shader_prelude.wgsl`. The content draws
// its letters in key colours: the headline red, the paragraph blue, drips
// black and a selection green. Three blurred copies of it, the renderer's
// substrates, stand in for a film solve: a blurred letter is thick in the
// middle of a wide stroke and thin at its rim and in a thin stroke, as a
// poured film is. From them come the softened, swollen outline, the film's
// height and slope, the light it lets through (it glows orange where thin),
// a studio mirrored in it and a coloured shadow on the wall. A selection is
// mint icing with the letters sitting in it.
//
// Uniforms: 0..1 = the content's size in logical pixels; 2 = pixels to the
// unit; 3 = how far the outline has softened; 4 = how far the strokes have
// swollen; 5 = how hot the chocolate is; 6..7 = the warm spot under the
// pointer, in the content's pixels, and 8 its strength; 9 = the headline's
// size against its design size, 10 = the paragraph's; 11 = device pixels to
// the logical pixel; 12..14 = the three substrates' blur radii in logical
// pixels; 15 = the height that parts headline and paragraph; 16..18 = the
// wall's colour at the top, 20..22 at the bottom, both linear; 24..25 =
// where the content lies on the stage; 26..27 = the stage's size.

const ABSORPTION: vec3<f32> = vec3<f32>(0.27, 0.63, 1.75);
const COCOA: vec3<f32> = vec3<f32>(0.0355, 0.0122, 0.0053);
const ICING_SHADE: vec3<f32> = vec3<f32>(0.0368, 0.4125, 0.2789);
const ICING_LIGHT: vec3<f32> = vec3<f32>(0.3712, 0.8228, 0.6514);
const LIGHT: vec3<f32> = vec3<f32>(-0.4153, -0.5722, 0.7845);
const WARM_REACH: f32 = 105.0;

fn param(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn param3(first: u32) -> vec3<f32> {
    return vec3<f32>(param(first), param(first + 1u), param(first + 2u));
}

// The content at `uv` through a region of the input in texels; the whole
// input when the region is empty.
fn read(region: vec4<f32>, uv: vec2<f32>) -> vec4<f32> {
    let dims = vec2<f32>(textureDimensions(input_texture));
    let whole = vec4<f32>(0.0, 0.0, dims);
    let r = select(whole, region, region.z > 0.0);
    let texel = clamp(r.xy + uv * r.zw, r.xy + vec2<f32>(0.5), r.xy + r.zw - vec2<f32>(0.5));
    return textureSampleLevel(input_texture, input_sampler, texel / dims, 0.0);
}

// Blurred copy `which` (0, 1 or 2) of the content at `uv`; the content
// itself where the renderer gave none.
fn blurred(which: u32, uv: vec2<f32>) -> vec4<f32> {
    var region = u[58];
    if (which == 1u) {
        region = u[57];
    } else if (which == 2u) {
        region = u[56];
    }
    if (region.z <= 0.0) {
        region = u[59];
    }
    return read(region, uv);
}

// The chocolate in a texel of the content: what is not icing.
fn chocolate_of(texel: vec4<f32>) -> f32 {
    return max(texel.a - texel.g, 0.0);
}

fn drip_of(texel: vec4<f32>) -> f32 {
    return max(texel.a - texel.r - texel.g - texel.b, 0.0);
}

// Pseudo distance to the outline of a coverage blurred by `sigma`,
// positive inside: a blurred edge rises 0.4 / sigma per pixel at its
// middle.
fn edge_distance(coverage: f32, sigma: f32) -> f32 {
    return (coverage - 0.5) * 2.5 * sigma;
}

// Union of two distance fields with a rounded crease `k` wide.
fn unite(a: f32, b: f32, k: f32) -> f32 {
    let h = max(k - abs(a - b), 0.0) / k;
    return max(a, b) + h * h * k * 0.25;
}

fn warmth(q: vec2<f32>, unit: f32) -> f32 {
    let strength = param(8u);
    let d = (q - vec2<f32>(param(6u), param(7u))) / (unit * WARM_REACH);
    return strength * exp(-dot(d, d) * 0.5);
}

// A soft shoulder for the brightest light.
fn shoulder(c: vec3<f32>) -> vec3<f32> {
    return c / (1.0 + max(c - vec3<f32>(0.92), vec3<f32>(0.0)) * 0.75);
}

// The wall as `plaster.wgsl` paints it, without its relief.
fn wall_at(q: vec2<f32>, size: vec2<f32>) -> vec3<f32> {
    let c = q / size;
    let paint = mix(param3(16u), param3(20u), smoothstep(-0.1, 1.15, c.y * 0.75 + c.x * 0.35));
    var key = 1.13 - 0.31 * length((c - vec2<f32>(0.20, 0.02)) * vec2<f32>(0.72, 0.95));
    key *= 0.93 + 0.09 * smoothstep(0.0, 0.9, 1.0 - abs(c.x * 0.9 + c.y * 0.42 - 0.52));
    return shoulder(paint * key);
}

fn edge(d: f32, fw: f32) -> f32 {
    return clamp(d / fw * 0.5 + 0.5, 0.0, 1.0);
}

// The room mirrored in the film. `s` is the slope of the mirror direction.
// One big softbox hangs up left; it is near, so its reflection runs on from
// letter to letter. A narrow strip light stands to the right.
fn studio(s: vec2<f32>, q: vec2<f32>, size: vec2<f32>, fw_in: vec2<f32>, wall: vec3<f32>) -> vec3<f32> {
    let on_wall = vec2<f32>((q.x - 0.46 * size.x) / size.y, (q.y - 0.40 * size.y) / size.y);
    var w = s + on_wall * vec2<f32>(0.58, 0.66);
    w = vec2<f32>(w.x + 0.16 * w.y, w.y - 0.10 * w.x);
    let fw = max(fw_in, vec2<f32>(0.016));
    var c = mix(vec3<f32>(0.035, 0.032, 0.034), vec3<f32>(0.34, 0.26, 0.22), 1.0 - smoothstep(-0.9, 0.0, s.y + 0.2 * s.x));
    let box_x = min(edge(w.x + 0.98, fw.x * 2.0), edge(0.10 - w.x, fw.x + 0.05));
    let box_y = min(edge(w.y + 0.60, fw.y), edge(-0.225 - w.y, fw.y + 0.006));
    let brightness = mix(8.0, 36.0, 1.0 - smoothstep(-0.52, -0.22, w.y));
    c += vec3<f32>(1.0, 0.965, 0.92) * box_x * box_y * brightness * (0.45 + 0.55 * (1.0 - smoothstep(-0.5, 0.10, w.x)));
    let strip = edge(0.045 - abs(w.x - 0.70 + 0.15 * w.y), fw.x) * edge(0.55 - abs(w.y + 0.05), fw.y + 0.15);
    c += vec3<f32>(1.0, 0.94, 0.88) * strip * 10.0;
    c += wall * (0.10 + 0.75 * smoothstep(0.05, 0.8, s.y));
    return mix(c, wall * 0.82, smoothstep(0.85, 1.2, length(s)));
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x < vec3<f32>(0.0031308));
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<f32>(param(0u), param(1u));
    let unit = param(2u);
    let uv = input.uv;
    let q = uv * size;
    let stage = vec2<f32>(param(26u), param(27u));
    let on_stage = q + vec2<f32>(param(24u), param(25u));
    let px = 1.0 / max(param(11u), 0.5);
    let sigma = vec3<f32>(param(12u), param(13u), param(14u));
    let head_scale = param(9u);
    let copy_scale = param(10u);

    let sharp = read(u[59], uv);
    let near = blurred(0u, uv);
    let wide = blurred(1u, uv);
    let heading = 1.0 - smoothstep(param(15u) - unit * 6.0, param(15u) + unit * 6.0, q.y);
    let drip_here = smoothstep(0.02, 0.25, drip_of(near));
    let bold = max(heading, drip_here);
    let hot = warmth(q, unit);

    // The letters: sharp at first, rounded off as they soften, swollen.
    let soft = clamp(param(3u) + hot * 0.6, 0.0, 1.0);
    let sharp_ink = sharp.r + sharp.b;
    let sharp_distance = (sharp_ink - 0.5) * 1.6 * px;
    let round_sigma = mix(sigma.x, sigma.y, heading);
    let round_ink = mix(near.r + near.b, wide.r + wide.b, heading);
    var letters = mix(sharp_distance, edge_distance(round_ink, round_sigma), soft);
    let type_scale = mix(0.27 * copy_scale, head_scale, heading);
    letters += unit * type_scale * (param(4u) * mix(0.2, 1.0, heading) * 1.8 + hot * 2.2);

    let drips = edge_distance(drip_of(near), sigma.x);
    let crease = unit * mix(3.5 * copy_scale, 11.0 * head_scale, heading) * (0.25 + 0.75 * soft) + 0.01;
    let chocolate = unite(letters, drips, crease);
    let cov = clamp(0.5 + chocolate / max(fwidth(chocolate), 0.35 * px), 0.0, 1.0);

    // The icing: the blurred selection with a wavy edge that slumps as the
    // chocolate melts.
    let qu = q / unit;
    let wavy = 0.11 * sin(qu.x * 0.037 + qu.y * 0.05) + 0.08 * sin(qu.x * 0.083 + 1.7 + qu.y * 0.021) + 0.04 * sin(qu.x * 0.19 + qu.y * 0.11);
    let selection = wide.g;
    let icing = (selection - 0.31 + wavy * smoothstep(0.0, 0.3, selection)) * unit * 17.0;
    let cov_icing = clamp(0.5 + icing / max(fwidth(icing), 0.35 * px), 0.0, 1.0) * step(0.004, selection);

    // The film: its height in units and its slope, from the blurred
    // chocolate. Drips and the headline stand thick, the paragraph thin.
    // The headline's film is read from the wide copy, the paragraph's and
    // the drips' from the near one.
    let wide_film = heading > 0.5;
    let film_sigma = select(sigma.x, sigma.y, wide_film);
    let step_uv = vec2<f32>(film_sigma * 0.6) / size;
    let which = select(0u, 1u, wide_film);
    let film = chocolate_of(select(near, wide, wide_film));
    let gradient = vec2<f32>(
        chocolate_of(blurred(which, uv + vec2<f32>(step_uv.x, 0.0))) - chocolate_of(blurred(which, uv - vec2<f32>(step_uv.x, 0.0))),
        chocolate_of(blurred(which, uv + vec2<f32>(0.0, step_uv.y))) - chocolate_of(blurred(which, uv - vec2<f32>(0.0, step_uv.y))),
    ) / (2.0 * film_sigma * 0.6);
    let amplitude = unit * mix(mix(2.4 * copy_scale, 4.5, drip_here), 10.0 * max(head_scale, 0.6), heading);
    // A drop's profile: the film turns over fast within a rim of the
    // outline, so the rim faces the room and catches its light in a thin
    // line, and rises in a low dome over the rest.
    let rim_width = unit * mix(mix(1.6 * copy_scale, 2.6, drip_here), 6.0 * max(head_scale, 0.6), heading);
    // How far in from the outline: the outline's own distance near it, the
    // blurred film's farther in, where a sharp outline's distance stops.
    let inside = clamp(max(chocolate, edge_distance(film, film_sigma)) / rim_width, 0.0, 1.0);
    let rim_rise = 1.0 - (1.0 - inside) * (1.0 - inside) * (1.0 - inside);
    let height = amplitude * (0.45 * rim_rise + 0.55 * film) / unit;
    // A poured surface is never quite level: slow ripples fixed to the text.
    let ripple = vec2<f32>(
        0.5 * cos(qu.x * 0.043 + 0.9 * sin(qu.y * 0.027 + 1.0)) + 0.35 * cos(qu.x * 0.091 - qu.y * 0.052 + 2.0),
        0.55 * cos(qu.y * 0.049 + 0.8 * sin(qu.x * 0.021) + 0.5) + 0.3 * cos(qu.y * 0.102 + qu.x * 0.047),
    );
    let inward = gradient / max(length(gradient), 1.0e-5);
    let rim_slope = 3.0 * (1.0 - inside) * (1.0 - inside) * 0.45 * amplitude / rim_width;
    var slope = -inward * rim_slope - 0.55 * amplitude * gradient;
    slope = slope / max(1.0, length(slope) / 2.4) + ripple * 0.05 * smoothstep(2.0, 7.0, height);
    let icing_gradient = vec2<f32>(
        blurred(1u, uv + vec2<f32>(step_uv.x, 0.0)).g - blurred(1u, uv - vec2<f32>(step_uv.x, 0.0)).g,
        blurred(1u, uv + vec2<f32>(0.0, step_uv.y)).g - blurred(1u, uv - vec2<f32>(0.0, step_uv.y)).g,
    ) / (2.0 * film_sigma * 0.6);
    let slope_icing = -unit * 9.0 * icing_gradient + ripple.yx * 0.07;
    let blur_icing = fwidth(slope_icing);

    // The wall behind, and the shadow on it: a tight one and a wide one,
    // tinted by what the film lets through.
    let wall = wall_at(on_stage, stage);
    let thrown = vec2<f32>(5.0, 8.0) * unit * mix(0.42, 1.0, bold);
    let shadow_near = chocolate_of(blurred(1u, (q - thrown * 0.3) / size));
    let shadow_far = chocolate_of(blurred(2u, (q - thrown * 1.5) / size));
    let through_near = exp(-ABSORPTION * amplitude / unit * shadow_near * 1.25) * 1.02;
    let through_far = exp(-ABSORPTION * amplitude / unit * shadow_far * 1.25) * 1.02;
    var shade = mix(vec3<f32>(1.0), through_near, mix(0.28, 0.45, bold) * smoothstep(0.0, 0.6, shadow_near))
        * mix(vec3<f32>(1.0), through_far, mix(0.04, 0.3, bold) * smoothstep(0.0, 0.6, shadow_far));
    shade = mix(shade, vec3<f32>(dot(shade, vec3<f32>(0.3, 0.45, 0.25))), mix(0.75, 0.25, bold));
    let base = wall * shade;
    let halfway = normalize(LIGHT + vec3<f32>(0.0, 0.0, 1.0));

    var color = base;
    if (cov_icing > 0.001) {
        let n = normalize(vec3<f32>(slope_icing, 1.0));
        let room = min(studio(slope_icing, on_stage, stage, blur_icing + 0.03, wall), vec3<f32>(8.0));
        let fresnel = 0.05 + 0.95 * pow(1.0 - n.z, 5.0);
        let lit = max(dot(n, LIGHT), 0.0);
        let paint = mix(ICING_SHADE, ICING_LIGHT, smoothstep(0.30, 0.92, lit));
        var c = paint * (0.24 + 0.92 * lit) + paint * wall * 0.35 * smoothstep(0.0, 0.8, slope_icing.y);
        c += paint * pow(max(dot(n, halfway), 0.0), 18.0) * 0.35;
        c *= 1.0 - 0.42 * smoothstep(0.0, 0.6, shadow_near);
        // The letters sit in the icing: it darkens toward them.
        c *= 1.0 - 0.38 * exp(min(chocolate, 0.0) / (2.6 * unit));
        color = mix(color, shoulder(c * (1.0 - fresnel) + room * fresnel * 1.5), cov_icing);
    }

    if (cov > 0.001) {
        let n = normalize(vec3<f32>(slope, 1.0));
        let lit = max(dot(n, LIGHT), 0.0);
        let heat = clamp(param(5u) + hot, 0.0, 1.6);
        // How far a pixel's reflection reaches, from the film's own
        // curvature: small on a tall film, large at its rim.
        let rim = height * unit / 0.62 + unit * 0.7;
        let blur = px * (dot(slope, slope) / 0.62 + 0.62) / rim * (abs(slope) / max(length(slope), 0.01) + 0.2) * 1.2;
        let min_blur = vec2<f32>(mix(0.16, 0.012, bold));
        let room = studio(slope, on_stage, stage, max(blur, min_blur), wall);
        let fresnel = min(0.05 + 0.95 * pow(1.0 - n.z, 5.0), 0.55);

        // Chocolate is opaque: light gets through only where the film is
        // thin.
        let density = mix(mix(7.5 / copy_scale, 1.05, drip_here), 1.1 / head_scale, heading);
        // The light crosses the film and the chocolate around it: a rim
        // with bulk behind it stays dark, a thin drip glows.
        let bulk = amplitude / unit * chocolate_of(wide);
        let bulky = amplitude / unit * chocolate_of(blurred(2u, uv)) * bold;
        let path = (height * 0.16 + bulk * 1.05) * density * (1.0 + 3.0 * smoothstep(0.8, 3.2, bulky));
        let through = exp(-ABSORPTION * mix(1.0, 0.8, clamp(heat, 0.0, 1.0)) * (1.0 - 0.35 * hot) * path);
        let body = COCOA * (0.30 + 0.95 * lit) + COCOA * wall * 0.55 * smoothstep(0.1, 0.9, slope.y)
            + vec3<f32>(0.9, 0.52, 0.32) * pow(max(dot(n, halfway), 0.0), 20.0) * 0.016;
        let backlit = smoothstep(0.05, 1.25, dot(slope, vec2<f32>(0.57, 0.82)));
        let glow = vec3<f32>(1.0, 0.47, 0.08) * through * (0.12 + 0.8 * backlit * backlit);
        var c = color * through * 0.85 + glow * 0.6 + body * (1.0 - through)
            + vec3<f32>(1.0, 0.36, 0.05) * hot * 0.20 * exp(-path * 0.10);
        let contact = mix(0.4, 1.0, smoothstep(0.0, 1.6, height));
        c = c * (1.0 - fresnel) + room * fresnel * contact * mix(0.5, 1.0, bold);
        color = mix(color, shoulder(c), cov);
    }

    // Over the wall: the chocolate and the icing cover it; the shadow tints
    // it, as a share of the wall's own colour that it takes away.
    let cover = max(cov, cov_icing);
    let target_color = to_srgb(color);
    let wall_color = to_srgb(wall);
    let kept = clamp(min(min(target_color.r / wall_color.r, target_color.g / wall_color.g), target_color.b / wall_color.b), 0.0, 1.0);
    let shadow_alpha = 1.0 - kept;
    let alpha = mix(shadow_alpha, 1.0, cover);
    let premultiplied = target_color - wall_color * (1.0 - alpha);
    return vec4<f32>(max(premultiplied, vec3<f32>(0.0)), alpha);
}
