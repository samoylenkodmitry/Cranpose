// The glossy photographic print, after `noise.wgsl` and `sheet_paper.wgsl`:
// a white border round the bent copy of the paragraph, lit by the sky, the
// green bounce of the table and the window, with a softbox mirrored in its
// skin. While it bends a wire mesh shows on the picture.
//
// The mesh is in world space, y up. Its texture coordinates span the sheet,
// so the border lies outside 0..1.
//
// params: 0..1 = the sheet's size in logical pixels; 2 = the unit; 3 = how
// much of the print shows; 4 = the border at the sides and top; 5 = at the
// bottom; 6 = how far it has bent; 7 = how strong the wire mesh shows.

const COBALT: vec3<f32> = vec3<f32>(0.0137, 0.0782, 0.745);

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let sheet = vec2<f32>(shaded_param(0u), shaded_param(1u));
    let u = shaded_param(2u);
    let show = shaded_param(3u);
    let margin_bottom = shaded_param(5u);
    let bend = shaded_param(6u);
    let wires_show = shaded_param(7u);
    let q = surface.uv * sheet;
    let footprint = max(fwidth(q), vec2<f32>(1.0e-4));
    let content = surface.color.rgb / max(surface.color.a, 1.0 / 255.0);

    // The picture inside the white border.
    let inside = min(q, sheet - q);
    let picture = clamp(min(inside.x / footprint.x, inside.y / footprint.y) + 0.5, 0.0, 1.0);
    var pic = drafting_film(q, u, footprint) * vec3<f32>(0.955, 0.955, 0.95);
    // 18 by 12 quads, each split by a diagonal, as a wireframe shows them.
    let cell = sheet / vec2<f32>(18.0, 12.0);
    let g = q / cell;
    let gw = footprint / cell;
    let half_line = 0.5 + 0.42 * u;
    let wires = clamp(vec2<f32>(half_line) - abs(fract(g - 0.5) - 0.5) / gw, vec2<f32>(0.0), vec2<f32>(1.0));
    let diagonal = clamp(half_line - abs(fract(g.x + g.y - 0.5) - 0.5) / (gw.x + gw.y), 0.0, 1.0) * 0.7;
    pic = mix(pic, COBALT, max(max(wires.x, wires.y), diagonal) * wires_show);
    let printed = without_guides(content);
    let ink = clamp(1.0 - dot(printed, vec3<f32>(0.3333)) * 1.6, 0.0, 1.0);
    pic *= printed;
    var color = mix(vec3<f32>(0.985, 0.985, 0.975), pic, picture);
    // A faint dark line where the picture meets the border.
    let nearest_edge = min(inside.x, inside.y);
    color *= 1.0 - 0.10 * (1.0 - smoothstep(0.0, 1.6 * u, abs(nearest_edge))) * step(nearest_edge, 0.0);

    // Sky above, the table's green bounce near the foot with a dark contact
    // line, and the window.
    let n = surface.normal;
    let height = (sheet.y + margin_bottom - q.y) / u;
    let window = max(dot(n, surface.to_light), 0.0) * mix(1.0, surface.lit, 0.7);
    var ambient = mix(vec3<f32>(0.345, 0.370, 0.365), vec3<f32>(0.235, 0.365, 0.285), exp(-height / 130.0));
    ambient *= 1.0 - 0.35 * exp(-height / 9.0);
    let light = ambient + vec3<f32>(1.04, 0.99, 0.90) * mix(0.62, window * 1.02, 0.25 + 0.75 * bend) * 1.06;
    color *= light;

    // A glossy skin: one broad softbox and a green kick from the table, both
    // kept off the ink so the words stay dark.
    let r = reflect(-surface.to_eye, normalize(n + vec3<f32>(0.0, 0.30, 0.0)));
    let azimuth = atan2(r.x, r.z);
    let elevation = asin(clamp(r.y, -1.0, 1.0));
    let facing = clamp(dot(n, surface.to_eye), 0.0, 1.0);
    let fresnel = 0.04 + 0.96 * pow(1.0 - facing, 5.0);
    let on_paper = 1.0 - ink * 0.93;
    let softbox = smoothstep(1.45, 0.72, azimuth) * smoothstep(-0.08, 0.52, azimuth) * smoothstep(-0.30, 0.10, elevation);
    color += vec3<f32>(1.0, 0.985, 0.95) * (softbox * 2.5 * fresnel + fresnel * 0.2) * on_paper * mix(0.5, 1.0, bend);
    let kick = smoothstep(-0.42, -0.75, azimuth) * smoothstep(-1.25, -0.95, azimuth);
    color += vec3<f32>(0.55, 1.0, 0.75) * kick * fresnel * 1.2 * on_paper * bend;

    // Highlights roll off softly instead of clipping.
    color = mix(color, vec3<f32>(1.0) - 0.22 * exp(-(color - vec3<f32>(0.78)) / 0.22), step(vec3<f32>(0.78), color));
    return vec4<f32>(to_srgb(color) * show, show);
}
