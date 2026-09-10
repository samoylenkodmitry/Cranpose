const MAX_STEPS: i32 = 96;
const SHADOW_STEPS: i32 = 26;
const MAX_DIST: f32 = 9.0;
const BOUND_RADIUS: f32 = 1.30;
const BOUND_CENTER: vec3<f32> = vec3<f32>(0.0, 0.26, 0.0);

fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn scene_kind() -> i32 {
    return i32(get_float(0u) + 0.5);
}

fn key_dir() -> vec3<f32> {
    return normalize(vec3<f32>(-0.42, 0.86, 0.30));
}

fn rot_y(p: vec3<f32>, a: f32) -> vec3<f32> {
    let s = sin(a);
    let c = cos(a);
    return vec3<f32>(c * p.x + s * p.z, p.y, c * p.z - s * p.x);
}

fn rot_z(p: vec3<f32>, a: f32) -> vec3<f32> {
    let s = sin(a);
    let c = cos(a);
    return vec3<f32>(c * p.x - s * p.y, s * p.x + c * p.y, p.z);
}

fn sd_round_box(p: vec3<f32>, b: vec3<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec3<f32>(r);
    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0) - r;
}

fn sd_capsule(p: vec3<f32>, a: vec3<f32>, b: vec3<f32>, r: f32) -> f32 {
    let offset_a = p - a;
    let axis = b - a;
    let h = clamp(dot(offset_a, axis) / max(dot(axis, axis), 1e-6), 0.0, 1.0);
    return length(offset_a - axis * h) - r;
}

fn sd_round_cylinder(p: vec3<f32>, radius: f32, half_height: f32, edge: f32) -> f32 {
    let d = vec2<f32>(length(p.xz) - (radius - edge), abs(p.y) - (half_height - edge));
    return min(max(d.x, d.y), 0.0) + length(max(d, vec2<f32>(0.0))) - edge;
}

fn sd_sphere(p: vec3<f32>, r: f32) -> f32 {
    return length(p) - r;
}

fn crowned(d: f32, p: vec3<f32>, top: f32, curve: f32) -> f32 {
    return max(d, sd_sphere(p - vec3<f32>(0.0, top - curve, 0.0), curve));
}

fn sd_segment_2d(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let offset_a = p - a;
    let axis = b - a;
    let h = clamp(dot(offset_a, axis) / max(dot(axis, axis), 1e-6), 0.0, 1.0);
    return length(offset_a - axis * h);
}

fn sd_round_rect_2d(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

fn closer(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    if (a.x < b.x) {
        return a;
    }
    return b;
}

fn map_check(p: vec3<f32>, value: f32, press: f32) -> vec2<f32> {
    let r = 0.085 * smoothstep(0.0, 0.16, value);
    if (r < 0.004) {
        return vec2<f32>(MAX_DIST, 0.0);
    }
    let y = 0.105 - press * 0.020;
    let a = vec3<f32>(-0.48, y, -0.10);
    let b = vec3<f32>(-0.14, y, 0.50);
    let c = vec3<f32>(0.52, y, -0.62);
    let t1 = clamp(value / 0.40, 0.0, 1.0);
    let t2 = clamp((value - 0.36) / 0.64, 0.0, 1.0);
    var d = sd_capsule(p, a, mix(a, b, t1), r);
    if (t2 > 0.0) {
        d = min(d, sd_capsule(p, b, mix(b, c, t2), r));
    }
    return vec2<f32>(d, 0.0);
}

fn map_slider(p: vec3<f32>, value: f32, press: f32) -> vec2<f32> {
    let travel = get_float(17u);
    let rail = sd_capsule(
        p,
        vec3<f32>(-travel - 0.22, 0.052, 0.0),
        vec3<f32>(travel + 0.22, 0.052, 0.0),
        0.025,
    );
    let x = mix(-travel, travel, value);
    let lift = get_float(18u) - press * 0.014;
    let centre = p - vec3<f32>(x, lift, 0.0);
    var puck = crowned(
        sd_round_cylinder(centre, 0.285, 0.105, 0.058),
        centre,
        0.105,
        2.4,
    );
    let groove = sd_round_box(
        centre - vec3<f32>(0.0, 0.100, 0.0),
        vec3<f32>(0.130, 0.018, 0.012),
        0.010,
    );
    puck = max(puck, -groove);
    return closer(vec2<f32>(rail, 1.0), vec2<f32>(puck, 0.0));
}

fn map_toggle(p: vec3<f32>, value: f32, press: f32) -> vec2<f32> {
    let outline = abs(sd_segment_2d(p.xz, vec2<f32>(-0.26, 0.0), vec2<f32>(0.26, 0.0)) - 0.30);
    let track = length(vec2<f32>(outline, p.y - 0.040)) - 0.026;
    let x = mix(-0.26, 0.26, value);
    let lift = 0.160 - press * 0.012;
    let puck = sd_round_box(
        p - vec3<f32>(x, lift, 0.0),
        vec3<f32>(0.30, 0.115, 0.255),
        0.115,
    );
    return closer(vec2<f32>(track, 1.0), vec2<f32>(puck, 0.0));
}

fn map_button(p: vec3<f32>, press: f32) -> vec2<f32> {
    let body = sd_round_box(
        p - vec3<f32>(0.0, 0.185 - press * 0.070, 0.0),
        vec3<f32>(0.42, 0.135, 0.275),
        0.135,
    );
    return vec2<f32>(body, 0.0);
}

fn map_dial(p: vec3<f32>, value: f32, press: f32) -> vec2<f32> {
    let centre = p - vec3<f32>(0.0, 0.168 - press * 0.022, 0.0);
    let knob = crowned(
        sd_round_cylinder(centre, 0.42, 0.168, 0.072),
        centre,
        0.168,
        1.9,
    );
    let angle = mix(-2.36, 2.36, value);
    let q = rot_y(centre, angle);
    let slot = sd_round_box(
        q - vec3<f32>(0.0, 0.13, -0.285),
        vec3<f32>(0.032, 0.09, 0.135),
        0.024,
    );
    return vec2<f32>(max(knob, -slot), 0.0);
}

fn map_lever(p_in: vec3<f32>, value: f32, press: f32) -> vec2<f32> {
    let p = p_in + vec3<f32>(0.0, press * 0.018, 0.0);
    let collar = sd_round_cylinder(p - vec3<f32>(0.0, 0.048, 0.0), 0.27, 0.048, 0.044);
    let dome = sd_sphere(p - vec3<f32>(0.0, -0.02, 0.0), 0.19);
    let angle = mix(-0.52, 0.52, value);
    let q = rot_z(p - vec3<f32>(0.0, 0.09, 0.0), angle);
    let stick = sd_capsule(q, vec3<f32>(0.0), vec3<f32>(0.0, 0.40, 0.0), 0.05);
    let ball = sd_sphere(q - vec3<f32>(0.0, 0.45, 0.0), 0.105);
    var d = smooth_min(stick, dome, 0.07);
    d = min(d, ball);
    return closer(vec2<f32>(d, 0.0), vec2<f32>(collar, 1.0));
}

fn map(p: vec3<f32>) -> vec2<f32> {
    let kind = scene_kind();
    let value = get_float(1u);
    let press = get_float(2u);
    if (kind == 0) {
        return map_check(p, value, press);
    }
    if (kind == 1) {
        return map_slider(p, value, press);
    }
    if (kind == 2) {
        return map_toggle(p, value, press);
    }
    if (kind == 3) {
        return map_button(p, press);
    }
    if (kind == 4) {
        return map_dial(p, value, press);
    }
    return map_lever(p, value, press);
}

fn scene_normal(p: vec3<f32>) -> vec3<f32> {
    let e = vec2<f32>(1.0, -1.0) * 0.0016;
    return normalize(
        e.xyy * map(p + e.xyy).x + e.yyx * map(p + e.yyx).x + e.yxy * map(p + e.yxy).x
            + e.xxx * map(p + e.xxx).x,
    );
}

fn ambient_occlusion(p: vec3<f32>, n: vec3<f32>) -> f32 {
    var occ = 0.0;
    var scale = 1.0;
    for (var i = 0; i < 4; i = i + 1) {
        let h = 0.015 + 0.09 * f32(i);
        let d = map(p + n * h).x;
        occ = occ + (h - d) * scale;
        scale = scale * 0.72;
    }
    return clamp(1.0 - 2.4 * occ, 0.0, 1.0);
}

fn soft_shadow(origin: vec3<f32>, dir: vec3<f32>) -> f32 {
    var res = 1.0;
    var t = 0.035;
    for (var i = 0; i < SHADOW_STEPS; i = i + 1) {
        let h = map(origin + dir * t).x;
        res = min(res, 12.0 * h / t);
        t = t + clamp(h, 0.018, 0.22);
        if (res < 0.005 || t > 3.4) {
            break;
        }
    }
    return clamp(res, 0.0, 1.0);
}

fn bound_span(ro: vec3<f32>, rd: vec3<f32>) -> vec2<f32> {
    let oc = ro - BOUND_CENTER;
    let b = dot(oc, rd);
    let c = dot(oc, oc) - BOUND_RADIUS * BOUND_RADIUS;
    let disc = b * b - c;
    if (disc < 0.0) {
        return vec2<f32>(-1.0, -1.0);
    }
    let s = sqrt(disc);
    return vec2<f32>(-b - s, -b + s);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
}

fn environment(dir: vec3<f32>, dark: f32) -> vec3<f32> {
    let up = clamp(dir.y, -1.0, 1.0);
    let ground = mix(vec3<f32>(0.034, 0.034, 0.038), vec3<f32>(0.006, 0.006, 0.009), dark);
    let band = mix(vec3<f32>(1.18, 1.20, 1.26), vec3<f32>(0.26, 0.29, 0.38), dark);
    let sky = mix(vec3<f32>(0.210, 0.213, 0.222), vec3<f32>(0.050, 0.054, 0.068), dark);
    let ceiling = mix(vec3<f32>(0.128, 0.130, 0.138), vec3<f32>(0.030, 0.032, 0.040), dark);
    var c = mix(ground, band, smoothstep(-0.22, 0.03, up));
    c = mix(c, sky, smoothstep(0.07, 0.36, up));
    c = mix(c, ceiling, smoothstep(0.52, 0.88, up));
    let softbox = mix(vec3<f32>(5.6, 5.5, 5.3), vec3<f32>(3.6, 3.6, 3.8), dark);
    c = c + softbox * smoothstep(0.930, 0.992, dot(dir, key_dir()));
    let fill = normalize(vec3<f32>(0.74, 0.30, 0.60));
    c = c + mix(vec3<f32>(0.85, 0.88, 0.98), vec3<f32>(0.34, 0.38, 0.50), dark)
        * smoothstep(0.90, 0.995, dot(dir, fill));
    let rim = normalize(vec3<f32>(-0.82, 0.08, -0.56));
    c = c + mix(vec3<f32>(1.10, 1.12, 1.20), vec3<f32>(0.44, 0.50, 0.66), dark)
        * smoothstep(0.88, 0.998, dot(dir, rim));
    return c;
}

fn tonemap(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn tick_mark(xz: vec2<f32>, value: f32, width: f32) -> vec2<f32> {
    let radius = length(xz);
    if (radius < 0.50 || radius > 0.66) {
        return vec2<f32>(0.0, 0.0);
    }
    let angle = atan2(xz.x, -xz.y);
    let span = 2.36;
    if (abs(angle) > span + 0.06) {
        return vec2<f32>(0.0, 0.0);
    }
    let steps = 10.0;
    let slot = clamp(round((angle + span) / (2.0 * span) * steps), 0.0, steps);
    let centre = slot / steps * 2.0 * span - span;
    let arc = abs(angle - centre) * radius;
    let mask = 1.0 - smoothstep(0.007, 0.007 + width, arc);
    let filled = step(slot / steps, value + 0.001);
    return vec2<f32>(mask, filled);
}

fn lever_mark(xz: vec2<f32>, value: f32, width: f32) -> vec2<f32> {
    let off = sd_round_rect_2d(xz - vec2<f32>(-0.50, -0.02), vec2<f32>(0.055, 0.013), 0.013);
    let on = sd_round_rect_2d(xz - vec2<f32>(0.50, -0.02), vec2<f32>(0.055, 0.013), 0.013);
    let mask_off = 1.0 - smoothstep(0.0, width, off);
    let mask_on = 1.0 - smoothstep(0.0, width, on);
    return vec2<f32>(max(mask_off, mask_on), mix(mask_off, mask_on, step(0.5, value)));
}

fn plate_distance(kind: i32, xz: vec2<f32>) -> f32 {
    if (kind == 0) {
        return sd_round_rect_2d(xz, vec2<f32>(0.74, 0.78), 0.22);
    }
    if (kind == 1) {
        return sd_round_rect_2d(xz, vec2<f32>(1.02, 0.24), 0.24);
    }
    if (kind == 2) {
        return sd_round_rect_2d(xz, vec2<f32>(0.72, 0.40), 0.38);
    }
    if (kind == 3) {
        return sd_round_rect_2d(xz, vec2<f32>(0.78, 0.52), 0.26);
    }
    if (kind == 4) {
        return sd_round_rect_2d(xz, vec2<f32>(0.78, 0.78), 0.78);
    }
    return sd_round_rect_2d(xz, vec2<f32>(0.70, 0.44), 0.22);
}

fn ground_decal(kind: i32, xz: vec2<f32>, value: f32, dark: f32, width: f32) -> vec4<f32> {
    let w = max(width, 0.0016);
    let d = plate_distance(kind, xz);
    let fill = (1.0 - smoothstep(-w, w, d)) * mix(0.18, 0.07, dark);
    let edge = (1.0 - smoothstep(0.004, 0.004 + w * 2.5, abs(d))) * mix(0.24, 0.22, dark);
    let plate_alpha = clamp(fill + edge * (1.0 - fill), 0.0, 1.0);
    let plate_colour = mix(vec3<f32>(1.0, 1.0, 1.0), vec3<f32>(0.74, 0.78, 0.88), dark);

    var mark = vec2<f32>(0.0, 0.0);
    if (kind == 4) {
        mark = tick_mark(xz, value, w * 2.0 + 0.004);
    } else if (kind == 5) {
        mark = lever_mark(xz, value, w * 2.0 + 0.004);
    }
    if (mark.x <= 0.0) {
        return vec4<f32>(plate_colour, plate_alpha);
    }

    let idle = mix(vec3<f32>(0.55, 0.56, 0.59), vec3<f32>(0.36, 0.38, 0.44), dark);
    let live = mix(vec3<f32>(0.10, 0.11, 0.13), vec3<f32>(0.88, 0.90, 0.96), dark);
    let mark_colour = mix(idle, live, mark.y);
    let mark_alpha = clamp(mark.x * mix(0.8, 0.92, mark.y), 0.0, 1.0);
    let alpha = mark_alpha + plate_alpha * (1.0 - mark_alpha);
    let colour = (mark_colour * mark_alpha + plate_colour * plate_alpha * (1.0 - mark_alpha))
        / max(alpha, 0.0001);
    return vec4<f32>(colour, alpha);
}

fn ground_linear(kind: i32, xz: vec2<f32>, value: f32, dark: f32, background: vec3<f32>) -> vec3<f32> {
    let decal = ground_decal(kind, xz, value, dark, 0.012);
    return srgb_to_linear(mix(background, decal.rgb, decal.a));
}

fn power_glyph(icon: vec2<f32>) -> f32 {
    let ring = abs(length(icon) - 0.082) - 0.013;
    let gap = sd_round_rect_2d(icon - vec2<f32>(0.0, 0.075), vec2<f32>(0.026, 0.075), 0.0);
    let bar = sd_round_rect_2d(icon - vec2<f32>(0.0, 0.052), vec2<f32>(0.012, 0.055), 0.012);
    return min(max(ring, -gap), bar);
}

fn shade_surface(
    p: vec3<f32>,
    n: vec3<f32>,
    rd: vec3<f32>,
    material: f32,
    kind: i32,
    value: f32,
    dark: f32,
    background: vec3<f32>,
) -> vec3<f32> {
    let r = reflect(rd, n);
    var col = environment(r, dark);
    if (r.y < -0.02) {
        let hit = p.xz + r.xz * (p.y / max(-r.y, 1e-4));
        let ground = ground_linear(kind, hit, value, dark, background) * mix(0.62, 0.40, dark);
        let fade = clamp(-r.y * 2.4, 0.0, 1.0) * clamp(2.0 - 0.7 * length(hit), 0.0, 1.0);
        col = mix(col, ground, clamp(fade, 0.0, 0.92));
    }

    var tint = mix(vec3<f32>(0.94, 0.95, 0.97), vec3<f32>(0.62, 0.64, 0.68), material);
    if (kind == 3 && material < 0.5 && n.y > 0.86) {
        let mask = 1.0 - smoothstep(0.0, 0.006, power_glyph(vec2<f32>(p.x, -p.z)));
        tint = mix(tint, vec3<f32>(0.10, 0.11, 0.13), mask);
    }

    let occlusion = ambient_occlusion(p, n);
    let shadow = soft_shadow(p + n * 0.006, key_dir());
    let fresnel = 0.90 + 0.10 * pow(1.0 - clamp(dot(n, -rd), 0.0, 1.0), 5.0);
    return col * tint * fresnel * mix(0.34, 1.0, occlusion) * mix(0.66, 1.0, shadow);
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let effect_rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let size_px = max(effect_rect.zw, vec2<f32>(1.0));
    let unit = min(size_px.y, size_px.x / max(get_float(19u), 0.1));
    let local_px = input.uv * tex_size - effect_rect.xy;
    let offset = (local_px - size_px * 0.5) / unit;
    let screen = vec2<f32>(offset.x, -offset.y);

    let kind = scene_kind();
    let value = get_float(1u);
    let dark = get_float(4u);
    let yaw = get_float(12u) + get_float(5u);
    let pitch = clamp(get_float(13u) + get_float(6u), 0.14, 1.30);
    let distance = get_float(14u);
    let focal = get_float(15u);
    let background = vec3<f32>(get_float(8u), get_float(9u), get_float(10u));

    let look_at = vec3<f32>(0.0, get_float(16u), 0.0);
    let ro = look_at
        + vec3<f32>(sin(yaw) * cos(pitch), sin(pitch), cos(yaw) * cos(pitch)) * distance;
    let fwd = normalize(look_at - ro);
    let right = normalize(cross(fwd, vec3<f32>(0.0, 1.0, 0.0)));
    let up = cross(right, fwd);
    let rd = normalize(screen.x * right + screen.y * up + focal * fwd);
    let pixel_scale = 0.5 / (focal * unit);

    var ground = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if (rd.y < -0.0005) {
        let t_ground = -ro.y / rd.y;
        if (t_ground < MAX_DIST) {
            let hit = ro + rd * t_ground;
            let width = t_ground * pixel_scale / max(-rd.y, 0.12);
            let decal = ground_decal(kind, hit.xz, value, dark, width);
            var lit = 1.0;
            if (length(hit.xz) < 2.1) {
                let raw = soft_shadow(hit + vec3<f32>(0.0, 0.004, 0.0), key_dir());
                lit = mix(1.0, raw, mix(0.44, 0.52, dark));
            }
            ground = vec4<f32>(
                decal.rgb * decal.a * lit,
                1.0 - lit * (1.0 - decal.a),
            );
        }
    }

    var object = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    let bounds = bound_span(ro, rd);
    if (bounds.y > 0.0) {
        var t = max(bounds.x, 0.02);
        let t_max = min(bounds.y, MAX_DIST);
        var hit = false;
        var coverage = 1.0;
        var best_t = t;
        var material = 0.0;
        for (var i = 0; i < MAX_STEPS; i = i + 1) {
            if (t > t_max) {
                break;
            }
            let probe = map(ro + rd * t);
            let radius = max(t * pixel_scale, 1e-5);
            let ratio = probe.x / radius;
            if (ratio < coverage) {
                coverage = ratio;
                best_t = t;
                material = probe.y;
            }
            if (ratio < 0.35) {
                hit = true;
                break;
            }
            t = t + max(probe.x * 0.8, radius * 0.35);
        }

        var alpha = 0.0;
        if (hit) {
            alpha = 1.0;
        } else {
            alpha = clamp(1.0 - coverage, 0.0, 1.0);
        }
        if (alpha > 0.002) {
            let p = ro + rd * best_t;
            let n = scene_normal(p);
            let lit = shade_surface(p, n, rd, material, kind, value, dark, background);
            object = vec4<f32>(linear_to_srgb(tonemap(lit)) * alpha, alpha);
        }
    }

    let composed = object + ground * (1.0 - object.a);
    let base = textureSample(input_texture, input_sampler, input.uv);
    return composed + base * (1.0 - composed.a);
}
