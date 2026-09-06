
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

// Renderer-reserved slots 236..240: the region of the input texture this
// effect reads (x, y, width, height in texels), zero when the input is the
// whole texture. Every sample goes through `map_uv`, which holds the
// coordinate to the region's texel centers, so a region packed edge to edge
// beside other effects' inputs reads only its own texels and its edge reads
// exactly as a dedicated texture's clamp-to-edge would.
fn region_extent() -> vec4<f32> {
    let region = get_vec4(236u);
    let dims = vec2<f32>(textureDimensions(input_texture));
    if region.z > 0.5 && region.w > 0.5 {
        return region;
    }
    return vec4<f32>(0.0, 0.0, dims.x, dims.y);
}

// Renderer-reserved slots 252/253: the LOGICAL pixel extent the input
// region represents, injected when it is rasterized smaller than that (a
// blur chain's scratch-size intermediate). Pixel-calibrated offsets divide
// by the logical extent — dividing by the physical dimensions of a
// quarter-scale texture would inflate every displacement fourfold. Zero
// means the input is at its logical size.
fn logical_extent() -> vec2<f32> {
    let logical = get_vec2(252u);
    if (logical.x > 0.5 && logical.y > 0.5) {
        return logical;
    }
    return region_extent().zw;
}

// The region as a map from region-local uv to texture uv, computed once
// per fragment and threaded through the sampling helpers so no tap pays for
// a texture query or a uniform fetch.
struct RegionMap {
    offset: vec2<f32>,
    scale: vec2<f32>,
    half_texel: vec2<f32>,
}

fn region_map() -> RegionMap {
    let region = region_extent();
    let dims = max(vec2<f32>(textureDimensions(input_texture)), vec2<f32>(1.0));
    return RegionMap(region.xy / dims, region.zw / dims, 0.5 / max(region.zw, vec2<f32>(1.0)));
}

fn map_uv(map: RegionMap, local_uv: vec2<f32>) -> vec2<f32> {
    let held = clamp(local_uv, map.half_texel, vec2<f32>(1.0) - map.half_texel);
    return map.offset + held * map.scale;
}

fn get_vec4(index: u32) -> vec4<f32> {
    return vec4<f32>(get_float(index), get_float(index + 1u), get_float(index + 2u), get_float(index + 3u));
}

// Renderer-reserved slots 232..236: the first substrate region, the source
// blurred by the adaptive frost's neighbourhood radius and packed in the
// same texture at the blur's scratch size; zero when the renderer packed
// none. Read through its own map, held to its texel centers.
fn substrate_map() -> RegionMap {
    let region = get_vec4(232u);
    let dims = max(vec2<f32>(textureDimensions(input_texture)), vec2<f32>(1.0));
    return RegionMap(region.xy / dims, region.zw / dims, 0.5 / max(region.zw, vec2<f32>(1.0)));
}

fn has_substrate() -> bool {
    let region = get_vec4(232u);
    return region.z > 0.5 && region.w > 0.5;
}

// Material specialization. Every optional feature of this program is gated
// by a uniform, and on a tiler the gates that are OFF still cost: their
// dead branches hold registers and their uniforms are fetched per fragment.
// Each flag below is a pipeline-overridable constant the renderer sets to
// `true` when the uniform it covers holds the feature's inactive value
// (`LIQUID_GLASS_SPECIALIZATIONS` on the Rust side is the one table of
// flags, slots and inactive values). A raised flag replaces the uniform read
// with that value — the same number the uniform carried, so every
// downstream expression is unchanged and the specialized pipeline is
// byte-identical to the general one; the compiler then removes the dead
// feature. Measured on Mali-G76: the showcase card material fell from
// 47 ms to 21 ms of isolated effect-pass time per frame.
override GLASS_LOUPE_OFF: bool = false;
override GLASS_FOLD_OFF: bool = false;
override GLASS_SCENE_SHAPES_OFF: bool = false;
override GLASS_WOBBLE_OFF: bool = false;
override GLASS_ELLIPSE_BLEND_OFF: bool = false;
override GLASS_STRAIN_OFF: bool = false;
override GLASS_ZOOM_ANCHOR_OFF: bool = false;
override GLASS_TOUCH_OFF: bool = false;
override GLASS_CONTENT_MASK_OFF: bool = false;
override GLASS_OPTICAL_BLUR_OFF: bool = false;
override GLASS_SHADOW_OFF: bool = false;
override GLASS_ZOOM_OFF: bool = false;
override GLASS_PHYSICAL_REFRACTION_OFF: bool = false;
override GLASS_FULL_TRANSMISSION: bool = false;
override GLASS_DISPERSION_OFF: bool = false;
override GLASS_ADAPTIVE_FROST_OFF: bool = false;
override GLASS_INK_OFF: bool = false;
override GLASS_RIM_STYLE_OFF: bool = false;
// The interior guard: every rim term (meniscus, bevel, border line,
// specular, the opposite-wall reflection) is a product with a band weight
// that is exactly zero deeper inside the shape than `rim_reach`, so a
// fragment there skips the gradient's two extra SDF evaluations and the
// reflection's five taps and lands on the same bits. Off, the reference
// evaluates everything everywhere; the renderer raises it with the other
// specializations, and the parity test holds the two byte-identical.
override GLASS_INTERIOR_GUARD: bool = false;
// The renderer draws the glass twice when it can, 1 for the interior and
// 2 for the rim, each pipeline compiled without the other's work and
// discarding the other's fragments before any fetch; 0 draws it whole.
override GLASS_RIM_DRAW: i32 = 0;

fn fixed_or(value: f32, fixed: f32, is_fixed: bool) -> f32 {
    return select(value, fixed, is_fixed);
}

fn fixed_or_vec2(value: vec2<f32>, fixed: vec2<f32>, is_fixed: bool) -> vec2<f32> {
    return select(value, fixed, is_fixed);
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
    interior: f32,
    edge_light: f32,
    face_light: f32,
}

const WCKSRD_GRADIENT_EXTENT_DP: f32 = 1.3333334;
const WCKSRD_EDGE_EXTENT_DP: f32 = 0.33333334;

// ONE anti-aliasing policy for every optical band: a transition narrower
// than a physical pixel quantizes into staircase arcs, so every band
// width in px passes through this floor. The original wcKSRD ran at one
// fixed screen scale where its constants were safe; this port derives
// band widths from material and density, and each can go sub-pixel
// independently.
const MIN_BAND_WIDTH_PX: f32 = 1.0;
const MIN_LINE_WIDTH_PX: f32 = 1.4;

fn floored_band_width(width_px: f32) -> f32 {
    return max(width_px, MIN_BAND_WIDTH_PX);
}

fn wcksrd_meniscus(
    distance: f32,
    lens_refraction: f32,
    gradient_extent: f32,
) -> f32 {
    // Each edge ramp spans lens_refraction/4 px; keep it >= a pixel.
    let ramp = floored_band_width(lens_refraction * 0.25);
    let gradient_outer = clamp(-(distance - gradient_extent) / ramp, 0.0, 1.0);
    let gradient_inner = clamp(-(distance + gradient_extent) / ramp, 0.0, 1.0);
    return gradient_outer - gradient_inner;
}

// A surface reflection is bounded by the physical coating/rim thickness, not
// by the distance travelled by the refracted backdrop ray. Keeping this band
// separate lets a deep optic bend content without painting that full depth as
// a bright bevel on regular surface glass.
fn wcksrd_surface_rim(distance: f32, gradient_extent: f32) -> f32 {
    let rim_extent = floored_band_width(gradient_extent);
    return 1.0 - smoothstep(0.0, rim_extent, max(-distance, 0.0));
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

// The rim FOLD: within the band the sampling distance reaches out fast
// (band start -> crest just inside the rim), then walks back down a long
// descending branch — replaying the lens interior mirrored toward the edge.
// Measured on the reference toggle (band 6dp of inradius, crest 0.94,
// slope -1 keeps mirrored content near its original scale).
fn fold_source_units(
    xr: f32,
    band_start: f32,
    crest_xr: f32,
    fold_peak: f32,
    mirror_slope: f32,
) -> f32 {
    if xr <= crest_xr {
        return mix(band_start, fold_peak, smoothstep(band_start, crest_xr, xr));
    }
    return fold_peak + mirror_slope * (xr - crest_xr);
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
fn sample_wcksrd_sharp_path(map: RegionMap, uv: vec2<f32>, tex_size: vec2<f32>) -> vec4<f32> {
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
                + textureSampleLevel(input_texture, input_sampler, map_uv(map, sample_uv), 0.0)
                    * weight_x
                    * weight_y;
        }
    }
    return clamp(reconstructed, vec4<f32>(0.0), vec4<f32>(1.0));
}

fn sample_wcksrd_path(
    map: RegionMap,
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
            return sample_wcksrd_sharp_path(map, center_uv, tex_size);
        }
        return textureSampleLevel(input_texture, input_sampler, map_uv(map, center_uv), 0.0);
    }
    if reconstruct_sharp {
        // Loupe rims soften per pixel up to ~4.5px, wide enough that a
        // coarser grid ripples against the compressed re-image; only this
        // path keeps the dense 9x9 footprint.
        let blur_step = blur_radius / 4.0;
        var accumulated = vec4<f32>(0.0);
        for (var x = -4; x <= 4; x = x + 1) {
            for (var y = -4; y <= 4; y = y + 1) {
                let offset = vec2<f32>(f32(x), f32(y)) * blur_step / tex_size;
                accumulated = accumulated + textureSampleLevel(
                    input_texture,
                    input_sampler,
                    map_uv(map, clamp(center_uv + offset, vec2<f32>(0.0), vec2<f32>(1.0))),
                    0.0,
                );
            }
        }
        return accumulated / 81.0;
    }
    // Material glass caps this blur at 2px (heavier radii ride the separable
    // Gaussian pre-pass), so half-radius spacing keeps every tap within
    // bilinear reach of its neighbour: the same +/-radius footprint as the
    // 9x9 grid at under a third of the taps.
    let blur_step = blur_radius / 2.0;
    var accumulated = vec4<f32>(0.0);
    for (var x = -2; x <= 2; x = x + 1) {
        for (var y = -2; y <= 2; y = y + 1) {
            let offset = vec2<f32>(f32(x), f32(y)) * blur_step / tex_size;
            accumulated = accumulated + textureSampleLevel(
                input_texture,
                input_sampler,
                map_uv(map, clamp(center_uv + offset, vec2<f32>(0.0), vec2<f32>(1.0))),
                0.0,
            );
        }
    }
    return accumulated / 25.0;
}

// The low-frequency neighbourhood: one tap of the substrate the renderer
// blurred by the neighbourhood radius, or, without one, nine taps of the
// source a radius apart.
fn sample_adaptive_neighborhood(
    map: RegionMap,
    uv: vec2<f32>,
    tex_size: vec2<f32>,
    displacement: vec2<f32>,
    radius: f32,
) -> vec4<f32> {
    let center_uv = clamp(
        uv + displacement / tex_size,
        vec2<f32>(0.0),
        vec2<f32>(1.0),
    );
    if (has_substrate()) {
        return textureSampleLevel(input_texture, input_sampler, map_uv(substrate_map(), center_uv), 0.0);
    }
    var accumulated = vec4<f32>(0.0);
    for (var x = -1; x <= 1; x = x + 1) {
        for (var y = -1; y <= 1; y = y + 1) {
            let offset = vec2<f32>(f32(x), f32(y)) * radius / tex_size;
            accumulated = accumulated + textureSampleLevel(
                input_texture,
                input_sampler,
                map_uv(map, clamp(center_uv + offset, vec2<f32>(0.0), vec2<f32>(1.0))),
                0.0,
            );
        }
    }
    return accumulated / 9.0;
}

fn sample_wcksrd_reflection_path(
    map: RegionMap,
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
    let center = textureSampleLevel(input_texture, input_sampler, map_uv(map, center_uv), 0.0);
    let inner = textureSampleLevel(
        input_texture,
        input_sampler,
        map_uv(map, clamp(center_uv - inner_offset, vec2<f32>(0.0), vec2<f32>(1.0))),
        0.0,
    ) + textureSampleLevel(
        input_texture,
        input_sampler,
        map_uv(map, clamp(center_uv + inner_offset, vec2<f32>(0.0), vec2<f32>(1.0))),
        0.0,
    );
    let outer = textureSampleLevel(
        input_texture,
        input_sampler,
        map_uv(map, clamp(center_uv - outer_offset, vec2<f32>(0.0), vec2<f32>(1.0))),
        0.0,
    ) + textureSampleLevel(
        input_texture,
        input_sampler,
        map_uv(map, clamp(center_uv + outer_offset, vec2<f32>(0.0), vec2<f32>(1.0))),
        0.0,
    );
    return center * 0.40 + inner * 0.20 + outer * 0.10;
}

fn wcksrd_optics(
    local_position: vec2<f32>,
    half_size: vec2<f32>,
    distance: f32,
    requested_lens_refraction: f32,
    gradient_extent: f32,
    edge_extent: f32,
    edge_sharpness: f32,
    rim_style: f32,
) -> OpticalSample {
    let lens_refraction = max(requested_lens_refraction, 0.001);
    let interior = clamp(-distance / lens_refraction, 0.0, 1.0);
    // The border line's ramp spans lens_refraction/edge_sharpness px. The
    // drawn line must stay resolvable by the pixel grid: a sub-pixel band
    // point-sampled at pixel centers renders as disconnected sparkles along
    // a curved rim. Widening the band below the floor conserves its energy
    // exactly — the profile's integral along the normal equals its extent,
    // so the gain ratio keeps the line's total light unchanged.
    let border_extent = max(edge_extent, MIN_LINE_WIDTH_PX);
    let border_gain = edge_extent / border_extent;
    let border_ramp = max(lens_refraction / max(edge_sharpness, 1.0), MIN_LINE_WIDTH_PX);
    let border = (clamp(-(distance - edge_extent) / border_ramp, 0.0, 1.0)
        - clamp(-(distance + border_extent - edge_extent) / border_ramp, 0.0, 1.0))
        * border_gain;
    let optical_gradient_band = wcksrd_meniscus(
        distance,
        lens_refraction,
        gradient_extent,
    );
    let lighting_band = mix(
        wcksrd_surface_rim(distance, gradient_extent),
        optical_gradient_band,
        clamp(rim_style, 0.0, 1.0),
    );
    let source_y = -local_position.y / max(half_size.y, 1.0) * 0.29;
    let face_light = 0.5 * clamp(clamp(source_y, 0.0, 0.2) + 0.1, 0.0, 1.0)
        + 0.5 * clamp(clamp(-source_y, -1.0, 0.2) * lighting_band + 0.1, 0.0, 1.0);
    return OpticalSample(interior, border, face_light);
}

// One wavelength's walk through the SAME continuous lens field: the
// refractive index scales the ramp length (blue reaches closer to the
// boundary limit — sample-from-center — than red), so the channels'
// entire descending branches diverge across the rim band, painting the
// reference's wide fringe pairs over the compressed re-image. On the
// face every channel clamps to lens_scale 1 and the split self-cancels.
// The zoom projection rides the same channel interior; the fold
// re-image translates all wavelengths alike.
fn channel_lens_displacement(
    sampling_position: vec2<f32>,
    distance: f32,
    lens_refraction: f32,
    index_scale: f32,
    refraction_curve: f32,
    transmission_refraction: f32,
    optical_zoom: f32,
    zoom_anchor: vec2<f32>,
    loupe_mode: f32,
    loupe_activity: f32,
    loupe_magnification: f32,
    fold_displacement: vec2<f32>,
) -> vec2<f32> {
    let interior = clamp(
        -distance / max(lens_refraction * index_scale, 0.001),
        0.0,
        1.0,
    );
    let lens_scale = sin(pow(interior, refraction_curve) * 1.57);
    // The optical axis (zoom_anchor, zero for plain glass) owns the whole
    // ray: both the projection AND the descending branch pivot on it, so a
    // leaning silhouette still re-images the RIDDEN content around its own
    // center — at the boundary limit every rim point compresses to the
    // anchor, ringing the lens with the content's compressed line
    // uniformly instead of starving the trailing cap and overshooting the
    // leading one.
    //
    let optical_position = sampling_position - zoom_anchor;
    var displacement = optical_position * (lens_scale - 1.0)
        * transmission_refraction;
    if optical_zoom > 1.0 && loupe_mode <= 0.5 {
        // The projection gate follows the 4th power of the interior — the
        // dome is thickest at its apex and sheds the zoom well before the
        // rim band. A linear gate held mid-band samples pinned inside the
        // ridden content, so the descending branch never revisited the
        // well around it and the rim re-image lost its white ring + dark
        // line sequence (the reference "U"). With the steep gate the band
        // walks OUT to the unzoomed rim content, then the branch carries
        // it back to the boundary limit — one continuous sweep.
        let projection_gate = interior * interior * interior * interior;
        displacement += optical_position
            * (1.0 / optical_zoom - 1.0)
            * projection_gate;
    }
    if loupe_mode > 0.5 {
        let rim_bend = sampling_position
            * ((lens_scale - 1.0) / loupe_magnification);
        displacement = mix(displacement, rim_bend, loupe_activity);
    }
    return displacement + fold_displacement;
}

// Cheap screen-space hash for the anti-banding dither.
fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 = p3 + dot(p3, vec3<f32>(p3.y, p3.z, p3.x) + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn apply_tone_and_lift(
    source: vec3<f32>,
    saturation: f32,
    contrast: f32,
    lift: f32,
) -> vec3<f32> {
    let source_luma = dot(source, vec3<f32>(0.2126, 0.7152, 0.0722));
    var toned = mix(vec3<f32>(source_luma), source, max(saturation, 0.0));
    let tone_luma = dot(toned, vec3<f32>(0.2126, 0.7152, 0.0722));
    toned = toned + vec3<f32>((tone_luma - 0.5) * (contrast - 1.0));
    if lift >= 0.0 {
        return vec3<f32>(1.0) - (vec3<f32>(1.0) - toned) * (1.0 - lift);
    }
    return toned * (1.0 + lift);
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
    return mix(rounded_d, ellipse_d, clamp(fixed_or(get_float(110u), 0.0, GLASS_ELLIPSE_BLEND_OFF), 0.0, 1.0))
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

// Renderer-reserved slots 240..248: the composite's rounded clip in region
// pixels (rect, then the four corner radii), zero when unclipped; slot 254:
// the composite alpha. Applying both here lets the glass draw straight into
// the final pass instead of through a masked blit of its own texture.
fn composite_coverage(local_uv: vec2<f32>) -> f32 {
    let mask_rect = get_vec4(240u);
    if mask_rect.z <= 0.5 || mask_rect.w <= 0.5 {
        return 1.0;
    }
    let radii = get_vec4(244u);
    let p = local_uv * logical_extent();
    let half_size = mask_rect.zw * 0.5;
    let center = mask_rect.xy + half_size;
    let local = p - center;
    let radius = select(
        select(radii.x, radii.y, local.x > 0.0),
        select(radii.z, radii.w, local.x > 0.0),
        local.y > 0.0,
    );
    let d = sd_round_rect(local, half_size, radius);
    return 1.0 - smoothstep(-0.5, 0.5, d);
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    return glass_fs(input) * composite_coverage(input.uv) * get_float(254u);
}

fn glass_fs(input: VertexOutput) -> vec4<f32> {
    let uv = input.uv;
    let map = region_map();
    let tex_size = logical_extent();
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
    let shape_count = u32(max(fixed_or(get_float(30u), 0.0, GLASS_SCENE_SHAPES_OFF), 0.0));
    let glue = get_float(31u) * s;
    let wobble_amp = fixed_or(get_float(32u), 0.0, GLASS_WOBBLE_OFF) * s;
    let wobble_phase = get_float(33u);
    let bulge_amp = fixed_or(get_float(26u), 0.0, GLASS_WOBBLE_OFF) * s;
    let bulge_dir = get_float(27u);
    var strain_axis = fixed_or_vec2(get_vec2(106u), vec2<f32>(1.0, 0.0), GLASS_STRAIN_OFF);
    let strain_axis_length = length(strain_axis);
    if strain_axis_length > 0.001 {
        strain_axis = strain_axis / strain_axis_length;
    } else {
        strain_axis = vec2<f32>(1.0, 0.0);
    }
    var strain_along = fixed_or(get_float(108u), 1.0, GLASS_STRAIN_OFF);
    var strain_across = fixed_or(get_float(109u), 1.0, GLASS_STRAIN_OFF);
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
    let physical_refraction_depth = max(get_float(98u), 0.0) * optical_scale;
    let lens_refraction = max(
        select(
            inradius * refraction_depth,
            physical_refraction_depth,
            fixed_or(get_float(101u), 0.0, GLASS_PHYSICAL_REFRACTION_OFF) > 0.5,
        ),
        0.001,
    );
    // How far inside the edge any rim term still has weight: the meniscus
    // bands and their ramp, the border line, the surface rim, and the fold
    // band, each as wide as the code below makes it, plus a pixel.
    let guard_ramp = floored_band_width(lens_refraction * 0.25);
    let guard_border_ramp = max(lens_refraction / max(mix(16.0, 8.0, clamp(fixed_or(get_float(28u), 0.0, GLASS_RIM_STYLE_OFF), 0.0, 1.0)), 1.0), MIN_LINE_WIDTH_PX);
    let guard_fold = fixed_or(get_float(88u), 0.0, GLASS_FOLD_OFF) * optical_scale;
    let rim_reach = max(
        max(1.5 * gradient_extent + guard_ramp, floored_band_width(gradient_extent)),
        max(max(edge_extent, MIN_LINE_WIDTH_PX) + guard_border_ramp, guard_fold),
    ) + 1.0;
    if (GLASS_RIM_DRAW == 1 && d >= -rim_reach) {
        discard;
    }
    if (GLASS_RIM_DRAW == 2 && d < -rim_reach) {
        discard;
    }
    let in_rim = GLASS_RIM_DRAW == 2 || (GLASS_RIM_DRAW != 1 && (!GLASS_INTERIOR_GUARD || d >= -rim_reach));
    let rounded_box = clamp(-d / lens_refraction, 0.0, 1.0);
    // Coverage AA rides the material's refraction band (wcKSRD's rb1·32).
    // A DRAINED lens (material activity 0) is a soft tint pool, not
    // defined glass: the reference's resting bar bubble edge fades over
    // ~8dp (bar_over_orange_purple) — a crisp resting edge doubles with
    // the pill's rim line into an onion contour. Activity sharpens the
    // edge back to the AA band as the lens rises.
    let rest_feather = 8.0 * optical_scale;
    let coverage_ramp = floored_band_width(
        mix(rest_feather, lens_refraction / 32.0, material_activity),
    );
    let coverage = smoothstep(0.0, 1.0, clamp(-d / coverage_ramp, 0.0, 1.0));
    let optical_coverage = smoothstep(
        0.0,
        1.0,
        clamp(
            (gradient_extent - d) / floored_band_width(edge_extent),
            0.0,
            1.0,
        ),
    );
    let outer_coverage = optical_coverage * (1.0 - coverage);
    let surface_coverage = max(coverage, optical_coverage);
    // A morphing glass node uses this same scene SDF as the alpha mask for
    // its foreground content. Keeping the mask in this shader guarantees
    // that blurred children and the refracted backdrop share one silhouette.
    if fixed_or(get_float(112u), 0.0, GLASS_CONTENT_MASK_OFF) > 0.5 {
        return textureSample(input_texture, input_sampler, map_uv(map, input.uv))
            * coverage
            * material_activity;
    }
    let resting_tint = get_vec4(113u);
    // A drained lens is FROSTED glass, not paint: the pane still transmits
    // the blurred backdrop it samples, washed with the resting tint. A
    // tint-only resting output reads as an opaque plank — stars behind a
    // resting bar simply vanished instead of glowing through the frost.
    let resting_weight = (1.0 - material_activity) * coverage;
    let plain_path = textureSampleLevel(input_texture, input_sampler, map_uv(map, uv), 0.0);
    let resting_frost = plain_path * (1.0 - resting_tint.a)
        + vec4<f32>(resting_tint.rgb * resting_tint.a, resting_tint.a);
    let resting_output = resting_frost * resting_weight;
    if material_activity <= 0.0 {
        return resting_output;
    }
    let shadow_strength = clamp(fixed_or(get_float(102u), 0.0, GLASS_SHADOW_OFF), 0.0, 1.0);
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

    // Outward SDF normal (gradient) — the lens axis at this fragment. Deep in
    // the interior every term it feeds carries a zero band weight.
    var outward_normal = vec2<f32>(0.0);
    if (in_rim) {
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
        outward_normal = select(vec2<f32>(0.0), grad / grad_len, grad_len > 0.001);
    }

    // Refraction uses the wcKSRD source mapping. Loupe focus/magnification
    // are applied to that one path rather than selecting another optical
    // model.
    //
    // - LOUPE mode (uniform 80): the text-drag magnifier — a solid glass
    //   drop over an offset focus. ONE continuous mapping (shaders.txt):
    //   sample = focus + p·lens_scale/m — the magnified face, the
    //   descending-branch inversion at the rim and the rim line all come
    //   from the same displacement field, with no band boundaries.
    let rim_style = clamp(fixed_or(get_float(28u), 0.0, GLASS_RIM_STYLE_OFF), 0.0, 1.0);
    // Materials may push past 1 for stronger chromatic splits (the toggle
    // hold runs 1.1); the spread factor keeps the split proportional.
    let dispersion_strength = clamp(fixed_or(get_float(95u), 0.0, GLASS_DISPERSION_OFF), 0.0, 2.0);
    let loupe_mode = fixed_or(get_float(80u), 0.0, GLASS_LOUPE_OFF);
    var refraction_curve = get_float(94u);
    if refraction_curve <= 0.0 {
        refraction_curve = 0.25;
    }
    // wcKSRD is authored with both the rounded box and optical origin at the
    // viewport midpoint. Components move that box, so translate the optical
    // origin with its live SDF center instead of refracting toward the screen.
    let sampling_position = p;
    // Interactive lenses draw the reference's crisp ~1px rim line; surface
    // glass keeps its finer band. The loupe keeps the tight border — its
    // rim hugs bright content (selection accent, the handle dot) and the
    // wider band smears them along the caps.
    var edge_sharpness = mix(16.0, 8.0, rim_style);
    if loupe_mode > 0.5 {
        edge_sharpness = 16.0;
    }
    let optical_sample = wcksrd_optics(
        p,
        half_size,
        d,
        lens_refraction,
        gradient_extent,
        edge_extent,
        edge_sharpness,
        rim_style,
    );
    let interior = optical_sample.interior;

    let transmission_refraction = clamp(fixed_or(get_float(96u), 1.0, GLASS_FULL_TRANSMISSION), 0.0, 1.0);
    // Uniform face magnification (uniform 89, dp-free ratio): the riding
    // lens projects its backdrop enlarged across the whole face. Blended by
    // the channel interior so the rim band keeps the wcKSRD edge mapping,
    // and applied outside the transmission attenuation — the face zoom is a
    // projection property, not an edge-refraction one. Pure displacement:
    // white stays white. The optical axis (uniform 128, dp offset from the
    // SDF center) can trail a leaning silhouette: the droplet's body leans
    // toward its travel side while its curvature apex stays over the content
    // it rides (the toggle thumb) — anchoring the magnification there keeps
    // the face filled by the ridden content instead of pulling in the well
    // beyond it.
    let optical_zoom = max(fixed_or(get_float(89u), 1.0, GLASS_ZOOM_OFF), 1.0);
    let zoom_anchor = fixed_or_vec2(get_vec2(128u), vec2<f32>(0.0), GLASS_ZOOM_ANCHOR_OFF) * dp_scale;
    // Displacement that translates the image without bending the ray (the
    // loupe's focus offset — optically a flat-slab shift). Translation does
    // not disperse; only the ray-bend components built per channel below
    // carry the chromatic split.
    var achromatic_displacement = vec2<f32>(0.0);
    var loupe_rim_softening = 0.0;
    var loupe_activity = 0.0;
    var loupe_magnification = 1.0;
    if loupe_mode > 0.5 {
        loupe_activity = clamp(get_float(90u), 0.0, 1.0);
        let focus_px = get_vec2(81u) * dp_scale;
        var m0 = get_float(83u);
        if m0 <= 0.0 {
            m0 = 1.0;
        }
        loupe_magnification = max(m0, 0.2);
        // The single wcKSRD field, centered on the offset focus: deep in the
        // face lens_scale = 1 and the whole interior shows the focus
        // neighbourhood magnified by m; toward the rim the SAME sweep that
        // shapes surface glass walks the sample back to the focus point,
        // replaying the content between them inverted — the drop optic's
        // bottom-edge wrap — with C1 continuity everywhere. The field
        // p·(lens_scale/m − 1) decomposes exactly into a pure magnification
        // p·(1/m − 1) plus the rim bend p·(lens_scale − 1)/m; only the bend
        // disperses (channel_lens_displacement), the magnified face stays
        // achromatic like the reference loupe glyphs.
        let lens_scale = sin(pow(interior, refraction_curve) * 1.57);
        let pure_zoom = p * (1.0 / loupe_magnification - 1.0);
        achromatic_displacement = (focus_px + pure_zoom) * loupe_activity;
        // Near the rim the sweep minifies the face text hard enough that
        // single sharp taps hit isolated white glyph pixels as round
        // specks (live report: "white dots"). The original shader blurs
        // inside the lens; here the blur fades in only over the
        // compression zone so the face keeps its Catmull-Rom crispness.
        let rim_compression = 1.0 - smoothstep(0.70, 0.95, lens_scale);
        loupe_rim_softening = 4.5 * optical_scale * rim_compression * loupe_activity;
    }

    // Interactive rim fold (uniform 88 = band depth in dp; zero = off):
    // a PURE displacement — the band replays the interior mirrored toward
    // the rim, the reference toggle's "U". No color terms ride on it, so
    // white surround mirrors white and colored tracks mirror themselves.
    let fold_depth_px = fixed_or(get_float(88u), 0.0, GLASS_FOLD_OFF) * optical_scale;
    var fold_displacement = vec2<f32>(0.0);
    var fold_absorb = 0.0;
    if loupe_mode <= 0.5 && fold_depth_px > 0.0 {
        let r_in_fold = max(0.5 * min(rect_size.x, rect_size.y), 1.0);
        let fold_band_start = clamp(1.0 - fold_depth_px / r_in_fold, 0.05, 0.95);
        let xr = 1.0 - clamp(-d / r_in_fold, 0.0, 1.0);
        if xr > fold_band_start {
            let crest_xr = fold_band_start + 0.3 * (1.0 - fold_band_start);
            let s_units = fold_source_units(xr, fold_band_start, crest_xr, 0.94, -1.0);
            let fold_tau = clamp(
                (xr - fold_band_start) / max(1.0 - fold_band_start, 0.001),
                0.0,
                1.0,
            );
            let fold_presence = smoothstep(0.0, 0.12, fold_tau);
            fold_displacement = outward_normal * (s_units - xr) * r_in_fold * fold_presence;
            // Grazing rays traverse the folded material twice: the return
            // darkens by the transmitted color itself near the rim (sage
            // doubles into its saturated turn line; white stays white).
            fold_absorb = smoothstep(0.62, 1.0, fold_tau) * 0.35;
        }
    }
    let base_displacement = channel_lens_displacement(
        sampling_position,
        d,
        lens_refraction,
        1.0,
        refraction_curve,
        transmission_refraction,
        optical_zoom,
        zoom_anchor,
        loupe_mode,
        loupe_activity,
        loupe_magnification,
        fold_displacement,
    );

    // wcKSRD owns source mapping and backdrop blur.
    let wcksrd_blur_radius = max(max(fixed_or(get_float(93u), 0.0, GLASS_OPTICAL_BLUR_OFF), 0.0), loupe_rim_softening);
    let transmitted_displacement = achromatic_displacement + base_displacement;
    let transmitted_path = sample_wcksrd_path(
        map,
        uv,
        tex_size,
        transmitted_displacement,
        wcksrd_blur_radius,
        loupe_mode > 0.5,
    );
    var rgb = transmitted_path.rgb;
    if dispersion_strength > 0.0 {
        // Chromatic transmission as ONE continuous ray model: each channel
        // walks the SAME lens field at its own refractive index (blue bends
        // more than red, as in real glass). The index scales the ramp
        // length, so blue lands nearer the boundary limit — the compressed
        // re-image of the interior — than red, and the channels' whole
        // descending branches diverge across the rim band. On the face
        // every channel clamps to lens_scale 1 and the split self-cancels;
        // no band masks and no separate spectral path. Green rides the
        // already-sampled transmitted ray; everything downstream (fold
        // absorption, meniscus, ink recolor, tone) operates on the merged
        // chromatic transmission.
        let index_spread = dispersion_strength * 0.22;
        let red_displacement = achromatic_displacement + channel_lens_displacement(
            sampling_position,
            d,
            lens_refraction,
            1.0 - index_spread,
            refraction_curve,
            transmission_refraction,
            optical_zoom,
            zoom_anchor,
            loupe_mode,
            loupe_activity,
            loupe_magnification,
            fold_displacement,
        );
        let blue_displacement = achromatic_displacement + channel_lens_displacement(
            sampling_position,
            d,
            lens_refraction,
            1.0 + index_spread,
            refraction_curve,
            transmission_refraction,
            optical_zoom,
            zoom_anchor,
            loupe_mode,
            loupe_activity,
            loupe_magnification,
            fold_displacement,
        );
        var red_path = transmitted_path;
        if any(red_displacement != transmitted_displacement) {
            red_path = sample_wcksrd_path(
                map,
                uv,
                tex_size,
                red_displacement,
                wcksrd_blur_radius,
                loupe_mode > 0.5,
            );
        }
        var blue_path = transmitted_path;
        if any(blue_displacement != transmitted_displacement) {
            blue_path = sample_wcksrd_path(
                map,
                uv,
                tex_size,
                blue_displacement,
                wcksrd_blur_radius,
                loupe_mode > 0.5,
            );
        }
        rgb = vec3<f32>(red_path.r, rgb.g, blue_path.b);
    }
    if fold_absorb > 0.0 {
        rgb = rgb * mix(vec3<f32>(1.0), rgb, fold_absorb);
    }
    // Ink recolor (uniforms 124..126 color, 127 strength): the lens itself
    // recolors the dark INK it transmits — the reference tab bubble shows
    // the icon beneath in the accent as a pure optical act. The mask keys
    // on transmitted luminance, so the light bar surface passes through
    // untouched and only glyph ink takes the color. This lives in the
    // MATERIAL: recoloring the elements instead refracts their accent
    // into smears around the bubble rim (live report).
    let ink_recolor_strength = clamp(fixed_or(get_float(127u), 0.0, GLASS_INK_OFF), 0.0, 1.0);
    if ink_recolor_strength > 0.0 {
        let ink_color = vec3<f32>(get_float(124u), get_float(125u), get_float(126u));
        let transmitted_luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        // Ink is content close to the THEME'S OWN glyph luma (uniform 97:
        // near-black on a light scheme, near-white on a dark one) rather
        // than content below a fixed absolute cutoff. An absolute cutoff
        // means "dark", which only glyphs are on a light bar; on a dark
        // scheme the whole backdrop behind the bar is dark too, not only
        // the glyphs, so it classified almost everything as ink and
        // recolored the lens solid instead of just the glyph it rides over.
        let foreground_luma = clamp(get_float(97u), 0.0, 1.0);
        let ink_departure = abs(transmitted_luma - foreground_luma);
        let ink_mask = 1.0 - smoothstep(0.18, 0.50, ink_departure);
        rgb = mix(rgb, ink_color, ink_mask * ink_recolor_strength);
    }
    var outer_rgb = plain_path.rgb;
    var alpha = transmitted_path.a;

    // Ambient light return direction (uniforms 122,123): screen-space unit
    // vector pointing from the light source THROUGH the glass — the rim
    // facing it carries the wide bright return glow, the rim facing the
    // source its crisp arc. Zero = unset -> the reference default (light
    // overhead, return at the bottom). Device attitude feeds this, rotating
    // every bevel arc in real time.
    var light_return = get_vec2(122u);
    let light_return_len = length(light_return);
    if light_return_len < 0.001 {
        light_return = vec2<f32>(0.0, 1.0);
    } else {
        light_return = light_return / light_return_len;
    }
    let lens_light_direction = light_return;
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
    var reflection_rgb = vec3<f32>(0.0);
    if (in_rim) {
        let reflection_displacement = opposite_side_reflection_displacement(
            p,
            outward_normal,
            half_size,
            corner_radius,
        );
        let reflection_tangent = vec2<f32>(-outward_normal.y, outward_normal.x);
        let reflection_path = sample_wcksrd_reflection_path(
            map,
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
        reflection_rgb = reflection_path.rgb * internal_reflection_transmittance;
    }
    // The opposite-wall return belongs to an interactive lens. Applying it
    // to a regular surface duplicates the rim as a darker band inside it.
    let long_edge_return = 0.40 + 0.60 * pow(abs(outward_normal.y), 1.5);
    let meniscus_reflection = clamp(
        face_meniscus
            * long_edge_return
            * rim_style
            * 0.24,
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
    // Rim reflectivity (uniform 121, 0 = unset -> full): the toggle's
    // reference rim draws this line, the segmented lens body is invisible.
    var rim_reflectivity = get_float(121u);
    if rim_reflectivity <= 0.0 {
        rim_reflectivity = 1.0;
    }
    rgb = mix(rgb, reflection_rgb, meniscus_reflection * rim_reflectivity);
    outer_rgb = mix(outer_rgb, reflection_rgb, bevel_reflection * rim_reflectivity);


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

    // The etalon adds its rb2 border line UNGATED (+1.0*rb2); gating it by
    // the material highlight erased the reference's crisp bright ring on
    // low-highlight interactive lenses (the pressed toggle reads a thin
    // white line the chromatic fringes color). The line rides rim
    // reflectivity, so the segmented lens keeps its invisible body.
    let etalon_border_gain = rim_style * rim_reflectivity * 0.7;
    let wcksrd_edge_gain = max(
        mix(highlight, highlight * lens_edge_incidence, rim_style),
        etalon_border_gain,
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
    // The loupe face is PURE magnification: the reference preserves the
    // backdrop's luminance (dark editor stays dark under the loupe) — the
    // additive face light read as a milky film there. Every other lighting
    // term is already gated off in loupe mode; this one leaked through.
    let wcksrd_face_light = clamp(optical_sample.face_light * highlight, 0.0, 0.35)
        * select(1.0, 0.0, loupe_mode > 0.5);
    rgb = rgb + vec3<f32>(wcksrd_face_light);
    alpha = max(alpha, wcksrd_face_light);
    // The BEVEL (measured on the on-white reference discs): a directional
    // pair riding the existing meniscus bands. The rim facing the light
    // source draws a crisp arc; the opposite rim a WIDER, stronger return
    // glow — the dominant edge feature of the reference material. Both
    // brighten the transmitted color itself (the teal disc's bottom lip
    // stays saturated teal, never washes to white) with only a whisper of
    // additive white, and both rotate with the light input.
    let bevel_source_axis = pow(max(dot(outward_normal, -light_return), 0.0), 2.5);
    let bevel_return_axis = pow(max(dot(outward_normal, light_return), 0.0), 1.8);
    // The bevel is STRUCTURAL for fully-active glass: the reference discs
    // carry it at rest independent of the material's specular highlight
    // knob — so it takes its own floor and only grows past it when the
    // material is explicitly more specular. Measured on the on-white teal
    // disc: the arcs are THIN (the raw ~1.3dp meniscus band, no falloff
    // sharpening — the pow-6 core crushed them to invisibility) and
    // STRONG (body 184 -> lip 237 green, ~+50% luminance at highlight
    // 0.72, hue preserved). A DRAINED lens is not structural: the
    // reference's resting bar bubble is a soft tint capsule with no rim
    // pair (bar_over_orange_purple), so the floor scales with the
    // material's activity and vanishes with it.
    // Interactive lenses (rim_style -> 1) carry NO structural floor: their
    // rim is the chromatic dispersion band (toggle-press reference), and
    // the white bevel lip both buried those fringes and clipped to white
    // over light wells. Surface glass (discs, pills, toggles' wells) keeps
    // the structural bevel.
    let bevel_gain = max(
        highlight,
        0.18 * material_activity * (1.0 - clamp(rim_style, 0.0, 1.0)),
    );
    let surface_bevel_band = wcksrd_surface_rim(meniscus_distance, gradient_extent);
    let optical_bevel_band = wcksrd_meniscus(
        meniscus_distance,
        lens_refraction,
        gradient_extent,
    );
    let bevel_band = clamp(
        mix(surface_bevel_band, optical_bevel_band, rim_style),
        0.0,
        1.0,
    ) * coverage;
    // The lip is a SPECULAR return of the (white) environment light, not a
    // brightening of the body color: the reference teal disc's lip lifts
    // its red channel 0 -> 112 — a screen blend toward white at ~0.44 lip
    // weight (top arc measured ~0.08). Applied AFTER the tint below: the
    // reflection rides the glass SURFACE, above the tinted body (applying
    // it first buried it under an 82%-alpha tint).
    let bevel_source_arc = bevel_band * bevel_source_axis * bevel_gain * 0.10;
    let bevel_return_glow = bevel_band * bevel_return_axis * bevel_gain * 0.62;
    // The loupe keeps its own calibrated rim: the white lip lands on its
    // mirrored-text band and fuses with glyph strokes into white blobs
    // over dark backdrops (live report).
    let bevel_light = clamp(bevel_source_arc + bevel_return_glow, 0.0, 0.60)
        * select(1.0, 0.0, loupe_mode > 0.5);
    alpha = max(alpha, bevel_light);
    // Tone pipeline: vibrancy first, then a gentle contrast pivot, then the
    // scheme lift. The lift is a SCREEN blend toward white (or a multiply
    // toward black when negative) — unlike an alpha-mix it keeps the ghosts
    // of what's behind the glass colored, which is what makes the material
    // read bright yet alive.
    // Tone: vibrancy, then a LUMA compression toward the mid pivot that
    // carries chroma unchanged — the reference dark menu blooms saturated
    // magenta out of a deep purple backdrop while dimming white through the
    // same law (menu-expand f_020); a per-channel pivot crushes exactly the
    // chroma that bloom needs.
    // Interactive lenses frost their face without bleaching the meniscus:
    // the target toggle keeps saturated green/cyan at the outer rise while
    // its recessed chamber approaches white. Surface glass uses uniform lift.
    let face_lift = mix(
        lift,
        lift * mix(0.18, 1.0, interior),
        rim_style,
    );
    rgb = apply_tone_and_lift(rgb, saturation, contrast, face_lift);
    outer_rgb = apply_tone_and_lift(outer_rgb, saturation, contrast, lift * 0.18);

    // The wcKSRD interior ramp keeps an interactive lens clearer at its edge;
    // surface glass retains a uniform tint across its body.
    let optical_tint_alpha = tint_color.a * mix(1.0, interior, rim_style);
    rgb = mix(rgb, tint_color.rgb, optical_tint_alpha);
    // The bevel's specular return, on the outermost surface of the glass.
    rgb = rgb + (vec3<f32>(1.0) - rgb) * bevel_light;

    // Touch glow (uniforms 118-119 node-local dp, 120 intensity): a pressed
    // liquid surface concentrates saturation and a soft light in a radial
    // gradient UNDER THE FINGER — never a flat surface recolor. Saturation
    // of white is white and the light is a small screen-ish add, so bright
    // backdrops stay safe.
    let touch_strength = clamp(fixed_or(get_float(120u), 0.0, GLASS_TOUCH_OFF), 0.0, 1.0);
    if touch_strength > 0.0 {
        let touch_px = vec2<f32>(get_float(118u), get_float(119u)) * dp_scale;
        let touch_reach = 58.0 * optical_scale;
        let touch_falloff =
            1.0 - smoothstep(0.0, touch_reach, distance(coord, touch_px));
        let glow = touch_falloff * touch_falloff * touch_strength * coverage;
        let touch_luma = dot(rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        rgb = mix(rgb, mix(vec3<f32>(touch_luma), rgb, 1.65), glow);
        rgb = rgb + vec3<f32>(0.09) * glow;
    }

    // Adaptive frost (91 strength, 97 foreground luma) protects either
    // foreground polarity. The decision comes from a low-frequency backdrop
    // neighborhood and applies one local exposure correction to both the
    // background and its detail. A per-fragment decision classifies thin
    // light/dark backdrop glyphs as a new background polarity and inverts
    // them, even though the surrounding card already has safe contrast.
    let adaptive_frost = clamp(fixed_or(get_float(91u), 0.0, GLASS_ADAPTIVE_FROST_OFF), 0.0, 1.0);
    if adaptive_frost > 0.0 {
        let foreground_luma = clamp(get_float(97u), 0.0, 1.0);
        let adaptive_sample = sample_adaptive_neighborhood(
            map,
            uv,
            tex_size,
            achromatic_displacement + base_displacement,
            16.0 * optical_scale,
        );
        var adaptive_rgb = apply_tone_and_lift(
            adaptive_sample.rgb,
            saturation,
            contrast,
            face_lift,
        );
        adaptive_rgb = mix(adaptive_rgb, tint_color.rgb, optical_tint_alpha);
        let adaptive_luma = dot(adaptive_rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        let separation = abs(adaptive_luma - foreground_luma);
        let contrast_need = 1.0 - smoothstep(0.38, 0.58, separation);
        let foreground_is_light = smoothstep(0.35, 0.65, foreground_luma);
        let target_luma = mix(0.82, 0.18, foreground_is_light);
        let correction = (target_luma - adaptive_luma) * adaptive_frost * contrast_need;
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
