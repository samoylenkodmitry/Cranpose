// The drawing board's face, after `noise.wgsl` and `sheet_paper.wgsl`: the
// flat copy of the paragraph on drafting film, taped at its corners to a
// frame of light timber, with the boxes ruled round its lines and words in
// blue and, for a moment, the corners of a view finder round the sheet.
//
// The mesh covers the board's face and a band round it, where only the
// finder shows; its texture coordinates span the sheet, so the timber lies
// outside 0..1.
//
// params: 0..1 = the sheet's size in logical pixels; 2 = the unit; 3 = how
// strong the ruled boxes show; 4 = the wood round the sheet at the sides and
// top; 5 = at the bottom; 6 = how strong the finder shows; 7 = how far out
// of the sheet its corners stand.

const TAPE: vec3<f32> = vec3<f32>(0.80, 0.675, 0.41);
const FINDER: vec3<f32> = vec3<f32>(1.0, 0.95, 0.82);

fn wood_at(w: vec2<f32>) -> f32 {
    let warp = value_noise(w * vec2<f32>(3.0 / 1300.0, 5.0 / 520.0)) * 2.2;
    return value_noise(vec2<f32>(w.x * 2.0 / 1300.0, w.y * 22.0 / 520.0 + warp * 1.6)) * 0.4
        + value_noise(vec2<f32>(w.x * 5.0 / 1300.0, w.y * 64.0 / 520.0 + warp * 2.2)) * 0.3
        + value_noise(vec2<f32>(w.x * 24.0 / 1300.0, w.y * 256.0 / 520.0)) * 0.3;
}

// The timber at `q`, sheet pixels from the sheet's corner; `edge` is the
// distance to the board's outline, which is `half` from its `middle`.
fn timber(q: vec2<f32>, sheet: vec2<f32>, u: f32, edge: f32, middle: vec2<f32>, half: vec2<f32>) -> vec3<f32> {
    // The side pieces run upright: their grain turns a quarter.
    let stile = (q.x < 0.0) | (q.x > sheet.x);
    let offset = select(vec2<f32>(0.0), vec2<f32>(311.0, select(613.0, 97.0, q.x < 0.0)) * u, stile);
    let w = (select(q, q.yx, stile) + offset) / u;
    let rings = wood_at(w);
    let pores = value_noise(w * vec2<f32>(0.11, 0.6));
    var wood = mix(vec3<f32>(0.60, 0.40, 0.215), vec3<f32>(0.765, 0.57, 0.355), smoothstep(0.2, 0.7, rings))
        * (0.955 + 0.09 * pores) * select(1.0, 0.985, stile);
    // The dark joint between a side piece and the middle.
    let to_joint = min(abs(q.x), abs(q.x - sheet.x));
    wood *= 1.0 - 0.42 * smoothstep(1.3 * u + 1.0, 0.2 * u, to_joint);
    // Light from the upper right: a gentle gradient, a bright bevel there
    // and a dark one down on the left.
    wood *= 1.03 - 0.10 * ((sheet.x - q.x) + q.y) / (sheet.x + sheet.y);
    let away = (q.y - middle.y) / half.y - (q.x - middle.x) / half.x;
    let bevel = (edge + 1.6 * u) / (1.2 * u);
    wood += vec3<f32>(0.16 * exp(-bevel * bevel) * smoothstep(0.0, -1.0, away));
    wood *= 1.0 - 0.45 * smoothstep(-3.2 * u, 0.0, edge) * smoothstep(-0.3, 1.0, away);
    // Toward the foot: darker, and tinted by the green table.
    let to_foot = middle.y + half.y - q.y;
    wood *= 1.0 - 0.5 * smoothstep(16.0 * u, 7.0 * u, to_foot);
    wood *= mix(vec3<f32>(1.0), vec3<f32>(0.62, 0.86, 0.72), smoothstep(60.0 * u, 0.0, to_foot) * 0.45);
    // The sheet's soft shadow on the wood, and a tight dark line at its edge.
    let sheet_half = sheet * 0.5;
    wood *= 1.0 - 0.62 * smoothstep(9.0 * u, -5.0 * u, round_box(q - sheet_half - vec2<f32>(-3.0, 5.0) * u, sheet_half, 2.0 * u));
    wood *= 1.0 - 0.35 * smoothstep(1.6 * u, -1.0 * u, round_box(q - sheet_half, sheet_half, 1.0 * u));
    return wood;
}

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let sheet = vec2<f32>(shaded_param(0u), shaded_param(1u));
    let u = shaded_param(2u);
    let guides = shaded_param(3u);
    let margin = vec2<f32>(shaded_param(4u), shaded_param(5u));
    let q = surface.uv * sheet;
    let footprint = fwidth(q);
    let px = max(footprint.x, footprint.y);
    let content = surface.color.rgb / max(surface.color.a, 1.0 / 255.0);

    let low = vec2<f32>(-margin.x);
    let high = sheet + margin;
    let middle = (low + high) * 0.5;
    let half = (high - low) * 0.5;
    let edge = round_box(q - middle, half, 9.0 * u);
    let board = clamp(0.5 - edge / px, 0.0, 1.0);

    let wood = timber(q, sheet, u, edge, middle, half);
    let sheet_half = sheet * 0.5;
    let sheet_edge = round_box(q - sheet_half, sheet_half, 0.0);
    let on_sheet = clamp(0.5 - sheet_edge / px, 0.0, 1.0);
    var film = drafting_film(q, u, footprint);
    film *= 1.06 - 0.13 * ((1.0 - q.x / sheet.x) * 0.4 + q.y / sheet.y * 0.6);
    film *= mix(vec3<f32>(1.0), vec3<f32>(0.86, 0.97, 0.90), smoothstep(0.55, 1.0, q.y / sheet.y) * 0.5);
    film *= 1.0 - 0.06 * smoothstep(3.0 * u, 0.0, -sheet_edge);
    var paper = film * without_guides(content);
    paper = mix(paper, to_linear(INK_BLUE), guide_in(content) * guides);
    var face = mix(wood, paper, on_sheet);

    // Tape: the four corners folded onto one and turned a quarter, so the
    // strip lies across the corner.
    let from_corner = abs(q - sheet_half) - sheet_half;
    let side = sign(q - sheet_half);
    let skew = 0.06 * side.x * side.y + select(-0.03, 0.05, side.x > 0.0);
    var t = vec2<f32>(from_corner.x + from_corner.y, from_corner.y - from_corner.x) * 0.70711;
    t += t.yx * vec2<f32>(skew, -skew);
    t.x += 5.0 * u;
    let weave = value_noise(t / u / vec2<f32>(2.0, 14.0));
    let tear = (value_noise(t.xx / u / 3.0) - 0.5) * 5.0 * u;
    let tape_shadow = round_box(t - vec2<f32>(0.6, 2.4) * u, vec2<f32>(10.0, 36.0) * u, 1.0 * u);
    face *= 1.0 - 0.34 * smoothstep(3.5 * u, -2.0 * u, tape_shadow);
    let tape_cover = clamp(0.5 - round_box(t, vec2<f32>(10.0 * u, 36.0 * u + tear), 0.5 * u) / px, 0.0, 1.0);
    let tape = TAPE * (0.90 + 0.20 * weave) * (1.04 - 0.12 * smoothstep(-10.0, 10.0, t.x / u));
    face = mix(face, mix(face, tape, 0.86), tape_cover);

    // The view finder's corners, cream, round the sheet, out over the
    // table too: the mesh reaches past the board for them.
    let finder = shaded_param(6u);
    var finder_cover = 0.0;
    if (finder > 0.0) {
        let stand_out = shaded_param(7u);
        let arm = 46.0 * u;
        let reach = vec2<f32>(sheet.x, sheet.y + margin.y - margin.x) * 0.5 + vec2<f32>(stand_out);
        let from_middle = abs(q - vec2<f32>(sheet_half.x, (sheet.y + margin.y - margin.x) * 0.5));
        let along = reach - from_middle;
        let width = 2.0 * u;
        let arms = max(
            step(-width, along.x) * step(along.x, arm) * clamp((width - abs(along.y)) / px + 0.5, 0.0, 1.0),
            step(-width, along.y) * step(along.y, arm) * clamp((width - abs(along.x)) / px + 0.5, 0.0, 1.0),
        );
        finder_cover = arms * finder;
    }
    let cover = max(board, on_sheet);
    let shown = mix(to_srgb(face) * cover, to_srgb(to_linear(FINDER)), finder_cover);
    return vec4<f32>(shown, max(cover, finder_cover));
}
