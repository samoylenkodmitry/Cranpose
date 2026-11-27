//! Half-float (binary16) helpers for the ASTC HDR decode paths. Everything is
//! integer bit construction: the only float-to-half conversion these paths
//! need is `k * (1/65536)` rounded toward zero with integer `k`, which reduces
//! to an exact function of `k` because the scale is a power of two and
//! truncation of an integer-valued mantissa is exact.

/// Whether a half is Inf or NaN (biased exponent all ones).
#[inline]
pub fn is_half_inf_or_nan(h: u16) -> bool {
    (h >> 10) & 0x1F == 0x1F
}

/// Convert `k * 2^-16` for `k` in `[0, 65535]` to a half, rounding toward
/// zero, as pure integer math. `k * 2^-16` has floor exponent `n - 16` where
/// `n = floor(log2 k)`; `n < 2` lands on the half denormal branch
/// (`trunc(2^24 * v) = k << 8`), and otherwise the biased exponent is `n - 1`
/// with the mantissa field the top ten fraction bits of `k` below bit `n`,
/// truncated toward zero.
pub fn half_from_unorm16(k: u32) -> u16 {
    debug_assert!(k <= 0xFFFF);
    if k == 0 {
        return 0;
    }
    let n = 31 - k.leading_zeros();
    if n < 2 {
        return (k << 8) as u16;
    }
    let m = if n >= 10 {
        (k >> (n - 10)) & 0x3FF
    } else {
        (k << (10 - n)) & 0x3FF
    };
    (((n - 1) << 10) | m) as u16
}

/// Half to f32, exact bit math. Only used to feed the float RGB9E5 packer for
/// solid HDR blocks; the inputs there are finite because HDR void extents with
/// Inf/NaN components are rejected at unpack, but the Inf/NaN branches are kept
/// for completeness.
pub fn half_to_float(h: u16) -> f32 {
    let s = (h as u32 >> 15) & 1;
    let mut e = (h as u32 >> 10) & 0x1F;
    let mut m = h as u32 & 0x3FF;

    if e == 0 {
        if m == 0 {
            return f32::from_bits(s << 31);
        }
        // Denormal: renormalize the mantissa.
        while m & 0x400 == 0 {
            m <<= 1;
            e = e.wrapping_sub(1);
        }
        e = e.wrapping_add(1);
        m &= !0x400;
    } else if e == 31 {
        if m == 0 {
            return f32::from_bits((s << 31) | 0x7F80_0000);
        }
        return f32::from_bits((s << 31) | 0x7F80_0000 | (m << 13));
    }

    let e = e.wrapping_add(127 - 15);
    f32::from_bits((m << 13) | (e << 23) | (s << 31))
}

/// Quantized-log16 to half: the top five bits are the half exponent and the
/// low eleven bits map to the ten-bit half mantissa through a three-segment
/// piecewise-linear curve.
#[inline]
pub fn qlog16_to_half(k: i32) -> u16 {
    debug_assert!((0..=0xFFFF).contains(&k));
    let e = (k & 0xF800) >> 11;
    let m = k & 0x7FF;
    let mt = if m < 512 {
        3 * m
    } else if m >= 1536 {
        5 * m - 2048
    } else {
        4 * m - 512
    };
    ((e << 10) + (mt >> 3)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Float-to-half rounded toward zero, restricted to nonnegative finite
    /// inputs, computed with exact integer reasoning on the f32 bit pattern
    /// (truncation of an exactly power-of-two-scaled integer is a shift, so no
    /// float library calls are needed). Trusted oracle for the tests below.
    fn float_to_half_toward_zero(val: f32) -> u16 {
        let bits = val.to_bits();
        let flt_m = bits & 0x7F_FFFF;
        let flt_e = (bits >> 23) & 0xFF;
        assert_eq!(bits >> 31, 0);
        assert_ne!(flt_e, 0xFF);
        let (mut e, mut m) = (0u32, 0u32);
        if flt_e != 0 {
            let new_exp = flt_e as i32 - 127;
            if new_exp > 15 {
                e = 31;
            } else if new_exp < -14 {
                // truncf((1 << 24) * val): scale the significand exactly.
                let sig = (flt_m | 0x80_0000) as u64;
                let shift = 24 + new_exp - 23; // val = sig * 2^(new_exp - 23)
                m = if shift >= 0 {
                    (sig << shift) as u32
                } else {
                    (sig >> -shift) as u32
                };
            } else {
                e = (new_exp + 15) as u32;
                m = flt_m >> 13; // truncf(flt_m / 8192.0)
            }
        }
        if m == 1024 {
            e += 1;
            m = 0;
        }
        ((e << 10) | m) as u16
    }

    #[test]
    fn half_from_unorm16_matches_reference_formula() {
        for k in 0u32..=0xFFFF {
            let f = k as f32 * (1.0 / 65536.0);
            assert_eq!(half_from_unorm16(k), float_to_half_toward_zero(f), "k={k}");
        }
    }

    #[test]
    fn half_to_float_matches_ieee_values() {
        for h in 0u16..0x7C00 {
            let f = half_to_float(h);
            let (e, m) = ((h as u32 >> 10) & 0x1F, h as u32 & 0x3FF);
            let want = if e == 0 {
                // Denormal or zero: value = m * 2^-24, exactly representable.
                m as f32 * f32::from_bits((127 - 24) << 23)
            } else {
                f32::from_bits(((e + 112) << 23) | (m << 13))
            };
            assert_eq!(f.to_bits(), want.to_bits(), "h={h:#06x}");
        }
    }
}
