
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

// Height profile over the bezel: 0 at the edge, 1 deep inside.
//   circle:   x²           (gentle at the edge, grows inward)
//   squircle: 1 − (1−x)⁴   (steep at the edge, flattens inward)
fn height_profile(x: f32, profile: f32) -> f32 {
    let xc = clamp(x, 0.0, 1.0);
    let h_circle = xc * xc;
    let h_squircle = 1.0 - pow(1.0 - xc, 4.0);
    return mix(h_circle, h_squircle, clamp(profile, 0.0, 1.0));
}

// Derivative of the height profile — the bend strength.
fn d_height_dx(x: f32, profile: f32) -> f32 {
    let xc = clamp(x, 0.0, 1.0);
    let d_circle = 2.0 * xc;
    let d_squircle = 4.0 * pow(1.0 - xc, 3.0);
    return mix(d_circle, d_squircle, clamp(profile, 0.0, 1.0));
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
) -> f32 {
    var d = sd_round_rect(p_a, half_a, r_a);
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
        // Viscous leading-edge bulge: while the shape travels/inflates, its
        // leading side (along `bulge_dir`) swells like a droplet being
        // pulled — the trailing side stays put. cos^3 keeps one focused lobe.
        let align = max(cos(theta - bulge_dir), 0.0);
        d = d - bulge_amp * align * align * align;
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
    let disp_scale = get_float(8u) * s;
    let ri = get_float(9u);
    let profile = get_float(10u);
    let highlight = get_float(11u);
    let tilt_angle = get_float(12u);
    let tilt_pitch = get_float(13u);
    let tint_color = get_vec4(14u);
    let saturation = get_float(18u);
    let chroma = get_float(19u);
    let lift = get_float(20u);
    let dither_amount = get_float(21u);
    let light_dir_in = get_vec2(22u);
    var contrast = get_float(24u);
    if contrast <= 0.0 {
        contrast = 1.0;
    }
    var edge_band = get_float(25u);
    if edge_band <= 0.0 {
        edge_band = 0.5;
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
    // Dome direction: +1 stretches the backdrop outward at the rim (bars,
    // menus); -1 pulls samples toward the center — true magnification (the
    // interactive lens). 0 defaults to +1.
    var dome_dir = get_float(34u);
    if dome_dir == 0.0 {
        dome_dir = 1.0;
    }
    // True magnification factor (uniform 35): interior samples pull toward
    // the shape center by 1/m, enveloped by the height profile so it blends
    // into the rim band. 0/1 = off.
    var magnify = get_float(35u);
    if magnify <= 0.0 {
        magnify = 1.0;
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
    );

    // Anti-aliased shape coverage. Output is premultiplied and composited
    // src-over, so outside the shape the pass emits transparency and the
    // untouched (sharp) backdrop below shows through — the blur applied to
    // this pass's input must never leak outside the glass.
    let coverage = 1.0 - smoothstep(-0.75, 0.75, d);
    if coverage <= 0.0 {
        return vec4<f32>(0.0);
    }

    // Outward SDF normal (gradient) — the lens axis at this fragment.
    let eps = 0.5;
    let d_dx = scene_sdf(
        coord + vec2<f32>(eps, 0.0), p + vec2<f32>(eps, 0.0), half_size, corner_radius,
        shape_count, dp_scale, s, glue, wobble_amp, wobble_phase, bulge_amp, bulge_dir,
    );
    let d_dy = scene_sdf(
        coord + vec2<f32>(0.0, eps), p + vec2<f32>(0.0, eps), half_size, corner_radius,
        shape_count, dp_scale, s, glue, wobble_amp, wobble_phase, bulge_amp, bulge_dir,
    );
    let grad = vec2<f32>(d_dx - d, d_dy - d) / eps;
    let grad_len = length(grad);
    let outward_normal = select(vec2<f32>(0.0), grad / grad_len, grad_len > 0.001);

    // Refraction — two regimes sharing the sampling/dispersion machinery via
    // an achromatic displacement (disp_a) plus a chromatic one (disp_c, the
    // only part the spectral taps re-scale):
    //
    // - LOUPE mode (uniform 80): the text-drag magnifier — a solid glass
    //   drop over an offset focus. Dome magnification (strongest at the
    //   center, easing to exactly 1 where the rim band starts) and a rim
    //   FOLD: the sampling distance overshoots past the rim then walks back,
    //   painting an inverted, compressed image of what lies just beyond
    //   (the next text line upside-down at the bubble's bottom edge), with
    //   the dispersion fringes confined to that band.
    // - Legacy glass/lens: edge band + dome + tilt, exactly as before.
    let x_full = clamp(-d / bezel, 0.0, 1.0);
    let x_edge = clamp(-d / (bezel * edge_band), 0.0, 1.0);
    let bend = 1.0 - 1.0 / max(ri, 1.0001);
    let slope_edge = d_height_dx(x_edge, 1.0);
    let slope_dome = d_height_dx(x_full, profile);
    let rim_style = clamp(get_float(28u), 0.0, 1.0);
    let loupe_mode = get_float(80u);
    let tilt = vec2<f32>(tilt_angle, tilt_pitch);

    var disp_a = vec2<f32>(0.0);
    var disp_c = vec2<f32>(0.0);
    var spread = 0.0;
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
        disp_a = focus_px + p * (1.0 / m - 1.0);
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
        let vert_weight = mix(0.12, 1.0, sqrt(vert)) * below;
        if xr > band_start {
            let tau = clamp((xr - band_start) / max(1.0 - band_start, 0.001), 0.0, 1.0);
            // A fast reach-out (first ~30% of the band) followed by a LONG
            // descending branch — the inversion. The descent's slope is what
            // sets the mirrored image's scale: the reference fold shows the
            // next line at essentially MAIN-TEXT scale, so the descent walks
            // back through the content at ~1 content-dp per display-dp.
            let g = smoothstep(0.0, 0.3, tau) - 0.74 * smoothstep(0.3, 1.0, tau);
            let s_band0 = band_start / m;
            let s_units = s_band0 + (fold_peak - s_band0) * g;
            disp_c = outward_normal * (s_units - xr) * r_in * vert_weight;
            // The reference folds over a continuously curved surface: even
            // dead-center strokes bow sideways slightly and pick up
            // horizontal chroma. A pure normal displacement leaves vertical
            // stems ruler-straight and fringe-free at the band's center —
            // and a center-symmetric bow (p.x-proportional) still vanishes
            // exactly there, so a small uniform drift rides along.
            disp_c.x = disp_c.x + (p.x * 0.08 + r_in * 0.12) * tau * vert_weight;
            // Fold floor (uniform 87, dp): the bottom band must never
            // re-display content nearer the focus than this clearance —
            // the dragged handle's dot hangs just below the line, and
            // mirroring it paints a second pink lobe under the displayed
            // dot. An ABSOLUTE scene clearance (not inradius-relative):
            // the bubble shrinks during grow/dissolve but the dot doesn't.
            let seam_floor = max(get_float(87u), 0.0) * dp_scale.y;
            if seam_floor > 0.0 && outward_normal.y > 0.5 {
                let sample_dy = p.y + disp_a.y + disp_c.y;
                let floor_dy = focus_px.y + seam_floor;
                if sample_dy < floor_dy {
                    // COMPRESS toward the floor instead of pinning, and only
                    // on the truly-bottom arc: the dot lives at the band's
                    // center (normal straight down), while at the corners the
                    // weak fold samples shallow dot-free content — pushing
                    // those deep painted stretched "ribbon" flaps.
                    // Narrow gate: at the shoulders a partial push kinked
                    // the magnified highlight's edge into a jagged notch;
                    // the dot column sits at vert ~1 and keeps full cover.
                    let bottom_arc = smoothstep(0.78, 0.98, vert);
                    disp_c.y = disp_c.y + (floor_dy - sample_dy) * 0.85 * bottom_arc;
                }
            }
            spread = band_chroma * smoothstep(0.0, 0.25, tau) * vert_weight;
        }
    } else {
        // The interactive lens (rim_style 1) drops the dome term:
        // magnification owns its interior, and an opposing dome slope inside
        // the rim band double-imaged whatever sat under the lens.
        let lens =
            (slope_edge + dome_dir * slope_dome * 0.35 * (1.0 - rim_style)) * bend * disp_scale;
        var disp = outward_normal * lens + tilt * slope_dome * bend * disp_scale;
        if magnify != 1.0 {
            // Dome magnification: a plano-convex crown over the WHOLE face —
            // strongest at the deepest interior point, easing to exactly 1
            // at the rim. Straight content bows continuously through a
            // transit and glyphs near the lens edge stay readable (a flat
            // interior magnification read as an opaque scale-swap: content
            // near the edge fell outside the shrunken source window, and
            // passing icons showed no warp until dead center).
            let r_face = max(0.5 * min(rect_size.x, rect_size.y), 1.0);
            let x_face = clamp(-d / r_face, 0.0, 1.0);
            let m_local = 1.0 + (magnify - 1.0) * height_profile(x_face, profile);
            disp = disp + p * (1.0 / m_local - 1.0);
        }
        disp_c = disp;
        spread = chroma * (slope_edge / 4.0);
    }

    // Spectral dispersion: six taps across the refraction-scale range, each
    // weighted by an approximate rainbow spectrum (short wavelengths bend
    // most) — the green/yellow/magenta fringes the iOS lens shows at its
    // rim. Only the chromatic displacement re-scales per tap; away from the
    // rim the taps converge to one.
    let uv_c = clamp(uv + (disp_a + disp_c) / tex_size, vec2<f32>(0.0), vec2<f32>(1.0));
    let sample_c = textureSampleLevel(input_texture, input_sampler, uv_c, 0.0);
    var rgb = sample_c.rgb;
    let alpha = sample_c.a;
    if spread > 0.02 {
        var acc = vec3<f32>(0.0);
        var wsum = vec3<f32>(0.0);
        // The loupe's wide fold spread quantizes into distinct rainbow bands
        // at six taps ("fewer but hotter" fringe pixels than the reference's
        // softer blend); extra taps smooth it at the same total energy.
        // Legacy materials keep exactly six — their renders are pinned.
        let tap_count = select(6, 10, loupe_mode > 0.5);
        for (var i = 0; i < tap_count; i = i + 1) {
            let t = (f32(i) + 0.5) / f32(tap_count);
            let w = spectrum_weight(t);
            // Red (t→1) bends least, violet (t→0) most.
            let scale = 1.0 + (0.5 - t) * spread;
            let uv_i = clamp(
                uv + (disp_a + disp_c * scale) / tex_size,
                vec2<f32>(0.0),
                vec2<f32>(1.0),
            );
            acc = acc + w * textureSampleLevel(input_texture, input_sampler, uv_i, 0.0).rgb;
            wsum = wsum + w;
        }
        rgb = acc / max(wsum, vec3<f32>(0.0001));
    }

    // Tone pipeline: vibrancy first, then a gentle contrast pivot, then the
    // scheme lift. The lift is a SCREEN blend toward white (or a multiply
    // toward black when negative) — unlike an alpha-mix it keeps the ghosts
    // of what's behind the glass colored, which is what makes the material
    // read bright yet alive.
    let luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    rgb = mix(vec3<f32>(luma), rgb, max(saturation, 0.0));
    rgb = (rgb - vec3<f32>(0.5)) * contrast + vec3<f32>(0.5);
    if lift >= 0.0 {
        rgb = vec3<f32>(1.0) - (vec3<f32>(1.0) - rgb) * (1.0 - lift);
    } else {
        rgb = rgb * (1.0 + lift);
    }

    // Tint blending (near-neutral for plain glass; carries accent for
    // prominent buttons).
    rgb = mix(rgb, tint_color.rgb, tint_color.a);

    // Rim lighting: a crisp ~1.6px specular LINE exactly at the edge, lit
    // from `light_dir` with a softer counter-reflection on the opposite arc
    // (the double rim that makes iOS glass edges read as glass), plus a
    // broad low-power sheen across the bezel toward the light.
    let inward_normal = -outward_normal;
    let light_dir = normalize(light_dir_in + tilt * 0.35 + vec2<f32>(0.0, 1e-4));
    // Rim style (uniform 28, read above): 0 = surface glass (soft white
    // spec), 1 = the interactive lens (THIN bright line, stronger dark
    // outline — the chromatic fringe from the spectral taps does the color).
    // The loupe's rim is a ~2.5px bright line fully INSIDE the silhouette
    // (straddling the AA edge halves it and reads as absent); other glass
    // keeps the thin straddling line.
    let edge_width = mix(1.6, select(1.1, 2.6, loupe_mode > 0.5), rim_style);
    let edge_center = select(0.0, 1.6, loupe_mode > 0.5);
    let edge_line = 1.0 - smoothstep(0.0, edge_width, abs(d + edge_center) - 0.2);
    let facing_light = max(dot(inward_normal, light_dir), 0.0);
    let facing_away = max(dot(inward_normal, -light_dir), 0.0);
    // Counter-arc gain (uniform 88, default 0.45): how strongly the edge
    // opposite the light reflects. The reference edit menu's bottom rim runs
    // ~70% of its top; the default keeps every existing material identical.
    var counter_gain = get_float(88u);
    if counter_gain <= 0.0 {
        counter_gain = 0.45;
    }
    // Rim floor (uniform 89, default 0): minimum ring strength on the arcs
    // perpendicular to the light — the reference menu's rim holds ~60-70% of
    // its top brightness at 3/9 o'clock instead of pinching to nothing.
    let rim_floor = clamp(get_float(89u), 0.0, 1.0);
    let spec_line = edge_line
        * mix(
            pow(facing_light, 1.4) + pow(facing_away, 2.0) * counter_gain,
            1.0,
            rim_floor,
        );
    let sheen =
        pow(max(1.0 - x_full, 0.0), 3.0) * (0.4 + 0.6 * facing_light) * sheen_strength;
    // A faint dark contour just inside the bright line keeps the rim legible
    // when the glass sits on a background as bright as the line itself.
    let inner_contour = (1.0 - smoothstep(0.8, 4.0, -d)) * smoothstep(0.4, 1.4, -d);
    // Dark line at the very rim (the lens outline in the reference toggle),
    // strongest on the shadow side, under the bright specular arc.
    let rim_dark = (1.0 - smoothstep(0.0, 1.3, abs(d))) * (0.6 + 0.4 * facing_away);
    // The lens rim must be nearly invisible over uniform background — the
    // dispersion fringe and displacement do the talking; only a whisper of
    // specular reads on the lit arc.
    let spec_gain = mix(0.85, 0.12, rim_style);
    let rim_dark_gain = mix(0.16, 0.10, rim_style);
    let contour_gain = 0.10 * mix(1.0, 0.5, rim_style);
    rgb = rgb * (1.0 - inner_contour * contour_gain * highlight);
    rgb = rgb * (1.0 - rim_dark * rim_dark_gain * highlight);
    var spec_rgb = vec3<f32>(spec_line * spec_gain + sheen * 0.22);
    if loupe_mode > 0.5 {
        // The reference loupe rim is a continuous near-white line around the
        // WHOLE silhouette (brightest on top, still present on the straight
        // sides where a purely directional spec vanishes — without a floor
        // the sides read as a vesica), with only a faint prismatic tint
        // phased across the line's width.
        let facing = pow(facing_light, 1.4) + pow(facing_away, 2.0) * 0.3;
        // The reference ring is near-uniform around the silhouette (side
        // contrast ~as strong as the apex, both ~+127 luminance); only a
        // whisper of top emphasis rides the facing term.
        let ring_gain = (0.92 + 0.08 * facing) * spec_gain;
        // Chromatic rim: on the straight SIDES the reference ring splits
        // into a blue-shifted leading (outer) band and a red-shifted
        // trailing (inner) band; the top arc stays near-white. The split
        // rides the dispersion knob so it dies with the optics on dissolve.
        let side = 1.0 - outward_normal.y * outward_normal.y;
        // Narrow split: wide per-channel offsets wash the ring's mean
        // luminance at the sides (the reference side contrast matches the
        // apex) while staying visible on close crops.
        let fringe = get_float(86u) * side * 4.0;
        let dc = d + edge_center;
        let ring_r = 1.0 - smoothstep(0.0, edge_width, abs(dc + fringe) - 0.2);
        let ring_b = 1.0 - smoothstep(0.0, edge_width, abs(dc - fringe) - 0.2);
        spec_rgb = vec3<f32>(ring_r, edge_line, ring_b) * ring_gain;
    }
    rgb = rgb + spec_rgb * highlight;

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

    // Premultiplied output modulated by the shape coverage.
    return vec4<f32>(rgb, alpha) * coverage;
}
