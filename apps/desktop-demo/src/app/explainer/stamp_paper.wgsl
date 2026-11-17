// Stamp paper, after `noise.wgsl`: cream paper with a fine tooth and stray
// fibres, a picture printed on it, names printed in dark ink. A stamp curls
// up along its peel; where its corner has turned over, the gummed back
// shows, glossy where the light catches it. The content marks a selection
// in the marker's colour; the paper shows it as a red postmark, wavy bars
// that cancel the selected words.
//
// params: 0..1 = the content's size in logical pixels; 2 = how far along
// the peel the paper stays glued down, 3 = how far its free corner turns,
// 4 = the length of the peel across the stamp, 5..6 = the peel's
// direction; 8..11 = the picture's box in content pixels (zero size for
// none); 12..14 = the paper's colour in the content, 16..18 = the ink's,
// 20..22 = the marker's; 24 = how tall a line of the printed words is.

const POSTMARK: vec3<f32> = vec3<f32>(0.70, 0.04, 0.03);
const GUM: vec3<f32> = vec3<f32>(0.90, 0.80, 0.62);

fn param3(first: u32) -> vec3<f32> {
    return vec3<f32>(shaded_param(first), shaded_param(first + 1u), shaded_param(first + 2u));
}

// How much ink and how much marker lie on the paper in `color`.
fn inks(color: vec4<f32>) -> vec2<f32> {
    let paper = param3(12u);
    let to_ink = param3(16u) - paper;
    let to_marker = param3(20u) - paper;
    let from_paper = color.rgb / max(color.a, 1.0 / 255.0) - paper;
    let a = dot(to_ink, to_ink);
    let b = dot(to_ink, to_marker);
    let c = dot(to_marker, to_marker);
    let p = dot(to_ink, from_paper);
    let q = dot(to_marker, from_paper);
    let shares = vec2<f32>(c * p - b * q, a * q - b * p) / max(a * c - b * b, 1.0e-6);
    return clamp(shares, vec2<f32>(0.0), vec2<f32>(1.0));
}

// The angle the paper has turned through `s` along the peel.
fn curl_angle(s: f32) -> f32 {
    let attached = shaded_param(2u);
    let diagonal = shaded_param(4u);
    let a = clamp((s - attached) / max(diagonal - attached, 1.0e-3), 0.0, 1.0);
    return shaded_param(3u) * (0.3 * a + 0.7 * a * a * a);
}

// The room's window as the varnish mirrors it: a soft tilted rectangle with
// a cross of glazing bars.
fn window_glare(e_in: vec2<f32>) -> f32 {
    let e = mat2x2<f32>(0.94, 0.34, -0.34, 0.94) * (e_in - vec2<f32>(-0.40, -0.36));
    let a = abs(e) - vec2<f32>(0.24, 0.30);
    let pane = 1.0 - smoothstep(-0.07, 0.09, max(a.x, a.y));
    let bars = smoothstep(0.006, 0.03, abs(e.x)) * smoothstep(0.006, 0.03, abs(e.y + 0.06));
    return pane * mix(0.35, 1.0, bars);
}

// Wavy cancelling bars, worn like a rubber stamp not pressed flat.
fn postmark(point: vec2<f32>, line: f32) -> f32 {
    let y = point.y + line * 0.06 * sin(point.x / line * 4.4);
    let pitch = line / 5.2;
    let from_bar = abs(fract(y / pitch) - 0.5) * pitch;
    let bars = 1.0 - smoothstep(pitch * 0.18, pitch * 0.26, from_bar);
    let wear = value_noise(point * 9.0 / max(line, 16.0) + 3.1);
    let blotch = value_noise(point * 1.1 + 17.0);
    return bars * mix(0.45, 1.0, smoothstep(0.22, 0.5, wear * 0.62 + blotch * 0.38));
}

fn fibres(p: vec2<f32>) -> f32 {
    var sum = 0.0;
    for (var i = 0; i < 3; i++) {
        let a = f32(i) * 0.9 + 0.3;
        var q = mat2x2<f32>(cos(a), sin(a), -sin(a), cos(a)) * p;
        q.y += 3.0 * value_noise(q * 0.09 + f32(i) * 9.0);
        let streak = smoothstep(0.80, 0.97, value_noise(q * vec2<f32>(0.075, 1.1) + f32(i) * 31.0));
        sum += streak * select(-0.8, 1.0, value_noise(q * 0.05 + 40.0) > 0.5);
    }
    return sum;
}

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let alpha = surface.color.a;
    let size = vec2<f32>(shaded_param(0u), shaded_param(1u));
    let point = surface.uv * size;
    let peel = vec2<f32>(shaded_param(5u), shaded_param(6u));
    let angle = curl_angle(dot(point, peel));
    // The face's own normal: the paper turns about the axis across the peel.
    let face = vec3<f32>(peel * sin(angle), -cos(angle));
    let facing = max(dot(surface.normal, surface.to_light), 0.0);
    // A shadow from a stamp lifted over the paper falls darker than the
    // ambient share alone would make it.
    let light = (surface.ambient + (1.0 - surface.ambient) * facing) * mix(0.55, 1.0, surface.lit);

    if (dot(face, surface.to_eye) < 0.0) {
        // The gummed back: dried gum is glossy, one band of highlight.
        let grain = (value_noise(point * 1.7) - 0.5) * 0.05 + (value_noise(point * 0.06) - 0.5) * 0.12;
        var back = GUM * (0.55 + 0.45 * light) * (1.0 + grain);
        back += vec3<f32>(1.0, 0.95, 0.85) * pow(max(0.0, sin(angle - 0.35)), 10.0) * 0.22;
        return vec4<f32>(back * alpha, alpha);
    }

    let picture_box = vec4<f32>(shaded_param(8u), shaded_param(9u), shaded_param(10u), shaded_param(11u));
    let in_picture = step(picture_box.x, point.x) * step(picture_box.y, point.y)
        * step(point.x, picture_box.x + picture_box.z) * step(point.y, picture_box.y + picture_box.w);
    let shares = inks(surface.color);
    let line = max(shaded_param(24u), 1.0);
    let marked = shares.y * (1.0 - in_picture);

    let tooth = (value_noise(point * 2.1) - 0.5) * 0.022 + (hash(floor(point * 2.0)) - 0.5) * 0.014;
    let mottle = (value_noise(point * 0.03 + 8.0) - 0.5) * 0.03;
    var paper = param3(12u) * (1.0 + fibres(point * 1.15) * 0.02 + tooth + mottle);
    let cancel = postmark(point, line) * marked;
    paper = mix(paper, POSTMARK, cancel * 0.9);
    let ink = mix(param3(16u), mix(param3(16u), POSTMARK * 0.6, 0.8), shares.y);
    var color = mix(paper, ink, shares.x * (1.0 - in_picture));
    // The picture, printed as it is.
    let printed = surface.color.rgb / max(alpha, 1.0 / 255.0);
    color = mix(color, printed * (1.0 + tooth * 2.0), in_picture);

    // The curl swings the varnish's reflection across the room's window.
    let mirrored = reflect(-surface.to_eye, surface.normal);
    let glare = window_glare(mirrored.xy * 0.62) * mix(0.03, 0.115, smoothstep(0.02, 0.35, angle));
    color = color * light * mix(1.0, 1.08, smoothstep(0.0, 0.6, angle)) + vec3<f32>(1.0, 0.97, 0.9) * glare;
    return vec4<f32>(color * alpha, alpha);
}
