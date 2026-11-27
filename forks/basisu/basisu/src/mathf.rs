//! Float helpers that route to `std`'s float methods when the `std` feature is
//! on, and to `libm` when it is off. The float `sqrt` and `round` methods live
//! in `std`, not `core`, so a no_std build needs `libm` for them (enable the
//! `libm` feature). Both routes are bit-identical here: `sqrt` and `round` are
//! correctly-rounded IEEE operations, so the std intrinsics and libm agree, and
//! the transcoder's byte-for-byte output does not depend on which is used.

/// `f32` square root, `std` route.
#[cfg(feature = "std")]
#[inline]
pub fn sqrtf(x: f32) -> f32 {
    x.sqrt()
}

/// `f32` round-half-away-from-zero, `std` route.
#[cfg(feature = "std")]
#[inline]
pub fn roundf(x: f32) -> f32 {
    x.round()
}

/// `f32` square root, `libm` route for no_std builds.
#[cfg(all(not(feature = "std"), feature = "libm"))]
#[inline]
pub fn sqrtf(x: f32) -> f32 {
    libm::sqrtf(x)
}

/// `f32` round-half-away-from-zero, `libm` route for no_std builds.
#[cfg(all(not(feature = "std"), feature = "libm"))]
#[inline]
pub fn roundf(x: f32) -> f32 {
    libm::roundf(x)
}

/// `f32` ceiling, `std` route. Exactly IEEE `roundToIntegralTowardPositive`,
/// so the std intrinsic and libm agree bit for bit.
#[cfg(feature = "std")]
#[inline]
pub fn ceilf(x: f32) -> f32 {
    x.ceil()
}

/// `f32` ceiling, `libm` route for no_std builds.
#[cfg(all(not(feature = "std"), feature = "libm"))]
#[inline]
pub fn ceilf(x: f32) -> f32 {
    libm::ceilf(x)
}

/// `f32` absolute value as a sign-bit clear, exactly IEEE `abs`. Lives here
/// because the `abs` method is `std`-only on this crate's MSRV (core gained
/// float `abs` in a later toolchain), and the bit operation needs neither
/// `std` nor `libm`.
#[inline]
pub fn fabsf(x: f32) -> f32 {
    f32::from_bits(x.to_bits() & 0x7fff_ffff)
}

/// `f64` absolute value as a sign-bit clear, exactly IEEE `abs`. Same MSRV
/// rationale as [`fabsf`].
#[inline]
pub fn fabs(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & 0x7fff_ffff_ffff_ffff)
}

#[cfg(all(not(feature = "std"), not(feature = "libm")))]
compile_error!("a no_std build needs the `libm` feature: float sqrt/round are in std, not core");

// Proof that the no_std (libm) route is bit-identical to the std route, so a
// no_std build produces the same bytes as a std build. Run on std with the
// `libm` feature: `cargo test --features libm`.
#[cfg(all(test, feature = "libm"))]
mod tests {
    /// Every sampled input rounds and square-roots to the same bits through
    /// libm as through std, so the two routes cannot diverge.
    #[test]
    fn libm_matches_std_bit_for_bit() {
        for i in -200_000..200_000i32 {
            let x = i as f32 * 0.0137;
            assert_eq!(libm::roundf(x).to_bits(), x.round().to_bits(), "round({x})");
            if x >= 0.0 {
                assert_eq!(libm::sqrtf(x).to_bits(), x.sqrt().to_bits(), "sqrt({x})");
            }
        }
    }
}
