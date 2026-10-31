//! Float rounding without a library call.
//!
//! A target with no instruction that rounds a float, as the ESP32-S3,
//! 32-bit ARM and baseline x86 are, calls `floorf`, `ceilf`, `roundf` and
//! `truncf` for the `f32` methods, and layout snapping and the renderers
//! place pixels with dozens of them a frame. There these functions round
//! inline: through the integer conversion and the exact remainder, or, on
//! 32-bit ARM, on the bits. Wasm rounds halves only to even, so its `round`
//! truncates instead. Every result is the one the method gives, bit for
//! bit, signed zeros, infinities and NaN included.

// How the target rounds a float.
enum Rounding {
    // In one instruction, which the method compiles to.
    Hardware,
    // Through the integer conversion and the exact remainder.
    Conversion,
    // On the bits in core registers. On 32-bit ARM each float compare
    // moves its flags out of the FPU, and the in-order cores wait for it.
    Bits,
}

const ROUNDING: Rounding = if cfg!(target_arch = "arm") {
    Rounding::Bits
} else if cfg!(any(
    target_arch = "xtensa",
    all(
        any(target_arch = "x86", target_arch = "x86_64"),
        not(target_feature = "sse4.1")
    )
)) {
    Rounding::Conversion
} else {
    Rounding::Hardware
};

/// From this magnitude up every `f32` is whole.
const WHOLE_FROM: f32 = 8_388_608.0;

/// `value` without its fraction, and the fraction, for a magnitude below
/// 2^23, where both are exact; `None` for a value that is whole already,
/// infinite or NaN.
#[inline(always)]
fn split(value: f32) -> Option<(f32, f32)> {
    // A NaN fails the comparison too.
    (value.abs() < WHOLE_FROM).then(|| {
        let whole = (value as i32) as f32;
        (whole, value - whole)
    })
}

// The bits of a float, by the exponent of its units bit. The exponent is
// biased: `ONE` is the exponent of 1, and from `WHOLE` up the lowest
// mantissa bit counts units, so the float is whole, infinite or NaN.
macro_rules! float_bits {
    ($module:ident: $float:ty, $bits:ty, $exponent_bits:literal, $mantissa_bits:literal, { $($more:item)* }) => {
        mod $module {
            const MANTISSA_BITS: u32 = $mantissa_bits;
            const ONE: $bits = (1 << ($exponent_bits - 1)) - 1;
            const WHOLE: $bits = ONE + $mantissa_bits;
            const SIGN: $bits = 1 << (<$bits>::BITS - 1);
            const MANTISSA: $bits = (1 << MANTISSA_BITS) - 1;

            // The bits of `value`, whether it is negative, and its exponent.
            #[inline]
            fn parts(value: $float) -> ($bits, bool, $bits) {
                let bits = value.to_bits();
                let exponent = (bits >> MANTISSA_BITS) & ((1 << $exponent_bits) - 1);
                (bits, (bits & SIGN) != 0, exponent)
            }

            // The mantissa bits below the units bit of a magnitude from 1
            // below `WHOLE`.
            #[inline]
            fn fraction(exponent: $bits) -> $bits {
                MANTISSA >> (exponent - ONE)
            }

            #[inline]
            pub(super) fn round(value: $float) -> $float {
                let (bits, _, exponent) = parts(value);
                if exponent >= WHOLE {
                    return value;
                }
                if exponent < ONE - 1 {
                    // Below one half.
                    return <$float>::from_bits(bits & SIGN);
                }
                if exponent < ONE {
                    // From one half below 1.
                    return <$float>::from_bits((bits & SIGN) | <$float>::to_bits(1.0));
                }
                // Adding the half bit carries a half or more into the units.
                let fraction = fraction(exponent);
                <$float>::from_bits((bits + (fraction >> 1) + 1) & !fraction)
            }

            $($more)*
        }
    };
}

float_bits!(f64_bits: f64, u64, 11, 52, {});
float_bits!(f32_bits: f32, u32, 8, 23, {
    #[inline]
    pub(super) fn floor(value: f32) -> f32 {
        let (bits, negative, exponent) = parts(value);
        if exponent >= WHOLE {
            return value;
        }
        if exponent < ONE {
            // Zeros stay; others go to 0 or -1.
            return match (negative, (bits & !SIGN) == 0) {
                (_, true) => value,
                (false, false) => 0.0,
                (true, false) => -1.0,
            };
        }
        let fraction = fraction(exponent);
        let up = if negative && (bits & fraction) != 0 { fraction } else { 0 };
        f32::from_bits((bits + up) & !fraction)
    }

    #[inline]
    pub(super) fn ceil(value: f32) -> f32 {
        let (bits, negative, exponent) = parts(value);
        if exponent >= WHOLE {
            return value;
        }
        if exponent < ONE {
            // Zeros stay; others go to -0 or 1.
            return match (negative, (bits & !SIGN) == 0) {
                (_, true) => value,
                (true, false) => -0.0,
                (false, false) => 1.0,
            };
        }
        let fraction = fraction(exponent);
        let up = if !negative && (bits & fraction) != 0 { fraction } else { 0 };
        f32::from_bits((bits + up) & !fraction)
    }

    #[inline]
    pub(super) fn trunc(value: f32) -> f32 {
        let (bits, _, exponent) = parts(value);
        if exponent >= WHOLE {
            return value;
        }
        if exponent < ONE {
            return f32::from_bits(bits & SIGN);
        }
        f32::from_bits(bits & !fraction(exponent))
    }
});

/// `value.floor()`.
#[inline(always)]
pub fn floor(value: f32) -> f32 {
    match ROUNDING {
        Rounding::Hardware => value.floor(),
        Rounding::Bits => f32_bits::floor(value),
        Rounding::Conversion => match split(value) {
            Some((whole, fraction)) if fraction < 0.0 => whole - 1.0,
            Some((whole, _)) => whole.copysign(value),
            None => value,
        },
    }
}

/// `value.ceil()`.
#[inline(always)]
pub fn ceil(value: f32) -> f32 {
    match ROUNDING {
        Rounding::Hardware => value.ceil(),
        Rounding::Bits => f32_bits::ceil(value),
        Rounding::Conversion => match split(value) {
            Some((whole, fraction)) if fraction > 0.0 => whole + 1.0,
            Some((whole, _)) => whole.copysign(value),
            None => value,
        },
    }
}

/// `value.round()`: halves away from zero.
#[inline(always)]
pub fn round(value: f32) -> f32 {
    if cfg!(target_arch = "wasm32") {
        // The float just below one half carries a fraction of a half or
        // more over the next whole number and no smaller one.
        return (value + 0.499_999_97_f32.copysign(value)).trunc();
    }
    match ROUNDING {
        Rounding::Hardware => value.round(),
        Rounding::Bits => f32_bits::round(value),
        Rounding::Conversion => match split(value) {
            Some((whole, fraction)) if fraction >= 0.5 => whole + 1.0,
            Some((whole, fraction)) if fraction <= -0.5 => whole - 1.0,
            Some((whole, _)) => whole.copysign(value),
            None => value,
        },
    }
}

/// `value.trunc()`.
#[inline(always)]
pub fn trunc(value: f32) -> f32 {
    match ROUNDING {
        Rounding::Hardware => value.trunc(),
        Rounding::Bits => f32_bits::trunc(value),
        Rounding::Conversion => match split(value) {
            Some((whole, _)) => whole.copysign(value),
            None => value,
        },
    }
}

/// `value.fract()`: `value` less its [`trunc`].
#[inline]
pub fn fract(value: f32) -> f32 {
    value - trunc(value)
}

// From this magnitude up the whole part of an `f64` does not fit `i32`.
const I32_FROM: f64 = 2_147_483_648.0;

/// `value.round()` for an `f64`: halves away from zero.
#[inline(always)]
pub fn round_f64(value: f64) -> f64 {
    if cfg!(target_arch = "wasm32") {
        // As in `round`, with the `f64` just below one half.
        return (value + 0.499_999_999_999_999_94_f64.copysign(value)).trunc();
    }
    match ROUNDING {
        Rounding::Hardware => value.round(),
        Rounding::Bits => f64_bits::round(value),
        Rounding::Conversion if value.abs() >= I32_FROM || value.is_nan() => value.round(),
        Rounding::Conversion => {
            // Below 2^31 the whole part and the remainder are exact.
            let whole = f64::from(value as i32);
            let fraction = value - whole;
            if fraction >= 0.5 {
                whole + 1.0
            } else if fraction <= -0.5 {
                whole - 1.0
            } else {
                whole.copysign(value)
            }
        }
    }
}

/// `value` as a whole number, its fraction dropped toward zero: what a
/// pixel position or count rounded with [`floor`], [`ceil`] or [`round`]
/// holds. Saturates at the ends of `i32`; 0 for NaN.
#[inline(always)]
pub fn to_i32(value: f32) -> i32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the conversion saturates, which is what a pixel position wants"
    )]
    let whole = value as i32;
    whole
}
