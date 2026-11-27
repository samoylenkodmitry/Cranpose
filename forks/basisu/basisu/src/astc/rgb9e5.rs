//! RGB9E5 (shared-exponent) packing for the ASTC 9E5 decode path: the two
//! integer per-texel packers (LDR 16-bit interpolants and HDR half floats)
//! plus the float packer used only for void-extent (solid) blocks. The float
//! path is deterministic without libm: the exponent scale is built from bits,
//! division by a power of two is exact, and `floorf(x + 0.5)` of a
//! nonnegative value below 2^23 is an exact add plus truncation.

/// Largest value representable in RGB9E5 (`MAX_RGB9E5` = 0xff80): mantissa
/// 511/512 at the maximum exponent.
const MAX_RGB9E5: f32 = 0xFF80 as f32;

/// Count leading zeros of a 17-bit value (`clz17`).
fn clz17(x: u32) -> i32 {
    debug_assert!(x <= 0x1FFFF);
    let x = x & 0x1FFFF;
    if x == 0 {
        return 17;
    }
    (x.leading_zeros() as i32) - 15
}

/// Pack three LDR 16-bit interpolants ([0,65535], 1.0 = 65535) to RGB9E5
/// (`pack_rgb9e5_ldr_astc`): normalize by the joint leading-zero count, take
/// nine mantissa bits from each, exponent `16 - lz`.
pub fn pack_rgb9e5_ldr_astc(cr: i32, cg: i32, cb: i32) -> u32 {
    let mut lz = clz17((cr | cg | cb | 1) as u32);
    let mut cr = cr;
    let mut cg = cg;
    let mut cb = cb;
    if cr == 65535 {
        cr = 65536;
        lz = 0;
    }
    if cg == 65535 {
        cg = 65536;
        lz = 0;
    }
    if cb == 65535 {
        cb = 65536;
        lz = 0;
    }
    cr <<= lz;
    cg <<= lz;
    cb <<= lz;
    let cr = ((cr >> 8) & 0x1FF) as u32;
    let cg = ((cg >> 8) & 0x1FF) as u32;
    let cb = ((cb >> 8) & 0x1FF) as u32;
    let exponent = (16 - lz) as u32;
    (exponent << 27) | (cb << 18) | (cg << 9) | cr
}

/// Pack three half floats to RGB9E5 (`pack_rgb9e5_hdr_astc`): out-of-range
/// halves clamp (above Inf to 0, Inf to the largest finite half), then the
/// max-exponent component keeps nine mantissa bits and the others shift right
/// by their exponent deficit.
pub fn pack_rgb9e5_hdr_astc(cr: i32, cg: i32, cb: i32) -> u32 {
    let fix = |c: i32| {
        if c > 0x7C00 {
            0
        } else if c == 0x7C00 {
            0x7BFF
        } else {
            c
        }
    };
    let cr = fix(cr);
    let cg = fix(cg);
    let cb = fix(cb);

    let re = (cr >> 10) & 0x1F;
    let ge = (cg >> 10) & 0x1F;
    let be = (cb >> 10) & 0x1F;
    let rex = if re == 0 { 1 } else { re };
    let gex = if ge == 0 { 1 } else { ge };
    let bex = if be == 0 { 1 } else { be };
    let xm = ((cr | cg | cb) & 0x200) >> 9;
    let xe = re | ge | be;

    let (expo, rshift, gshift, bshift) = if xe == 0 {
        (xm, xm, xm, xm)
    } else if re >= ge && re >= be {
        (rex + 1, 2, rex - gex + 2, rex - bex + 2)
    } else if ge >= be {
        (gex + 1, gex - rex + 2, 2, gex - bex + 2)
    } else {
        (bex + 1, bex - rex + 2, bex - gex + 2, 2)
    };

    let rm = (cr & 0x3FF) | if re == 0 { 0 } else { 0x400 };
    let gm = (cg & 0x3FF) | if ge == 0 { 0 } else { 0x400 };
    let bm = (cb & 0x3FF) | if be == 0 { 0 } else { 0x400 };
    let rm = ((rm >> rshift) & 0x1FF) as u32;
    let gm = ((gm >> gshift) & 0x1FF) as u32;
    let bm = ((bm >> bshift) & 0x1FF) as u32;

    ((expo as u32) << 27) | (bm << 18) | (gm << 9) | rm
}

/// Clamp with an explicit comparison order (NaN would pass through, but no NaN
/// reaches this path).
#[inline]
fn clampf(a: f32, l: f32, h: f32) -> f32 {
    if a < l {
        l
    } else if a > h {
        h
    } else {
        a
    }
}

/// The float exponent field minus the bias (`floor_log2`); not correct for
/// zero and denormals, which the caller hides behind a max with the minimum
/// RGB9E5 exponent.
#[inline]
fn floor_log2(x: f32) -> i32 {
    ((x.to_bits() >> 23) & 0xFF) as i32 - 127
}

/// `2^e` as an f32 built from bits, avoiding a `powf` call (every conforming
/// libm returns powers of two exactly, and constructing the bits removes the
/// libm dependence entirely). `e` must be a normal exponent.
#[inline]
fn exp2i(e: i32) -> f32 {
    debug_assert!((-126..=127).contains(&e));
    f32::from_bits(((e + 127) as u32) << 23)
}

/// Pack three floats to RGB9E5, used for void-extent blocks: shared exponent
/// from the max component, mantissas rounded half-up. The `(x + 0.5) as i32`
/// cast implements `floor(x + 0.5)` because truncation equals floor for the
/// nonnegative values here.
pub fn pack_rgb9e5(r: f32, g: f32, b: f32) -> u32 {
    let r = clampf(r, 0.0, MAX_RGB9E5);
    let g = clampf(g, 0.0, MAX_RGB9E5);
    let b = clampf(b, 0.0, MAX_RGB9E5);

    let maxrgb = if r > g { r } else { g };
    let maxrgb = if maxrgb > b { maxrgb } else { b };

    // RGB9E5_EXP_BIAS = 15, RGB9E5_MANTISSA_BITS = 9.
    let mut exp_shared = floor_log2(maxrgb).max(-16) + 1 + 15;
    debug_assert!((0..=31).contains(&exp_shared));

    let mut denom = exp2i(exp_shared - 15 - 9);

    let maxm = (maxrgb / denom + 0.5) as i32;
    if maxm == 512 {
        denom *= 2.0;
        exp_shared += 1;
        debug_assert!(exp_shared <= 31);
    }

    let rm = (r / denom + 0.5) as u32;
    let gm = (g / denom + 0.5) as u32;
    let bm = (b / denom + 0.5) as u32;
    debug_assert!(rm <= 511 && gm <= 511 && bm <= 511);

    rm | (gm << 9) | (bm << 18) | ((exp_shared as u32) << 27)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_rgb9e5_known_values() {
        // 1.0: exponent 16, mantissa 256 in every packed channel.
        let one = pack_rgb9e5(1.0, 1.0, 1.0);
        assert_eq!(one >> 27, 16);
        assert_eq!(one & 0x1FF, 256);
        // Zero packs to all-zero fields.
        assert_eq!(pack_rgb9e5(0.0, 0.0, 0.0), 0);
        // The magenta error color (never observable output here, but a handy
        // fixed point): purple = (1, 0, 1).
        let purple = pack_rgb9e5(1.0, 0.0, 1.0);
        assert_eq!(purple >> 27, 16);
        assert_eq!(purple & 0x1FF, 256);
        assert_eq!((purple >> 9) & 0x1FF, 0);
        assert_eq!((purple >> 18) & 0x1FF, 256);
    }

    #[test]
    fn ldr_packer_saturates_at_one() {
        // 65535 is promoted to 65536 with a forced zero shift, giving mantissa
        // 256 at exponent 16 (exactly 1.0).
        let p = pack_rgb9e5_ldr_astc(65535, 65535, 65535);
        assert_eq!(p >> 27, 16);
        assert_eq!(p & 0x1FF, 256);
    }
}
