// Cotton card stock, after `noise.wgsl`: the content printed on paper with
// a fine tooth lit from the upper left, stray fibres and a faint
// cloudiness. The content marks a selection with the marker's colour; the
// paper shows it as a stroke of red highlighter whose edge wobbles from one
// row of the stroke to the next, with the selected ink printed dark red.
//
// params: 0..1 = the card's size in logical pixels; 4..6 = the paper's
// colour in the content; 8..10 = the ink's; 12..14 = the marker's.

const SELECTED_INK: vec3<f32> = vec3<f32>(0.10, 0.004, 0.014);
const HIGHLIGHTER: vec3<f32> = vec3<f32>(1.0, 0.33, 0.30);
/// How tall a row of the highlighter's stroke is, in logical pixels.
const STROKE_ROW: f32 = 2.2;

fn param3(first: u32) -> vec3<f32> {
    return vec3<f32>(shaded_param(first), shaded_param(first + 1u), shaded_param(first + 2u));
}

// How much ink and how much marker lie on the paper in `color`.
fn inks(color: vec4<f32>) -> vec2<f32> {
    let paper = param3(4u);
    let to_ink = param3(8u) - paper;
    let to_marker = param3(12u) - paper;
    let from_paper = color.rgb / max(color.a, 1.0 / 255.0) - paper;
    let a = dot(to_ink, to_ink);
    let b = dot(to_ink, to_marker);
    let c = dot(to_marker, to_marker);
    let p = dot(to_ink, from_paper);
    let q = dot(to_marker, from_paper);
    let shares = vec2<f32>(c * p - b * q, a * q - b * p) / max(a * c - b * b, 1.0e-6);
    return clamp(shares, vec2<f32>(0.0), vec2<f32>(1.0));
}

fn marker_at(point: vec2<f32>, card: vec2<f32>) -> f32 {
    return inks(content_color(point / card)).y;
}

fn tooth(g: vec2<f32>) -> f32 {
    return value_noise(g) + 0.6 * value_noise(g * 2.7 + 9.0);
}

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let alpha = surface.color.a;
    let card = vec2<f32>(shaded_param(0u), shaded_param(1u));
    let point = surface.uv * card;
    let ink = inks(surface.color).x;

    // The highlighter's edge wobbles from row to row. Ink inside a
    // selection is marked too: the marker lies on both sides of it.
    let row = floor(point.y / STROKE_ROW);
    let at = point + vec2<f32>((hash(vec2<f32>(row, 5.0)) - 0.5) * 3.0, 0.0);
    let across = vec2<f32>(5.0, 0.0);
    let down = vec2<f32>(0.0, 4.0);
    let marked = max(
        marker_at(at, card),
        max(
            min(marker_at(at - across, card), marker_at(at + across, card)),
            min(marker_at(at - down, card), marker_at(at + down, card)),
        ),
    );

    let g = point * 0.42;
    let relief = tooth(g + vec2<f32>(0.55, 0.75)) - tooth(g);
    let fibre = smoothstep(
        0.55,
        0.8,
        value_noise(vec2<f32>(point.x * 0.16 + point.y * 0.05, point.y * 0.9) + 40.0)
            * value_noise(point.yx * vec2<f32>(0.7, 0.12) + 11.0),
    );
    let cloud = value_noise(point * 0.022) + 0.5 * value_noise(point * 0.05 + 3.0) - 0.75;
    var paper = param3(4u) * (1.0 + 0.04 * relief - 0.02 * fibre + 0.012 * cloud);
    let streak = 0.78 + 0.3 * value_noise(vec2<f32>(point.x * 0.012, point.y * 0.62))
        + 0.1 * value_noise(point * 0.2);
    paper = mix(paper, paper * mix(vec3<f32>(1.0), HIGHLIGHTER * streak, 0.9), marked);
    var color = mix(paper, mix(param3(8u), SELECTED_INK, marked), ink);

    // The top edge catches the light; the right and lower edges turn away.
    let top = smoothstep(1.6, 0.0, point.y);
    let turned = smoothstep(2.0, 0.0, min(card.x - point.x, card.y - point.y));
    color = (color + vec3<f32>(0.06) * top) * (1.0 - 0.10 * turned);

    let lit = max(dot(surface.normal, surface.to_light), 0.0) * surface.lit;
    let diffuse = surface.ambient + (1.0 - surface.ambient) * lit;
    return vec4<f32>(color * diffuse * alpha, alpha);
}
