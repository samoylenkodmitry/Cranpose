// The cord that ties a spot of the bent copy to the same spot of the flat
// copy, after `sheet_paper.wgsl`: a twisted orange thread, shaded round,
// with a bright pulse running along it from the print to the board.
//
// The mesh is a tube in world space; its texture runs along it in u and
// round it in v.
//
// params: 0 = how much of the cord shows; 1 = where the pulse is, from 0 at
// the print to 1 at the board, below 0 for none; 2 = the cord's length in
// logical pixels; 3 = the unit; 4..5 = how far along it the words of the
// print's line end and those of the board's start: over them it is faint.

const ORANGE: vec3<f32> = vec3<f32>(1.0, 0.130, 0.012);

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let show = shaded_param(0u);
    let pulse_at = shaded_param(1u);
    let length_px = shaded_param(2u);
    let u = shaded_param(3u);
    let along = surface.uv.x * length_px;
    let facing = clamp(dot(surface.normal, surface.to_eye), 0.0, 1.0);
    let twist = 0.93 + 0.07 * sin(along / u * 1.9 + surface.uv.y * 13.8);
    let lit = max(dot(surface.normal, surface.to_light), 0.0) * surface.lit;
    var color = ORANGE * (0.50 + 0.62 * facing + 0.35 * lit) * twist;
    color += vec3<f32>(0.30) * pow(facing, 24.0);
    let from_pulse = (along - pulse_at * length_px) / u;
    let tail = select(90.0, 620.0, from_pulse < 0.0);
    let pulse = select(0.0, exp(-from_pulse * from_pulse / tail), pulse_at >= 0.0);
    color = mix(color, vec3<f32>(1.0, 0.93, 0.62) * 1.25, pulse * 0.92);
    let over_words = (along < shaded_param(4u)) | (along > shaded_param(5u));
    let alpha = show * select(1.0, 0.15 + 0.85 * pulse, over_words);
    return vec4<f32>(to_srgb(color) * alpha, alpha);
}
