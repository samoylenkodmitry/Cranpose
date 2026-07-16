
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn fullscreen_vs(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;
    let x = f32(i32(vertex_index & 1u) * 2 - 1);
    let y = f32(i32(vertex_index >> 1u) * 2 - 1);
    output.uv = vec2<f32>(x * 0.5 + 0.5, 1.0 - (y * 0.5 + 0.5));
    output.position = vec4<f32>(x, y, 0.0, 1.0);
    return output;
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> u: array<vec4<f32>, 64>;

// Uniform accessors
fn get_float(index: u32) -> f32 {
    return u[index / 4u][index % 4u];
}

fn get_vec2(index: u32) -> vec2<f32> {
    return vec2<f32>(get_float(index), get_float(index + 1u));
}

fn get_vec4(index: u32) -> vec4<f32> {
    return vec4<f32>(get_float(index), get_float(index + 1u), get_float(index + 2u), get_float(index + 3u));
}

// SDF for rounded rectangle
fn sd_round_rect(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

fn sd_ellipse_approx(p: vec2<f32>, half_size: vec2<f32>) -> f32 {
    let safe_half = max(half_size, vec2<f32>(0.001));
    return (length(p / safe_half) - 1.0) * min(safe_half.x, safe_half.y);
}

struct OpticalSample {
    source_displacement: vec2<f32>,
    interior: f32,
    edge_light: f32,
    face_light: f32,
}

const WCKSRD_GRADIENT_EXTENT_DP: f32 = 1.3333334;
const WCKSRD_EDGE_EXTENT_DP: f32 = 0.33333334;

fn wcksrd_meniscus(
    distance: f32,
    lens_refraction: f32,
    gradient_extent: f32,
) -> f32 {
    let gradient_outer = clamp(
        -(distance - gradient_extent) / lens_refraction,
        0.0,
        1.0,
    );
    let gradient_inner = clamp(
        -(distance + gradient_extent) / lens_refraction,
        0.0,
        1.0,
    );
    return clamp(gradient_outer * 4.0, 0.0, 1.0)
        - clamp(gradient_inner * 4.0, 0.0, 1.0);
}

fn opposite_side_reflection_displacement(
    local_position: vec2<f32>,
    outward_normal: vec2<f32>,
    half_size: vec2<f32>,
    corner_radius: f32,
) -> vec2<f32> {
    let radius = clamp(
        corner_radius,
        0.0,
        min(half_size.x, half_size.y),
    );
    let core_half_size = max(half_size - vec2<f32>(radius), vec2<f32>(0.0));
    let opposite_support = dot(abs(outward_normal), core_half_size) + radius;
    let opposite_surface = -outward_normal * opposite_support;
    return opposite_surface - local_position;
}

// Polynomial smooth minimum — SDF metaball gluing: shapes within `k` of
// each other neck together like merging droplets.
fn smin(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 {
        return min(a, b);
    }
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

fn catmull_rom_weight(distance: f32) -> f32 {
    let x = abs(distance);
    if x <= 1.0 {
        return ((1.5 * x - 2.5) * x) * x + 1.0;
    }
    if x < 2.0 {
        return ((-0.5 * x + 2.5) * x - 4.0) * x + 2.0;
    }
    return 0.0;
}

// A refractive lookup is a resample, not a blur. A single bilinear tap loses
// the high-frequency glyph and icon detail as soon as the coordinate field
// magnifies it. Catmull-Rom reconstructs the sharp source path from the same
// backdrop texture; intentionally blurred glass continues through the 9x9
// wcKSRD footprint below.
fn sample_wcksrd_sharp_path(uv: vec2<f32>, tex_size: vec2<f32>) -> vec4<f32> {
    let sample_position = uv * tex_size - vec2<f32>(0.5);
    let base = floor(sample_position);
    let fraction = sample_position - base;
    var reconstructed = vec4<f32>(0.0);
    for (var y = -1; y <= 2; y = y + 1) {
        let weight_y = catmull_rom_weight(f32(y) - fraction.y);
        for (var x = -1; x <= 2; x = x + 1) {
            let weight_x = catmull_rom_weight(f32(x) - fraction.x);
            let sample_uv = clamp(
                (base + vec2<f32>(f32(x), f32(y)) + vec2<f32>(0.5)) / tex_size,
                vec2<f32>(0.0),
                vec2<f32>(1.0),
            );
            reconstructed = reconstructed
                + textureSampleLevel(input_texture, input_sampler, sample_uv, 0.0)
                    * weight_x
                    * weight_y;
        }
    }
    return clamp(reconstructed, vec4<f32>(0.0), vec4<f32>(1.0));
}

fn sample_wcksrd_path(
    uv: vec2<f32>,
    tex_size: vec2<f32>,
    base_displacement: vec2<f32>,
    blur_radius: f32,
    reconstruct_sharp: bool,
) -> vec4<f32> {
    let center_uv = clamp(
        uv + base_displacement / tex_size,
        vec2<f32>(0.0),
        vec2<f32>(1.0),
    );
    if blur_radius <= 0.0 {
        if reconstruct_sharp {
            return sample_wcksrd_sharp_path(center_uv, tex_size);
        }
        return textureSampleLevel(input_texture, input_sampler, center_uv, 0.0);
    }
    let blur_step = blur_radius / 4.0;
    var accumulated = vec4<f32>(0.0);
    for (var x = -4; x <= 4; x = x + 1) {
        for (var y = -4; y <= 4; y = y + 1) {
            let offset = vec2<f32>(f32(x), f32(y)) * blur_step / tex_size;
            accumulated = accumulated + textureSampleLevel(
                input_texture,
                input_sampler,
                clamp(center_uv + offset, vec2<f32>(0.0), vec2<f32>(1.0)),
                0.0,
            );
        }
    }
    return accumulated / 81.0;
}

fn sample_wcksrd_reflection_path(
    uv: vec2<f32>,
    tex_size: vec2<f32>,
    displacement: vec2<f32>,
    tangent: vec2<f32>,
    blur_radius: f32,
) -> vec4<f32> {
    let center_uv = clamp(
        uv + displacement / tex_size,
        vec2<f32>(0.0),
        vec2<f32>(1.0),
    );
    let tangent_length = length(tangent);
    let tangent_direction = select(
        vec2<f32>(1.0, 0.0),
        tangent / max(tangent_length, 0.001),
        tangent_length > 0.001,
    );
    let outer_offset = tangent_direction * blur_radius / tex_size;
    let inner_offset = outer_offset * 0.5;
    let center = textureSampleLevel(input_texture, input_sampler, center_uv, 0.0);
    let inner = textureSampleLevel(
        input_texture,
        input_sampler,
        clamp(center_uv - inner_offset, vec2<f32>(0.0), vec2<f32>(1.0)),
        0.0,
    ) + textureSampleLevel(
        input_texture,
        input_sampler,
        clamp(center_uv + inner_offset, vec2<f32>(0.0), vec2<f32>(1.0)),
        0.0,
    );
    let outer = textureSampleLevel(
        input_texture,
        input_sampler,
        clamp(center_uv - outer_offset, vec2<f32>(0.0), vec2<f32>(1.0)),
        0.0,
    ) + textureSampleLevel(
        input_texture,
        input_sampler,
        clamp(center_uv + outer_offset, vec2<f32>(0.0), vec2<f32>(1.0)),
        0.0,
    );
    return center * 0.40 + inner * 0.20 + outer * 0.10;
}

fn wcksrd_optics(
    local_position: vec2<f32>,
    sampling_position: vec2<f32>,
    half_size: vec2<f32>,
    distance: f32,
    refraction_depth: f32,
    refraction_curve: f32,
    gradient_extent: f32,
    edge_extent: f32,
) -> OpticalSample {
    let inradius = max(min(half_size.x, half_size.y), 1.0);
    let lens_refraction = max(inradius * refraction_depth, 0.001);
    let interior = clamp(-distance / lens_refraction, 0.0, 1.0);
    let lens_scale = sin(pow(interior, refraction_curve) * 1.57);
    let source_displacement = sampling_position * (lens_scale - 1.0);
    let outer_interior = clamp(
        -(distance - edge_extent) / lens_refraction,
        0.0,
        1.0,
    );
    let border = clamp(outer_interior * 16.0, 0.0, 1.0)
        - clamp(interior * 16.0, 0.0, 1.0);
    let gradient_band = wcksrd_meniscus(
        distance,
        lens_refraction,
        gradient_extent,
    );
    let source_y = -local_position.y / max(half_size.y, 1.0) * 0.29;
    let face_light = 0.5 * clamp(clamp(source_y, 0.0, 0.2) + 0.1, 0.0, 1.0)
        + 0.5 * clamp(clamp(-source_y, -1.0, 0.2) * gradient_band + 0.1, 0.0, 1.0);
    return OpticalSample(
        source_displacement,
        interior,
        border,
        face_light,
    );
}

// Cheap screen-space hash for the anti-banding dither.
fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 = p3 + dot(p3, vec3<f32>(p3.y, p3.z, p3.x) + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Extra scene shapes for the liquid field: every glass shape near another
// GLUES to it via a smooth union — the growing menu bubble necks with a
// neighboring button simply by passing near it, the drag lens merges with
// the search circle on approach. Up to 8 extra shapes at 5 floats each
// (center.xy, size.xy, radius; negative radius = capsule) from float 36 on.
const MAX_SCENE_SHAPES: u32 = 8u;

fn scene_shape_sdf(coord: vec2<f32>, base: u32, dp_scale: vec2<f32>, s: f32) -> f32 {
    let center = vec2<f32>(get_float(base), get_float(base + 1u)) * dp_scale;
    let size = vec2<f32>(get_float(base + 2u), get_float(base + 3u)) * dp_scale;
    var radius = get_float(base + 4u) * s;
    if radius < 0.0 {
        radius = 0.5 * min(size.x, size.y);
    }
    return sd_round_rect(coord - center, size * 0.5, radius);
}

// Smooth subtraction (the carve twin of smin): shapes with the subtract
// sentinel punch a hole in the field — the growing menu droplet leaves its
// anchor button un-glassed (crisp, riding ON TOP) until it swallows it.
fn smax_sub(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 {
        return max(a, -b);
    }
    let h = clamp(0.5 - 0.5 * (a + b) / k, 0.0, 1.0);
    return mix(a, -b, h) + k * h * (1.0 - h);
}

fn strain_display_to_local(
    display_position: vec2<f32>,
    strain_axis: vec2<f32>,
    strain_along: f32,
    strain_across: f32,
) -> vec2<f32> {
    let strain_normal = vec2<f32>(-strain_axis.y, strain_axis.x);
    return strain_axis * (dot(display_position, strain_axis) / strain_along)
        + strain_normal * (dot(display_position, strain_normal) / strain_across);
}

fn primary_scene_distance(
    p_a: vec2<f32>,
    half_a: vec2<f32>,
    r_a: f32,
    strain_axis: vec2<f32>,
    strain_along: f32,
    strain_across: f32,
) -> f32 {
    let strained_p = strain_display_to_local(
        p_a,
        strain_axis,
        strain_along,
        strain_across,
    );
    let rounded_d = sd_round_rect(strained_p, half_a, r_a);
    let ellipse_d = sd_ellipse_approx(strained_p, half_a);
    return mix(rounded_d, ellipse_d, clamp(get_float(110u), 0.0, 1.0))
        * min(strain_along, strain_across);
}

// Combined scene SDF: the primary shape smooth-unioned with every extra
// scene shape, with a low-frequency angular wobble that makes mid-flight
// shapes bubble like liquid. Wobble perturbs the distance field itself so
// coverage, normals and the lens all follow it.
fn scene_sdf(
    coord: vec2<f32>,
    p_a: vec2<f32>,
    half_a: vec2<f32>,
    r_a: f32,
    shape_count: u32,
    dp_scale: vec2<f32>,
    s: f32,
    glue: f32,
    wobble_amp: f32,
    wobble_phase: f32,
    bulge_amp: f32,
    bulge_dir: f32,
    strain_axis: vec2<f32>,
    strain_along: f32,
    strain_across: f32,
) -> f32 {
    // The inverse affine transform gives the exact deformed zero contour.
    // Scaling the returned distance by its smaller singular value keeps the
    // antialias/refraction bands conservative under strong strain.
    var d = primary_scene_distance(
        p_a,
        half_a,
        r_a,
        strain_axis,
        strain_along,
        strain_across,
    );
    let count = min(shape_count, MAX_SCENE_SHAPES);
    for (var i = 0u; i < count; i = i + 1u) {
        let base = 36u + i * 5u;
        let d_i = scene_shape_sdf(coord, base, dp_scale, s);
        // Radius sentinel ≤ -2 marks a SUBTRACT shape (see smax_sub).
        if get_float(base + 4u) <= -2.0 {
            d = smax_sub(d, d_i, glue);
        } else {
            d = smin(d, d_i, glue);
        }
    }
    if wobble_amp > 0.001 || bulge_amp > 0.001 {
        let theta = atan2(p_a.y, p_a.x);
        let lobes = sin(3.0 * theta + wobble_phase)
            + 0.6 * sin(5.0 * theta - 1.7 * wobble_phase)
            + 0.35 * sin(2.0 * theta + 2.3 * wobble_phase);
        d = d - wobble_amp * lobes * 0.5;
        // Viscous leading-edge redistribution. The focused cos^3 lobe has
        // angular mean 2/(3*pi); subtracting that mean pulls the remaining
        // perimeter inward by the same first-order volume the leading edge
        // gains, so the bulge cannot inflate the bubble.
        let align = max(cos(theta - bulge_dir), 0.0);
        let volume_neutral_lobe = align * align * align - 0.21220659;
        d = d - bulge_amp * volume_neutral_lobe;
    }
    return d;
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let uv = input.uv;
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let material_activity = clamp(get_float(111u), 0.0, 1.0);

    // Effect layer pixel rect injected by the renderer at uniform slot 62
    // (x_offset, y_offset, width, height) in viewport pixels.
    let effect_rect = get_vec4(248u);
    let container_dp = get_vec2(0u);

    // Two geometry modes, keyed on the container-size uniform:
    // - explicit-rect (container > 0): the container is the node/effect area
    //   size in dp and every geometry uniform is dp — dividing the
    //   renderer-injected pixel rect by the container yields px-per-dp, so
    //   geometry lands correctly at ANY render scale (live density, robot
    //   captures at 1.0, fractional desktop scales). Morphing glass and the
    //   raw-effect demo API both use this.
    // - cover (container == 0): the glass covers the whole effect rect —
    //   geometry uniforms are px at the platform density; used by
    //   `Modifier::glass_effect`, whose node size is only known at render
    //   time.
    let cover_mode = container_dp.x <= 0.0 || container_dp.y <= 0.0;
    var dp_scale = vec2<f32>(1.0, 1.0);
    var s = 1.0;
    if !cover_mode {
        dp_scale = effect_rect.zw / max(container_dp, vec2<f32>(1.0));
        s = min(dp_scale.x, dp_scale.y);
    }
    var optical_scale = s;
    if cover_mode {
        optical_scale = max(get_float(99u), 1.0);
    }
    let gradient_extent = WCKSRD_GRADIENT_EXTENT_DP * optical_scale;
    let edge_extent = WCKSRD_EDGE_EXTENT_DP * optical_scale;

    // Fragment position in effect-local pixel coordinates
    let coord = uv * tex_size - effect_rect.xy;

    var center: vec2<f32>;
    var rect_size: vec2<f32>;
    if cover_mode {
        rect_size = effect_rect.zw;
        center = rect_size * 0.5;
    } else {
        center = get_vec2(2u) * dp_scale;
        rect_size = get_vec2(4u) * dp_scale;
    }
    var corner_radius = get_float(6u) * s;
    if corner_radius < 0.0 {
        // Capsule sentinel: the radius follows the smaller half-extent.
        corner_radius = 0.5 * min(rect_size.x, rect_size.y);
    }
    let highlight = get_float(11u);
    let refraction_depth = max(get_float(9u), 0.0);
    let tint_color = get_vec4(14u);
    let saturation = get_float(18u);
    let lift = get_float(20u);
    let dither_amount = get_float(21u);
    var contrast = get_float(24u);
    if contrast <= 0.0 {
        contrast = 1.0;
    }
    // Liquid scene: float 30 = extra shape count (shapes at 36+, 5 floats
    // each); 31 = glue radius for the smooth union; 32/33 = wobble
    // amplitude px / phase.
    let shape_count = u32(max(get_float(30u), 0.0));
    let glue = get_float(31u) * s;
    let wobble_amp = get_float(32u) * s;
    let wobble_phase = get_float(33u);
    let bulge_amp = get_float(26u) * s;
    let bulge_dir = get_float(27u);
    var strain_axis = get_vec2(106u);
    let strain_axis_length = length(strain_axis);
    if strain_axis_length > 0.001 {
        strain_axis = strain_axis / strain_axis_length;
    } else {
        strain_axis = vec2<f32>(1.0, 0.0);
    }
    var strain_along = get_float(108u);
    var strain_across = get_float(109u);
    if strain_along <= 0.0 || strain_across <= 0.0 {
        strain_along = 1.0;
        strain_across = 1.0;
    }
    let half_size = rect_size * 0.5;

    // SDF distance from combined scene (negative = inside)
    let p = coord - center;
    let d = scene_sdf(
        coord, p, half_size, corner_radius,
        shape_count, dp_scale, s, glue, wobble_amp, wobble_phase, bulge_amp, bulge_dir,
        strain_axis, strain_along, strain_across,
    );

    // wcKSRD's `smoothstep(0, 1, rb1)` is the material's coverage transition.
    // Premultiplied compositing against the untouched backdrop is equivalent
    // to the reference shader's final `mix(backdrop, lighting, transition)`.
    let inradius = max(min(half_size.x, half_size.y), 1.0);
    let lens_refraction = max(inradius * refraction_depth, 0.001);
    let rounded_box = clamp(-d / lens_refraction, 0.0, 1.0);
    let coverage = smoothstep(0.0, 1.0, clamp(rounded_box * 32.0, 0.0, 1.0));
    let optical_coverage = smoothstep(
        0.0,
        1.0,
        clamp(
            (gradient_extent - d) / max(edge_extent, 0.001),
            0.0,
            1.0,
        ),
    );
    let outer_coverage = optical_coverage * (1.0 - coverage);
    let surface_coverage = max(coverage, optical_coverage);
    // A morphing glass node uses this same scene SDF as the alpha mask for
    // its foreground content. Keeping the mask in this shader guarantees
    // that blurred children and the refracted backdrop share one silhouette.
    if get_float(112u) > 0.5 {
        return textureSample(input_texture, input_sampler, input.uv)
            * coverage
            * material_activity;
    }
    let resting_tint = get_vec4(113u);
    let resting_alpha = resting_tint.a
        * (1.0 - material_activity)
        * coverage;
    let resting_output = vec4<f32>(
        resting_tint.rgb * resting_alpha,
        resting_alpha,
    );
    if material_activity <= 0.0 {
        return resting_output;
    }
    let shadow_strength = clamp(get_float(102u), 0.0, 1.0);
    var shadow_alpha = 0.0;
    if shadow_strength > 0.0 {
        let shadow_offset = vec2<f32>(0.0, get_float(104u) * s);
        let shadow_coord = coord - shadow_offset;
        let shadow_p = p - shadow_offset;
        let shadow_d = scene_sdf(
            shadow_coord, shadow_p, half_size, corner_radius,
            shape_count, dp_scale, s, glue, wobble_amp, wobble_phase, bulge_amp, bulge_dir,
            strain_axis, strain_along, strain_across,
        ) - get_float(105u) * s;
        let shadow_blur = max(get_float(103u) * s, 1.0);
        shadow_alpha = (1.0 - smoothstep(-0.75, shadow_blur, shadow_d)) * shadow_strength;
    }
    if surface_coverage <= 0.0 {
        return vec4<f32>(0.0, 0.0, 0.0, shadow_alpha);
    }

    // Outward SDF normal (gradient) — the lens axis at this fragment.
    let eps = 0.5;
    let d_dx = scene_sdf(
        coord + vec2<f32>(eps, 0.0), p + vec2<f32>(eps, 0.0), half_size, corner_radius,
        shape_count, dp_scale, s, glue, wobble_amp, wobble_phase, bulge_amp, bulge_dir,
        strain_axis, strain_along, strain_across,
    );
    let d_dy = scene_sdf(
        coord + vec2<f32>(0.0, eps), p + vec2<f32>(0.0, eps), half_size, corner_radius,
        shape_count, dp_scale, s, glue, wobble_amp, wobble_phase, bulge_amp, bulge_dir,
        strain_axis, strain_along, strain_across,
    );
    let grad = vec2<f32>(d_dx - d, d_dy - d) / eps;
    let grad_len = length(grad);
    let outward_normal = select(vec2<f32>(0.0), grad / grad_len, grad_len > 0.001);

    // Refraction uses the wcKSRD source mapping. Loupe focus/fold offsets are
    // applied to that one path rather than selecting another optical model.
    //
    // - LOUPE mode (uniform 80): the text-drag magnifier — a solid glass
    //   drop over an offset focus. Dome magnification (strongest at the
    //   center, easing to exactly 1 where the rim band starts) and a rim
    //   FOLD: the sampling distance overshoots past the rim then walks back,
    //   painting an inverted, compressed image of what lies just beyond
    //   (the next text line upside-down at the bubble's bottom edge).
    let rim_style = clamp(get_float(28u), 0.0, 1.0);
    let dispersion_strength = clamp(get_float(95u), 0.0, 1.0);
    let loupe_mode = get_float(80u);
    var refraction_curve = get_float(94u);
    if refraction_curve <= 0.0 {
        refraction_curve = 0.25;
    }
    // wcKSRD is authored with both the rounded box and optical origin at the
    // viewport midpoint. Components move that box, so translate the optical
    // origin with its live SDF center instead of refracting toward the screen.
    let sampling_position = p;
    let optical_sample = wcksrd_optics(
        p,
        sampling_position,
        half_size,
        d,
        refraction_depth,
        refraction_curve,
        gradient_extent,
        edge_extent,
    );
    let interior = optical_sample.interior;

    let transmission_refraction = clamp(get_float(96u), 0.0, 1.0);
    var base_displacement = optical_sample.source_displacement
        * transmission_refraction;
    var loupe_seam_mask = 0.0;
    if loupe_mode > 0.5 {
        let loupe_activity = clamp(get_float(90u), 0.0, 1.0);
        // The bubble's inradius: capsule half-height (the SDF's deepest
        // point), the natural unit of the drop optic.
        let r_in = max(0.5 * min(rect_size.x, rect_size.y), 1.0);
        let focus_px = get_vec2(81u) * dp_scale;
        var m0 = get_float(83u);
        if m0 <= 0.0 {
            m0 = 1.0;
        }
        var band_start = get_float(84u);
        if band_start <= 0.0 {
            band_start = 0.78;
        }
        var fold_peak = get_float(85u);
        if fold_peak <= 0.0 {
            fold_peak = 1.25;
        }
        // 0 deep inside → 1 at the rim.
        let xr = 1.0 - clamp(-d / r_in, 0.0, 1.0);
        // Magnification: UNIFORM m0 across the whole interior (the measured
        // reference is a flat ~1.25x — the primary line, the handle dot and
        // everything between share one scale; a varying magnification arced the
        // baseline or squashed the dot).
        let m = max(m0, 0.2);
        // Magnify about the shape center, looking at the offset focus. The
        // whole interior shows the focus neighbourhood (the loupe displays
        // content from under the finger, offset up).
        base_displacement = base_displacement + focus_px + p * (1.0 / m - 1.0);
        // The FOLD: an anisotropic vertical mirror — measured on the
        // reference, the band starts ~45% of the depth in on the long edges
        // (showing the NEXT text line inverted, near 1:1 and legible) and
        // barely exists at the end caps (weighting by the normal's
        // verticality also kills the rainbow bullseyes the caps produced
        // under a radial fold).
        let vert = outward_normal.y * outward_normal.y;
        // Vertical-edge weighting (caps barely fold), and the drop optic is
        // asymmetric: the fold below the focus is full strength, the one
        // above much weaker (the reference's top edge shows only a whisper
        // of mirrored ascenders).
        let below = select(0.35, 1.0, outward_normal.y > 0.0);
        // sqrt shaping: the reference visibly bends content at the SHOULDER
        // regions (diagonal normals) — vert-squared starved them — while
        // pure end caps stay near-quiet.
        let vert_weight = pow(vert, 0.60) * below;
        if xr > band_start {
            let tau = clamp((xr - band_start) / max(1.0 - band_start, 0.001), 0.0, 1.0);
            // A fast reach-out (first ~30% of the band) followed by a LONG
            // descending branch — the inversion. The descent's slope is what
            // sets the mirrored image's scale: the reference fold shows the
            // next line at essentially MAIN-TEXT scale, so the descent walks
            // back through the content at ~1 content-dp per display-dp.
            var g = 0.0;
            if tau <= 0.3 {
                g = smoothstep(0.0, 0.3, tau);
            } else {
                // This source-space slope is approximately -1 for the
                // calibrated band and reach, preserving legible mirrored
                // glyph bodies instead of stretching them into streaks.
                g = 1.0 - 0.48 * ((tau - 0.3) / 0.7);
            }
            let fold_weight = vert_weight * 0.62 * loupe_activity;
            let s_band0 = band_start / m;
            let s_units = s_band0 + (fold_peak - s_band0) * g;
            let fold_units = s_units - xr;
            let seam_floor = max(get_float(87u), 0.0) * dp_scale.y;
            let guard_radius = clamp(seam_floor * 1.15 / r_in, 0.28, 0.42);
            let center_column = 1.0
                - smoothstep(max(guard_radius - 0.05, 0.0), guard_radius + 0.08, abs(p.x) / r_in);
            let seam_avoid = center_column
                * smoothstep(0.45, 0.78, outward_normal.y)
                * smoothstep(0.35, 0.70, tau);
            loupe_seam_mask = seam_avoid * 0.55 * loupe_activity;
            // The target fold is anisotropic: it mirrors vertically across the
            // long edges. Using the radial normal here applies its vertical
            // component a second time and starves the shoulder glyphs.
            let fold_sign = select(-1.0, 1.0, outward_normal.y > 0.0);
            var fold_displacement = vec2<f32>(
                0.0,
                fold_sign * fold_units * r_in * fold_weight,
            );
            // The reference folds over a continuously curved surface: even
            // dead-center strokes bow sideways slightly and pick up
            // horizontal chroma. A pure normal displacement leaves vertical
            // stems ruler-straight and fringe-free at the band's center —
            // and a center-symmetric bow (p.x-proportional) still vanishes
            // exactly there, so a small uniform drift rides along.
            fold_displacement.x = fold_displacement.x
                + (p.x * 0.08 + r_in * 0.12) * tau * fold_weight;
            base_displacement = base_displacement + fold_displacement;
        }
    }

    // wcKSRD owns source mapping and backdrop blur.
    let wcksrd_blur_radius = max(get_float(93u), 0.0);
    let transmitted_path = sample_wcksrd_path(
        uv,
        tex_size,
        base_displacement,
        wcksrd_blur_radius,
        loupe_mode > 0.5,
    );
    let plain_path = textureSampleLevel(input_texture, input_sampler, uv, 0.0);
    var rgb = transmitted_path.rgb;
    var outer_rgb = plain_path.rgb;
    var alpha = transmitted_path.a;
    if loupe_seam_mask > 0.0 {
        rgb = mix(rgb, plain_path.rgb, loupe_seam_mask);
    }

    let lens_light_direction = normalize(vec2<f32>(1.0, 1.0));
    let lens_edge_incidence = 0.18
        + 0.82 * max(dot(outward_normal, lens_light_direction), 0.0);
    // The meniscus is a separate light path. At grazing incidence the
    // transmitted ray loses energy before the mirrored and spectral returns
    // are added; this creates the target's dark upper/left inner caustic and
    // bright lower return without displacing the face transmission.
    let meniscus_distance = d;
    // The meniscus and the exterior bevel are distinct optical paths.  The
    // face return is concentrated inside the body; carrying the same broad
    // mask through the exterior bevel turns backdrop detail into a painted
    // outline instead of the target's thin independent light return.
    let meniscus_core = pow(
        clamp(
            wcksrd_meniscus(meniscus_distance, lens_refraction, gradient_extent),
            0.0,
            1.0,
        ),
        6.0,
    );
    let face_meniscus = meniscus_core * coverage;
    let bevel_meniscus = optical_sample.edge_light;
    let long_edge_caustic = 0.18 + 0.82 * pow(abs(outward_normal.y), 1.5);
    let left_cap_absorption = pow(max(-outward_normal.x, 0.0), 2.0);
    let meniscus_transmission_axis = max(
        long_edge_caustic,
        left_cap_absorption,
    );
    let meniscus_absorption = clamp(get_float(100u), 0.0, 1.0);
    let meniscus_transmission_loss = face_meniscus
        * rim_style
        * (1.0 - lens_edge_incidence)
        * meniscus_transmission_axis
        * meniscus_absorption
        * 0.72;
    rgb = rgb * (1.0 - meniscus_transmission_loss);

    // The meniscus returns the ray from the opposite wall of the same glass
    // body. This is the mirrored image visible along the target's long edges;
    // its weight is the wcKSRD gradient band, not a painted bevel mask.
    let reflection_displacement = opposite_side_reflection_displacement(
        p,
        outward_normal,
        half_size,
        corner_radius,
    );
    let reflection_tangent = vec2<f32>(-outward_normal.y, outward_normal.x);
    let reflection_path = sample_wcksrd_reflection_path(
        uv,
        tex_size,
        reflection_displacement,
        reflection_tangent,
        gradient_extent * 1.5,
    );
    let reflection_path_length = length(reflection_displacement);
    let internal_reflection_extinction =
        0.097 * pow(1.0 - lens_edge_incidence, 2.0);
    let internal_reflection_transmittance = exp(
        -reflection_path_length / max(inradius, 1.0) * internal_reflection_extinction,
    );
    let reflection_rgb = reflection_path.rgb * internal_reflection_transmittance;
    let long_edge_return = 0.40 + 0.60 * pow(abs(outward_normal.y), 1.5);
    let meniscus_reflection = clamp(
        face_meniscus
            * long_edge_return
            * mix(0.14, 0.24, rim_style),
        0.0,
        0.24,
    ) * select(1.0, 0.0, loupe_mode > 0.5);
    let bevel_reflection = clamp(
        bevel_meniscus
            * long_edge_return
            * mix(0.035, 0.065, rim_style),
        0.0,
        0.08,
    ) * select(1.0, 0.0, loupe_mode > 0.5);
    rgb = mix(rgb, reflection_rgb, meniscus_reflection);
    outer_rgb = mix(outer_rgb, reflection_rgb, bevel_reflection);

    if rim_style > 0.0 && dispersion_strength > 0.0 {
        let grazing_displacement = -p;
        let prism_bend = clamp(refraction_depth * 0.70, 0.0, 0.45);
        let prism_split = clamp(
            refraction_depth * dispersion_strength * 1.50,
            0.0,
            prism_bend * 0.85,
        );
        let non_wcksrd_displacement = base_displacement
            - optical_sample.source_displacement * transmission_refraction;
        let red_displacement = non_wcksrd_displacement
            + grazing_displacement * (prism_bend - prism_split);
        let blue_displacement = non_wcksrd_displacement
            + grazing_displacement * (prism_bend + prism_split);
        let red_path = textureSampleLevel(
            input_texture,
            input_sampler,
            clamp(
                uv + red_displacement / tex_size,
                vec2<f32>(0.0),
                vec2<f32>(1.0),
            ),
            0.0,
        );
        let blue_path = textureSampleLevel(
            input_texture,
            input_sampler,
            clamp(
                uv + blue_displacement / tex_size,
                vec2<f32>(0.0),
                vec2<f32>(1.0),
            ),
            0.0,
        );
        let forward_dispersion = vec3<f32>(red_path.r, red_path.g, blue_path.b);
        let reflected_dispersion = vec3<f32>(blue_path.r, blue_path.g, red_path.b);
        let spectral_reflection_mix = smoothstep(-0.20, 0.20, outward_normal.y);
        let dispersed =
            mix(forward_dispersion, reflected_dispersion, spectral_reflection_mix);
        let dispersion_weight = clamp(
            face_meniscus
                * rim_style
                * dispersion_strength
                * long_edge_caustic,
            0.0,
            1.0,
        );
        let bevel_dispersion_weight = clamp(
            bevel_meniscus
                * rim_style
                * dispersion_strength
                * long_edge_caustic
                * 0.24,
            0.0,
            0.20,
        );
        rgb = mix(rgb, dispersed, dispersion_weight);
        outer_rgb = mix(outer_rgb, dispersed, bevel_dispersion_weight);
    }

    let inner_meniscus = clamp(
        wcksrd_meniscus(
            meniscus_distance + gradient_extent * 0.5,
            lens_refraction,
            gradient_extent,
        ),
        0.0,
        1.0,
    );
    let long_edge_specular = inner_meniscus
        * pow(abs(outward_normal.y), 4.0)
        * lens_edge_incidence
        * highlight
        * rim_style
        * 0.24;
    rgb = rgb + vec3<f32>(long_edge_specular);
    outer_rgb = outer_rgb + vec3<f32>(long_edge_specular);
    alpha = max(alpha, long_edge_specular);

    let wcksrd_edge_gain = mix(
        highlight,
        highlight * lens_edge_incidence,
        rim_style,
    );
    let wcksrd_edge_light = clamp(
        optical_sample.edge_light
            * wcksrd_edge_gain
            * mix(1.0, long_edge_caustic, rim_style),
        0.0,
        1.0,
    );
    rgb = rgb + vec3<f32>(wcksrd_edge_light);
    outer_rgb = outer_rgb + vec3<f32>(wcksrd_edge_light);
    alpha = max(alpha, wcksrd_edge_light);
    let wcksrd_face_light = clamp(optical_sample.face_light * highlight, 0.0, 0.35);
    rgb = rgb + vec3<f32>(wcksrd_face_light);
    alpha = max(alpha, wcksrd_face_light);
    // Tone pipeline: vibrancy first, then a gentle contrast pivot, then the
    // scheme lift. The lift is a SCREEN blend toward white (or a multiply
    // toward black when negative) — unlike an alpha-mix it keeps the ghosts
    // of what's behind the glass colored, which is what makes the material
    // read bright yet alive.
    let luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    rgb = mix(vec3<f32>(luma), rgb, max(saturation, 0.0));
    rgb = (rgb - vec3<f32>(0.5)) * contrast + vec3<f32>(0.5);
    let outer_luma = dot(outer_rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    outer_rgb = mix(vec3<f32>(outer_luma), outer_rgb, max(saturation, 0.0));
    outer_rgb = (outer_rgb - vec3<f32>(0.5)) * contrast + vec3<f32>(0.5);
    // Interactive lenses frost their face without bleaching the meniscus:
    // the target toggle keeps saturated green/cyan at the outer rise while
    // its recessed chamber approaches white. Surface glass uses uniform lift.
    let face_lift = mix(
        lift,
        lift * mix(0.18, 1.0, interior),
        rim_style,
    );
    if face_lift >= 0.0 {
        rgb = vec3<f32>(1.0) - (vec3<f32>(1.0) - rgb) * (1.0 - face_lift);
        outer_rgb = vec3<f32>(1.0)
            - (vec3<f32>(1.0) - outer_rgb) * (1.0 - lift * 0.18);
    } else {
        rgb = rgb * (1.0 + face_lift);
        outer_rgb = outer_rgb * (1.0 + lift * 0.18);
    }

    // The wcKSRD interior ramp keeps an interactive lens clearer at its edge;
    // surface glass retains a uniform tint across its body.
    let optical_tint_alpha = tint_color.a * mix(1.0, interior, rim_style);
    rgb = mix(rgb, tint_color.rgb, optical_tint_alpha);

    // Adaptive frost (91 strength, 97 foreground luma) protects either
    // foreground polarity. It reacts only when the post-material backdrop is
    // too close to the foreground, then moves the surface toward the opposite
    // luminance pole. Both sampled backdrop and actual foreground determine
    // the correction, preventing white-on-white and black-on-black alike.
    let adaptive_frost = clamp(get_float(91u), 0.0, 1.0);
    if adaptive_frost > 0.0 {
        let foreground_luma = clamp(get_float(97u), 0.0, 1.0);
        let frost_luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        let separation = abs(frost_luma - foreground_luma);
        let contrast_need = 1.0 - smoothstep(0.38, 0.58, separation);
        let foreground_is_light = smoothstep(0.35, 0.65, foreground_luma);
        let target_luma = mix(0.82, 0.18, foreground_is_light);
        let correction = (target_luma - frost_luma) * adaptive_frost * contrast_need;
        rgb = clamp(rgb + vec3<f32>(correction), vec3<f32>(0.0), vec3<f32>(1.0));
    }

    // Ordered-noise dither hides banding in the blurred gradients behind the
    // glass (±0.5/255 at dither_amount = 1).
    let dither = (hash12(coord) - 0.5) * (dither_amount / 255.0);
    rgb = rgb + vec3<f32>(dither);
    outer_rgb = outer_rgb + vec3<f32>(dither);

    // Premultiplied glass plus the shape-derived contact shadow. The shadow is
    // suppressed under the glass itself and therefore cannot darken its face.
    let shadow_out = shadow_alpha * (1.0 - surface_coverage);
    let face_output = vec4<f32>(rgb, alpha) * coverage;
    let outer_output = vec4<f32>(outer_rgb, plain_path.a) * outer_coverage;
    return (face_output + outer_output) * material_activity
        + resting_output
        + vec4<f32>(0.0, 0.0, 0.0, shadow_out);
}
