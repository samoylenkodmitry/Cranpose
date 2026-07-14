
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

const MAX_GLASS_PROFILE_KNOTS: u32 = 6u;

struct ProfileSample {
    height: f32,
    derivative: f32,
}

struct SurfaceSample {
    height: f32,
    gradient: vec2<f32>,
    radial_position: f32,
}

fn profile_knot(base: u32, index: u32) -> vec3<f32> {
    let offset = base + index * 3u;
    return vec3<f32>(
        get_float(offset),
        get_float(offset + 1u),
        get_float(offset + 2u),
    );
}

// The CPU generates monotonicity-preserving tangents. Evaluating the same
// cubic here makes editor curves and production optics mathematically
// identical rather than two approximations that happen to share knots.
fn sample_profile(position_in: f32, count_in: f32, base: u32) -> ProfileSample {
    let count = min(max(u32(count_in), 2u), MAX_GLASS_PROFILE_KNOTS);
    let position = clamp(position_in, 0.0, 1.0);
    let first = profile_knot(base, 0u);
    if position <= first.x {
        return ProfileSample(first.y, first.z);
    }
    let last = profile_knot(base, count - 1u);
    if position >= last.x {
        return ProfileSample(last.y, last.z);
    }

    var start = first;
    var end = profile_knot(base, 1u);
    for (var index = 0u; index + 1u < MAX_GLASS_PROFILE_KNOTS; index = index + 1u) {
        if index + 1u >= count {
            break;
        }
        start = profile_knot(base, index);
        end = profile_knot(base, index + 1u);
        if position <= end.x {
            break;
        }
    }

    let width = max(end.x - start.x, 0.00001);
    let t = clamp((position - start.x) / width, 0.0, 1.0);
    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    let height = h00 * start.y
        + h10 * width * start.z
        + h01 * end.y
        + h11 * width * end.z;

    let dh00 = 6.0 * t2 - 6.0 * t;
    let dh10 = 3.0 * t2 - 4.0 * t + 1.0;
    let dh01 = -dh00;
    let dh11 = 3.0 * t2 - 2.0 * t;
    let derivative = (
        dh00 * start.y
            + dh10 * width * start.z
            + dh01 * end.y
            + dh11 * width * end.z
    ) / width;
    return ProfileSample(height, derivative);
}

// Blend the authored principal cross-sections over a superelliptic radius.
// The blend-weight derivative is part of the analytic gradient: omitting it
// produces a visible diagonal seam whenever X-Z and Y-Z differ.
fn sample_glass_surface(
    local_p: vec2<f32>,
    half_size_in: vec2<f32>,
    unit_scale: f32,
) -> SurfaceSample {
    let half_size = max(half_size_in, vec2<f32>(0.001));
    let normalized = local_p / half_size;
    let abs_normalized = abs(normalized);
    let radial_power = clamp(get_float(114u), 1.5, 8.0);
    let a = pow(abs_normalized.x, radial_power);
    let b = pow(abs_normalized.y, radial_power);
    let q = a + b;
    let x_center = sample_profile(0.0, get_float(115u), 120u);
    if q <= 0.000001 || get_float(113u) <= 0.5 {
        return SurfaceSample(x_center.height, vec2<f32>(0.0), 0.0);
    }

    let inverse_power = 1.0 / radial_power;
    let radial = pow(q, inverse_power);
    let profile_position = clamp(radial, 0.0, 1.0);
    let x_sample = sample_profile(profile_position, get_float(115u), 120u);
    let y_sample = sample_profile(profile_position, get_float(116u), 140u);
    let y_weight = b / q;
    let oval_height = mix(x_sample.height, y_sample.height, y_weight);

    let sign_x = select(-1.0, 1.0, normalized.x >= 0.0);
    let sign_y = select(-1.0, 1.0, normalized.y >= 0.0);
    let x_power = pow(abs_normalized.x, radial_power - 1.0) * sign_x;
    let y_power = pow(abs_normalized.y, radial_power - 1.0) * sign_y;
    let radial_factor = pow(q, inverse_power - 1.0);
    let dr_du = radial_factor * x_power;
    let dr_dv = radial_factor * y_power;
    let q_squared = q * q;
    let dw_du = -b * radial_power * x_power / q_squared;
    let dw_dv = a * radial_power * y_power / q_squared;
    let radial_derivative = mix(x_sample.derivative, y_sample.derivative, y_weight);
    let profile_delta = y_sample.height - x_sample.height;
    let oval_dz_du = radial_derivative * dr_du + profile_delta * dw_du;
    let oval_dz_dv = radial_derivative * dr_dv + profile_delta * dw_dv;
    let x_axis_sample = sample_profile(clamp(abs_normalized.x, 0.0, 1.0), get_float(115u), 120u);
    let y_axis_sample = sample_profile(clamp(abs_normalized.y, 0.0, 1.0), get_float(116u), 140u);
    let toric_height = x_axis_sample.height + y_axis_sample.height - x_center.height;
    let toric_dz_du = x_axis_sample.derivative * sign_x;
    let toric_dz_dv = y_axis_sample.derivative * sign_y;
    let axis_coupling = clamp(get_float(158u), 0.0, 1.0);
    let coupling_weight = axis_coupling;
    let height_delta = toric_height - oval_height;
    let height = oval_height + height_delta * coupling_weight;
    let dz_du = oval_dz_du
        + (toric_dz_du - oval_dz_du) * coupling_weight;
    let dz_dv = oval_dz_dv
        + (toric_dz_dv - oval_dz_dv) * coupling_weight;
    let depth = max(get_float(117u), 0.0) * unit_scale;
    let gradient = vec2<f32>(
        dz_du * depth / half_size.x,
        dz_dv * depth / half_size.y,
    );
    return SurfaceSample(height, gradient, profile_position);
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

// Approximate visible-spectrum weight for a normalized wavelength t in
// [0,1] (0 = violet-bending end, 1 = red end): three gaussians peaking at
// blue, green and red. Used for the rim's rainbow dispersion.
fn spectrum_weight(t: f32) -> vec3<f32> {
    let r = exp(-pow((t - 0.80) / 0.22, 2.0));
    let g = exp(-pow((t - 0.50) / 0.22, 2.0));
    let b = exp(-pow((t - 0.20) / 0.22, 2.0));
    return vec3<f32>(r, g, b);
}

// Cheap screen-space hash for the anti-banding dither.
fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 = p3 + dot(p3, vec3<f32>(p3.y, p3.z, p3.x) + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn sampled_luma(sample_uv: vec2<f32>) -> f32 {
    let rgb = textureSampleLevel(
        input_texture,
        input_sampler,
        clamp(sample_uv, vec2<f32>(0.0), vec2<f32>(1.0)),
        0.0,
    ).rgb;
    return dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
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
    let strain_normal = vec2<f32>(-strain_axis.y, strain_axis.x);
    let local_along = dot(p_a, strain_axis) / strain_along;
    let local_across = dot(p_a, strain_normal) / strain_across;
    let strained_p = strain_axis * local_along + strain_normal * local_across;
    // The inverse affine transform gives the exact deformed zero contour.
    // Scaling the returned distance by its smaller singular value keeps the
    // antialias/refraction bands conservative under strong strain.
    let rounded_d = sd_round_rect(strained_p, half_a, r_a);
    let ellipse_d = sd_ellipse_approx(strained_p, half_a);
    var d = mix(rounded_d, ellipse_d, clamp(get_float(110u), 0.0, 1.0))
        * min(strain_along, strain_across);
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
    let bezel = max(get_float(7u) * s, 0.001);
    let optical_strength_raw = get_float(111u);
    let optical_strength = select(
        1.0,
        clamp(optical_strength_raw, 0.0, 1.0),
        optical_strength_raw > 0.0,
    );
    let disp_scale = get_float(8u) * optical_strength * s;
    let ri = get_float(9u);
    let highlight = get_float(11u);
    let tilt_angle = get_float(12u);
    let tilt_pitch = get_float(13u);
    let tint_color = get_vec4(14u);
    let saturation = get_float(18u);
    let chroma = get_float(19u) * optical_strength;
    let dispersion_axes = max(get_vec2(118u), vec2<f32>(0.0));
    let lift = get_float(20u);
    let dither_amount = get_float(21u);
    let light_dir_in = get_vec2(22u);
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
    // Sheen strength (float 29): broad bezel glow toward the light. 0 keeps
    // the default; the interactive lens dials it near zero for the crisp
    // interior of the reference frames.
    var sheen_strength = get_float(29u);
    if sheen_strength <= 0.0 {
        sheen_strength = 1.0;
    }

    let half_size = rect_size * 0.5;

    // SDF distance from combined scene (negative = inside)
    let p = coord - center;
    let d = scene_sdf(
        coord, p, half_size, corner_radius,
        shape_count, dp_scale, s, glue, wobble_amp, wobble_phase, bulge_amp, bulge_dir,
        strain_axis, strain_along, strain_across,
    );

    // Anti-aliased shape coverage. Output is premultiplied and composited
    // src-over, so outside the shape the pass emits transparency and the
    // untouched (sharp) backdrop below shows through — the blur applied to
    // this pass's input must never leak outside the glass.
    let coverage = 1.0 - smoothstep(-0.75, 0.75, d);
    // A morphing glass node uses this same scene SDF as the alpha mask for
    // its foreground content. Keeping the mask in this shader guarantees
    // that blurred children and the refracted backdrop share one silhouette.
    if get_float(112u) > 0.5 {
        return textureSample(input_texture, input_sampler, input.uv) * coverage;
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
    if coverage <= 0.0 {
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

    // Refraction — two regimes sharing the sampling machinery. The base image
    // displacement and zero-mean spectral carrier stay independent: a prism
    // can separate wavelengths without duplicating the full-color backdrop.
    //
    // - LOUPE mode (uniform 80): the text-drag magnifier — a solid glass
    //   drop over an offset focus. Dome magnification (strongest at the
    //   center, easing to exactly 1 where the rim band starts) and a rim
    //   FOLD: the sampling distance overshoots past the rim then walks back,
    //   painting an inverted, compressed image of what lies just beyond
    //   (the next text line upside-down at the bubble's bottom edge), with
    //   the dispersion fringes confined to that band.
    // - Surface glass/lens: one physical X-Z/Y-Z surface and its normal.
    let bezel_depth = clamp(-d / bezel, 0.0, 1.0);
    let bend = 1.0 - 1.0 / max(ri, 1.0001);
    let rim_style = clamp(get_float(28u), 0.0, 1.0);
    let loupe_mode = get_float(80u);
    let strain_normal = vec2<f32>(-strain_axis.y, strain_axis.x);
    let surface_local_p = strain_axis * (dot(p, strain_axis) / strain_along)
        + strain_normal * (dot(p, strain_normal) / strain_across);
    let surface = sample_glass_surface(surface_local_p, half_size, s);
    let surface_gradient = strain_axis * (dot(surface.gradient, strain_axis) / strain_along)
        + strain_normal * (dot(surface.gradient, strain_normal) / strain_across);
    let tilt = vec2<f32>(tilt_angle, tilt_pitch);
    // A tilted glass body is the same height field plus a shallow plane. The
    // resulting normal, rather than a separate directional displacement,
    // feeds every optical and lighting term below.
    let optical_gradient = surface_gradient + tilt * 0.18;
    let optical_slope = length(optical_gradient);
    let surface_normal = normalize(vec3<f32>(-optical_gradient, 1.0));
    let surface_interior = 1.0 - smoothstep(0.52, 0.88, surface.radial_position);

    var base_displacement = vec2<f32>(0.0);
    var spectral_displacement = vec2<f32>(0.0);
    var spread = 0.0;
    var loupe_seam_mask = 0.0;
    if loupe_mode > 0.5 {
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
        let band_chroma = get_float(86u);

        // 0 deep inside → 1 at the rim.
        let xr = 1.0 - clamp(-d / r_in, 0.0, 1.0);
        // Magnification: UNIFORM m0 across the whole interior (the measured
        // reference is a flat ~1.25x — the primary line, the handle dot and
        // everything between share one scale; any profile gradient arced the
        // baseline or squashed the dot).
        let m = max(m0, 0.2);
        // Magnify about the shape center, looking at the offset focus. The
        // whole interior shows the focus neighbourhood (the loupe displays
        // content from under the finger, offset up).
        base_displacement = focus_px + p * (1.0 / m - 1.0);
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
            let fold_weight = vert_weight;
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
            loupe_seam_mask = seam_avoid * 0.55;
            // The target fold is anisotropic: it mirrors vertically across the
            // long edges. Using the radial normal here applies its vertical
            // component a second time and starves the shoulder glyphs.
            let fold_sign = select(-1.0, 1.0, outward_normal.y > 0.0);
            spectral_displacement = vec2<f32>(
                0.0,
                fold_sign * fold_units * r_in * fold_weight,
            );
            // The reference folds over a continuously curved surface: even
            // dead-center strokes bow sideways slightly and pick up
            // horizontal chroma. A pure normal displacement leaves vertical
            // stems ruler-straight and fringe-free at the band's center —
            // and a center-symmetric bow (p.x-proportional) still vanishes
            // exactly there, so a small uniform drift rides along.
            spectral_displacement.x = spectral_displacement.x
                + (p.x * 0.08 + r_in * 0.12) * tau * fold_weight;
            base_displacement = base_displacement + spectral_displacement;
            spread = band_chroma * smoothstep(0.0, 0.25, tau) * fold_weight;
        }
    } else {
        // Snell displacement and dispersion are two observations of the same
        // surface normal. The signed slope naturally creates the outward
        // crest and inward return without painted concentric bands.
        base_displacement = -optical_gradient * bend * disp_scale;
        spectral_displacement = base_displacement * dispersion_axes;
        spread = min(chroma * smoothstep(0.06, 1.8, optical_slope), 3.0);
    }

    // Spectral dispersion: six taps across the refraction-scale range, each
    // weighted by an approximate rainbow spectrum (short wavelengths bend
    // most) — the green/yellow/magenta fringes the iOS lens shows at its
    // rim. Only the chromatic displacement re-scales per tap; away from the
    // rim the taps converge to one.
    let uv_c = clamp(uv + base_displacement / tex_size, vec2<f32>(0.0), vec2<f32>(1.0));
    let sample_c = textureSampleLevel(input_texture, input_sampler, uv_c, 0.0);
    var rgb = sample_c.rgb;
    let alpha = sample_c.a;
    if spread > 0.02 {
        var acc = vec3<f32>(0.0);
        var wsum = vec3<f32>(0.0);
        // The loupe's wide fold spread quantizes into distinct rainbow bands
        // at six taps ("fewer but hotter" fringe pixels than the reference's
        // softer blend); extra taps smooth it at the same total energy.
        // Surface materials keep six taps because their renders are pinned.
        let tap_count = select(6, 10, loupe_mode > 0.5 || rim_style > 0.5);
        for (var i = 0; i < tap_count; i = i + 1) {
            let t = (f32(i) + 0.5) / f32(tap_count);
            let w = spectrum_weight(t);
            // Red (t→1) bends least, violet (t→0) most.
            let scale = 1.0 + (0.5 - t) * spread;
            let uv_i = clamp(
                uv
                    + (base_displacement + spectral_displacement * (scale - 1.0)) / tex_size,
                vec2<f32>(0.0),
                vec2<f32>(1.0),
            );
            acc = acc + w * textureSampleLevel(input_texture, input_sampler, uv_i, 0.0).rgb;
            wsum = wsum + w;
        }
        rgb = acc / max(wsum, vec3<f32>(0.0001));
    }
    if loupe_seam_mask > 0.0 {
        let plain = textureSampleLevel(input_texture, input_sampler, uv, 0.0).rgb;
        rgb = mix(rgb, plain, loupe_seam_mask);
    }

    // Accent recoloring is a local-detail operation, not a cell state. Dark
    // strokes have lower luminance than their immediate four-neighbour field;
    // broad dark backdrop regions do not. The resulting mask travels with the
    // refracted sample and therefore exists only inside this glass coverage.
    let recolor_strength = clamp(get_float(98u), 0.0, 1.0);
    var content_detail = 0.0;
    if recolor_strength > 0.0 {
        let detail_step = vec2<f32>(4.5 * s) / tex_size;
        let center_luma = dot(sample_c.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        let surround_luma = 0.25 * (
            sampled_luma(uv_c - vec2<f32>(detail_step.x, 0.0))
                + sampled_luma(uv_c + vec2<f32>(detail_step.x, 0.0))
                + sampled_luma(uv_c - vec2<f32>(0.0, detail_step.y))
                + sampled_luma(uv_c + vec2<f32>(0.0, detail_step.y))
        );
        let support_step = vec2<f32>(18.0 * s) / tex_size;
        let support_luma = 0.25 * (
            sampled_luma(uv_c - vec2<f32>(support_step.x, 0.0))
                + sampled_luma(uv_c + vec2<f32>(support_step.x, 0.0))
                + sampled_luma(uv_c - vec2<f32>(0.0, support_step.y))
                + sampled_luma(uv_c + vec2<f32>(0.0, support_step.y))
        );
        let local_contrast = surround_luma - center_luma;
        let edge_detail = smoothstep(0.018, 0.16, local_contrast)
            * (1.0 - smoothstep(0.54, 0.76, center_luma));
        let dark_stroke = (1.0 - smoothstep(0.48, 0.72, center_luma))
            * smoothstep(0.52, 0.78, surround_luma);
        let bounded_dark_region = (1.0 - smoothstep(0.48, 0.68, center_luma))
            * smoothstep(0.54, 0.78, support_luma);
        content_detail = max(edge_detail, max(dark_stroke, bounded_dark_region));
        // Selection color rides through the refracted volume up to its outer
        // lip. The final two percent remains unmodified, preserving the
        // target's dark source sliver immediately outside the glass.
        let recolor_face = mix(1.0, 1.0 - smoothstep(0.82, 0.98, surface.radial_position), rim_style);
        content_detail = content_detail * recolor_face;
    }
    if content_detail > 0.0 {
        let recolor = get_vec4(99u).rgb;
        rgb = mix(rgb, recolor, content_detail * recolor_strength);
    }

    // Long-edge FOLD (uniform 92, 0 = off): the reference bar's meniscus
    // bends light past total internal reflection at its TOP edge — content
    // just above the bar renders inside the band VERTICALLY MIRRORED
    // (section headers upside-down in the bar), blurred by the material's
    // own frost and fading out by ~a third of the bar's height.
    let edge_fold = get_float(92u);
    if edge_fold > 0.0 && rim_style < 0.5 && loupe_mode < 0.5 {
        let top_edge_y = center.y - rect_size.y * 0.5;
        let depth = coord.y - top_edge_y;
        let band = rect_size.y * 0.34;
        if depth > 0.0 && depth < band && outward_normal.y < -0.3 {
            let w = edge_fold * (1.0 - depth / band);
            let uv_fold = clamp(
                uv
                    + vec2<f32>(base_displacement.x, -2.0 * depth) / tex_size,
                vec2<f32>(0.0),
                vec2<f32>(1.0),
            );
            let folded = textureSampleLevel(input_texture, input_sampler, uv_fold, 0.0).rgb;
            rgb = mix(rgb, folded, w);
        }
    }

    // Tone pipeline: vibrancy first, then a gentle contrast pivot, then the
    // scheme lift. The lift is a SCREEN blend toward white (or a multiply
    // toward black when negative) — unlike an alpha-mix it keeps the ghosts
    // of what's behind the glass colored, which is what makes the material
    // read bright yet alive.
    let luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    rgb = mix(vec3<f32>(luma), rgb, max(saturation, 0.0));
    rgb = (rgb - vec3<f32>(0.5)) * contrast + vec3<f32>(0.5);
    // Interactive lenses frost their face without bleaching the meniscus:
    // the target toggle keeps saturated green/cyan at the outer rise while
    // its recessed chamber approaches white. Surface glass uses uniform lift.
    let face_lift = mix(
        lift,
        lift * mix(0.18, 1.0, surface_interior),
        rim_style,
    );
    if face_lift >= 0.0 {
        rgb = vec3<f32>(1.0) - (vec3<f32>(1.0) - rgb) * (1.0 - face_lift);
    } else {
        rgb = rgb * (1.0 + face_lift);
    }

    // Tint blending (near-neutral for plain glass; carries accent for
    // prominent buttons).
    rgb = mix(rgb, tint_color.rgb, tint_color.a);

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

    // The authored center-to-crest height difference supplies the recessed
    // chamber's transmission loss; no independent inner contour is drawn.
    let surface_recess = surface_interior * clamp(1.0 - surface.height, 0.0, 1.0);
    let face_shadow = surface_recess * mix(0.010, 0.018, rim_style);
    rgb = rgb * (1.0 - face_shadow);
    let surface_light_direction = normalize(vec3<f32>(-light_dir_in, 1.0));
    let view_direction = vec3<f32>(0.0, 0.0, 1.0);
    let half_direction = normalize(surface_light_direction + view_direction);
    let fresnel = pow(1.0 - clamp(surface_normal.z, 0.0, 1.0), 3.0);
    let surface_specular = pow(max(dot(surface_normal, half_direction), 0.0), 28.0);
    let surface_reflection = (
        fresnel * mix(0.10, 0.17, rim_style)
            + surface_specular * mix(0.025, 0.045, rim_style)
    ) * highlight;
    rgb = mix(rgb, vec3<f32>(1.0), clamp(surface_reflection, 0.0, 0.32));
    let radial_surface_slope = dot(optical_gradient, outward_normal);
    let return_transmission = max(-radial_surface_slope, 0.0)
        / (1.0 + abs(radial_surface_slope));
    rgb = rgb * (1.0 - return_transmission * mix(0.008, 0.11, rim_style));

    // A single silhouette reflection closes the physical surface. Its arc and
    // the broad sheen both use the sampled surface normal; there is no second
    // painted bevel or chromatic outline.
    let edge_width = mix(1.45, select(0.85, 2.0, loupe_mode > 0.5), rim_style);
    let edge_center = select(0.0, 1.2, loupe_mode > 0.5);
    let edge_line = 1.0 - smoothstep(0.0, edge_width, abs(d + edge_center) - 0.2);
    let facing_light = max(dot(surface_normal, surface_light_direction), 0.0);
    let counter_light_direction = normalize(vec3<f32>(light_dir_in, 1.0));
    let facing_counter = max(dot(surface_normal, counter_light_direction), 0.0);
    var counter_gain = get_float(88u);
    if counter_gain <= 0.0 {
        counter_gain = 0.45;
    }
    let rim_floor = max(
        clamp(get_float(89u), 0.0, 1.0),
        0.12 * rim_style,
    );
    let reflection_arc = max(
        pow(facing_light, 1.4) + pow(facing_counter, 1.8) * counter_gain,
        rim_floor,
    );
    let spec_line = edge_line * reflection_arc;
    let sheen = pow(max(1.0 - bezel_depth, 0.0), 1.65)
        * mix(0.35 + 0.65 * facing_light, reflection_arc, rim_style)
        * sheen_strength;
    let spec_gain = mix(0.52, select(0.18, 0.48, loupe_mode > 0.5), rim_style);
    rgb = rgb + vec3<f32>(spec_line * spec_gain + sheen * 0.16) * highlight;

    if loupe_mode > 0.5 {
        // Loupe content alpha (uniform 90): the reference dissolve fades the
        // WHOLE lens content — magnified glyphs, rim, fold — toward what sits
        // behind the lens (~68% by mid-fade), while the optics themselves
        // stay at full power. Blend against the undisplaced backdrop so the
        // fade reads as the lens turning translucent, not the optics dying.
        var lens_alpha = get_float(90u);
        if lens_alpha <= 0.0 {
            lens_alpha = 1.0;
        }
        if lens_alpha < 1.0 {
            let plain = textureSampleLevel(input_texture, input_sampler, uv, 0.0);
            rgb = mix(plain.rgb, rgb, lens_alpha);
        }
    }

    // Ordered-noise dither hides banding in the blurred gradients behind the
    // glass (±0.5/255 at dither_amount = 1).
    let dither = (hash12(coord) - 0.5) * (dither_amount / 255.0);
    rgb = rgb + vec3<f32>(dither);

    // Premultiplied glass plus the shape-derived contact shadow. The shadow is
    // suppressed under the glass itself and therefore cannot darken its face.
    let shadow_out = shadow_alpha * (1.0 - coverage);
    return vec4<f32>(rgb, alpha) * coverage + vec4<f32>(0.0, 0.0, 0.0, shadow_out);
}
