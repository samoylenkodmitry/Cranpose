#version 450
// Writes a decoded picture's RGB. The immutable sampler of the pipeline holds the YCbCr
// conversion of the picture's hardware buffer, so the sampler returns the encoded R'G'B' values.
// Specialization constants compile out all HDR tone-mapping and BT.2020 gamut work for SDR streams.

layout(set = 0, binding = 0) uniform sampler2D picture;

// 0 = SDR, 1 = PQ (SMPTE ST 2084), 2 = HLG (ARIB STD-B67)
layout(constant_id = 0) const uint TRANSFER = 0u;
// 0 = BT.601 / BT.709 (sRGB gamut), 1 = BT.2020 -> BT.709 linear gamut conversion
layout(constant_id = 1) const uint GAMUT_2020 = 0u;

// Affine crop + rotation transform from target fragment coordinates to source buffer UVs, and the
// clamped chroma box that keeps edge taps inside the clean aperture.
layout(push_constant) uniform Window {
    vec4 origin_du;
    vec4 dv_inv_target;
    vec4 inside;
} window;

layout(location = 0) out vec4 color;

float srgb_to_linear_channel(float v) {
    if (v <= 0.04045) {
        return v / 12.92;
    }
    return pow((v + 0.055) / 1.055, 2.4);
}

vec3 srgb_to_linear(vec3 c) {
    return vec3(
        srgb_to_linear_channel(c.r),
        srgb_to_linear_channel(c.g),
        srgb_to_linear_channel(c.b)
    );
}

float linear_to_srgb_channel(float v) {
    float x = clamp(v, 0.0, 1.0);
    if (x <= 0.0031308) {
        return x * 12.92;
    }
    return 1.055 * pow(x, 1.0 / 2.4) - 0.055;
}

vec3 linear_to_srgb(vec3 c) {
    return vec3(
        linear_to_srgb_channel(c.r),
        linear_to_srgb_channel(c.g),
        linear_to_srgb_channel(c.b)
    );
}

vec3 luma_weights() {
    if (GAMUT_2020 != 0u) {
        return vec3(0.2627, 0.6780, 0.0593);
    }
    return vec3(0.2126, 0.7152, 0.0722);
}

vec3 tone_map_hdr(vec3 hdr) {
    float l_in = dot(hdr, luma_weights());
    if (l_in <= 0.65) {
        return max(hdr, vec3(0.0));
    }
    float excess = l_in - 0.65;
    float l_out = 0.65 + (0.35 * excess) / (excess + 0.35);
    return max(hdr * (l_out / max(l_in, 1e-6)), vec3(0.0));
}

vec3 pq_to_sdr_linear(vec3 encoded) {
    const float m1 = 0.1593017578125;
    const float m2 = 78.84375;
    const float c1 = 0.8359375;
    const float c2 = 18.8515625;
    const float c3 = 18.6875;
    vec3 p = pow(max(encoded, vec3(0.0)), vec3(1.0 / m2));
    vec3 num = max(p - vec3(c1), vec3(0.0));
    vec3 den = max(vec3(c2) - vec3(c3) * p, vec3(1e-6));
    vec3 linear_10000 = pow(num / den, vec3(1.0 / m1));
    return tone_map_hdr(linear_10000 * (10000.0 / 203.0));
}

float hlg_inv_oetf_channel(float e) {
    if (e <= 0.5) {
        return (e * e) / 3.0;
    }
    return (exp((e - 0.55991073) / 0.17883277) + 0.28466892) / 12.0;
}

vec3 hlg_to_sdr_linear(vec3 encoded) {
    vec3 scene = vec3(
        hlg_inv_oetf_channel(encoded.r),
        hlg_inv_oetf_channel(encoded.g),
        hlg_inv_oetf_channel(encoded.b)
    );
    float y_s = max(dot(scene, luma_weights()), 1e-6);
    vec3 display = scene * pow(y_s, 0.2);
    return tone_map_hdr(display * 4.0);
}

vec3 bt2020_to_bt709(vec3 c) {
    return vec3(
        1.6604910 * c.r - 0.5876411 * c.g - 0.0728499 * c.b,
        -0.1245505 * c.r + 1.1328999 * c.g - 0.0083494 * c.b,
        -0.0181508 * c.r - 0.1005789 * c.g + 1.1187297 * c.b
    );
}

void main() {
    vec2 t = gl_FragCoord.xy * window.dv_inv_target.zw;
    vec2 at = window.origin_du.xy
        + t.x * window.origin_du.zw
        + t.y * window.dv_inv_target.xy;
    vec3 rgb = clamp(texture(picture, clamp(at, window.inside.xy, window.inside.zw)).rgb, vec3(0.0), vec3(1.0));
    if (TRANSFER == 1u) {
        rgb = pq_to_sdr_linear(rgb);
    } else if (TRANSFER == 2u) {
        rgb = hlg_to_sdr_linear(rgb);
    } else if (GAMUT_2020 != 0u) {
        rgb = srgb_to_linear(rgb);
    }
    if (GAMUT_2020 != 0u) {
        rgb = clamp(bt2020_to_bt709(rgb), vec3(0.0), vec3(1.0));
    }
    if ((TRANSFER | GAMUT_2020) != 0u) {
        rgb = linear_to_srgb(rgb);
    }
    color = vec4(rgb, 1.0);
}
