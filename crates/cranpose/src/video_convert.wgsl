// Converts a decoded bi-planar YUV picture (8-bit NV12 or 10-bit P010) to encoded sRGB RGBA8.
// Supports BT.601 / BT.709 / BT.2020 matrices, limited and full range, clean aperture cropping,
// 0/90/180/270-degree clockwise rotation, and specialized HDR PQ/HLG tone-mapping plus BT.2020
// gamut mapping. When TRANSFER == 0 and GAMUT_2020 == 0, all HDR code compiles out.

// 0 = SDR, 1 = PQ (SMPTE ST 2084), 2 = HLG (ARIB STD-B67)
override TRANSFER: u32 = 0u;
// 0 = BT.601 / BT.709 (sRGB gamut), 1 = BT.2020 -> BT.709 linear gamut conversion
override GAMUT_2020: u32 = 0u;

struct Conversion {
    // x: y_offset, y: y_scale, z: c_offset, w: c_scale
    range: vec4<f32>,
    // x: r_cr, y: g_cb, z: g_cr, w: b_cb
    weights: vec4<f32>,
    // xy: source UV at target (0, 0), zw: source UV delta across target +X
    origin_du: vec4<f32>,
    // xy: source UV delta across target +Y, zw: (1 / target_w, 1 / target_h)
    dv_inv_target: vec4<f32>,
    // xy: min safe chroma UV inside crop, zw: max safe chroma UV inside crop
    inside: vec4<f32>,
}

@group(0) @binding(0) var luma: texture_2d<f32>;
@group(0) @binding(1) var chroma: texture_2d<f32>;
@group(0) @binding(2) var filtering: sampler;
@group(0) @binding(3) var<uniform> conversion: Conversion;

@vertex
fn cover(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

fn srgb_to_linear_channel(v: f32) -> f32 {
    if (v <= 0.04045) {
        return v / 12.92;
    }
    return pow((v + 0.055) / 1.055, 2.4);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        srgb_to_linear_channel(c.r),
        srgb_to_linear_channel(c.g),
        srgb_to_linear_channel(c.b),
    );
}

fn linear_to_srgb_channel(v: f32) -> f32 {
    let x = clamp(v, 0.0, 1.0);
    if (x <= 0.0031308) {
        return x * 12.92;
    }
    return 1.055 * pow(x, 1.0 / 2.4) - 0.055;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        linear_to_srgb_channel(c.r),
        linear_to_srgb_channel(c.g),
        linear_to_srgb_channel(c.b),
    );
}

fn luma_weights() -> vec3<f32> {
    if (GAMUT_2020 != 0u) {
        return vec3<f32>(0.2627, 0.6780, 0.0593);
    }
    return vec3<f32>(0.2126, 0.7152, 0.0722);
}

// Smooth luminance-preserving knee compression from HDR linear (1.0 = 203-nit SDR white) to [0, 1].
fn tone_map_hdr(hdr: vec3<f32>) -> vec3<f32> {
    let l_in = dot(hdr, luma_weights());
    if (l_in <= 0.65) {
        return max(hdr, vec3<f32>(0.0));
    }
    let excess = l_in - 0.65;
    let l_out = 0.65 + (0.35 * excess) / (excess + 0.35);
    return max(hdr * (l_out / max(l_in, 1e-6)), vec3<f32>(0.0));
}

fn pq_to_sdr_linear(encoded: vec3<f32>) -> vec3<f32> {
    let m1 = 0.1593017578125;
    let m2 = 78.84375;
    let c1 = 0.8359375;
    let c2 = 18.8515625;
    let c3 = 18.6875;
    let p = pow(max(encoded, vec3<f32>(0.0)), vec3<f32>(1.0 / m2));
    let num = max(p - vec3<f32>(c1), vec3<f32>(0.0));
    let den = max(vec3<f32>(c2) - vec3<f32>(c3) * p, vec3<f32>(1e-6));
    let linear_10000 = pow(num / den, vec3<f32>(1.0 / m1));
    // Scale relative to ITU-R BT.2408 SDR reference white (203 nits).
    return tone_map_hdr(linear_10000 * (10000.0 / 203.0));
}

fn hlg_inv_oetf_channel(e: f32) -> f32 {
    if (e <= 0.5) {
        return (e * e) / 3.0;
    }
    return (exp((e - 0.55991073) / 0.17883277) + 0.28466892) / 12.0;
}

fn hlg_to_sdr_linear(encoded: vec3<f32>) -> vec3<f32> {
    let scene = vec3<f32>(
        hlg_inv_oetf_channel(encoded.r),
        hlg_inv_oetf_channel(encoded.g),
        hlg_inv_oetf_channel(encoded.b),
    );
    let y_s = max(dot(scene, luma_weights()), 1e-6);
    let display = scene * pow(y_s, 0.2);
    return tone_map_hdr(display * 4.0);
}

fn bt2020_to_bt709(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        1.6604910 * c.r - 0.5876411 * c.g - 0.0728499 * c.b,
        -0.1245505 * c.r + 1.1328999 * c.g - 0.0083494 * c.b,
        -0.0181508 * c.r - 0.1005789 * c.g + 1.1187297 * c.b,
    );
}

@fragment
fn convert(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let t = at.xy * conversion.dv_inv_target.zw;
    let uv = conversion.origin_du.xy
        + t.x * conversion.origin_du.zw
        + t.y * conversion.dv_inv_target.xy;
    let luma_dims = vec2<i32>(textureDimensions(luma));
    let luma_px = clamp(
        vec2<i32>(floor(uv * vec2<f32>(luma_dims))),
        vec2<i32>(0),
        luma_dims - vec2<i32>(1),
    );
    let y = (textureLoad(luma, luma_px, 0).r - conversion.range.x) * conversion.range.y;
    let chroma_uv = clamp(uv, conversion.inside.xy, conversion.inside.zw);
    let c = (textureSampleLevel(chroma, filtering, chroma_uv, 0.0).rg - conversion.range.z)
        * conversion.range.w;
    var rgb = clamp(
        vec3<f32>(
            y + conversion.weights.x * c.y,
            y + conversion.weights.y * c.x + conversion.weights.z * c.y,
            y + conversion.weights.w * c.x,
        ),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );

    if (TRANSFER == 1u) {
        rgb = pq_to_sdr_linear(rgb);
    } else if (TRANSFER == 2u) {
        rgb = hlg_to_sdr_linear(rgb);
    } else if (GAMUT_2020 != 0u) {
        rgb = srgb_to_linear(rgb);
    }
    if (GAMUT_2020 != 0u) {
        rgb = clamp(bt2020_to_bt709(rgb), vec3<f32>(0.0), vec3<f32>(1.0));
    }
    if ((TRANSFER | GAMUT_2020) != 0u) {
        rgb = linear_to_srgb(rgb);
    }
    return vec4<f32>(rgb, 1.0);
}
