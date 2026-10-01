//! The system font-size setting, as the curve it is and not the multiplier it
//! looks like.
//!
//! On Android 13 and below the platform resolves a size in `Sp` as
//! `sp * font_scale` dp. Android 14 does not: above a threshold setting it runs
//! the sp value through a piecewise-linear table, so small text grows by the
//! full setting and large text grows by less.
//! `TypedValue.applyDimension(COMPLEX_UNIT_SP, ..)` answers from that table, and
//! Jetpack Compose carries its own copy so its `Density` answers the same.
//!
//! Measured on a Wear OS 5 emulator at density 2.0 with the setting at 1.24
//! (`TypedValue.applyDimension` and `androidx.compose.ui.unit.Density(context)`
//! agree to the last bit on every sample):
//!
//! | sp | multiplied | platform |
//! |---|---|---|
//! | 12 | 29.76 px | 29.76 px |
//! | 13 | 32.24 px | **32.72 px** |
//! | 14 | 34.72 px | **35.68 px** |
//! | 16 | 39.68 px | **38.72 px** |
//! | 19 | 47.12 px | **43.76 px** |
//!
//! So multiplying is wrong in both directions at once, and by enough to change
//! where a line wraps. A [`FontScaleCurve`] carries the platform's answer
//! instead: the host samples the real conversion when the configuration says it
//! changed, and everything that resolves an `Sp` goes through
//! [`FontScaleCurve::sp_to_dp`].
//!
//! Off Android, and on an Android below the version that has a table, the curve
//! holds no knots and `sp_to_dp` is the multiplication again — which is what
//! those platforms actually do.

/// How many knots a curve keeps.
///
/// The platform's own table has ten, and collapsing the points that sit on a
/// straight line through their neighbours leaves fewer, so this is headroom
/// rather than a limit anyone is expected to reach. It is a hard cap because a
/// curve is `Copy` and rides inside a type that is passed by value on the
/// measurement path.
pub const MAX_FONT_SCALE_KNOTS: usize = 12;

const COLLINEAR_EPSILON_DP: f32 = 1.0e-3;

/// The sp → dp conversion the platform performs, sampled from the platform.
///
/// With no knots this is `sp * scale`. With knots it is the piecewise-linear
/// function through them, extended past both ends by the ratio of the knot on
/// that end — which is how the platform extends its own table, so a size below
/// the first knot scales by the plain setting and one above the last keeps the
/// last knot's ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontScaleCurve {
    scale: f32,
    knots: [(f32, f32); MAX_FONT_SCALE_KNOTS],
    len: usize,
    fingerprint: u32,
}

impl Default for FontScaleCurve {
    fn default() -> Self {
        Self::linear(1.0)
    }
}

impl FontScaleCurve {
    /// The plain multiplier: what every platform without a conversion table
    /// does, and what Android itself did before it had one.
    pub const fn linear(scale: f32) -> Self {
        Self {
            scale,
            knots: [(0.0, 0.0); MAX_FONT_SCALE_KNOTS],
            len: 0,
            fingerprint: 0,
        }
    }

    /// A curve through `samples`, which are `(sp, dp)` pairs read from the
    /// platform in ascending sp order.
    ///
    /// Samples that lie on the straight line between their neighbours are
    /// dropped, so a densely sampled table comes back as the handful of points
    /// that actually bend. Anything the platform could not have produced — an
    /// empty or unsorted set, a non-finite or non-positive value, or more bends
    /// than [`MAX_FONT_SCALE_KNOTS`] — is refused, and the caller gets the
    /// multiplier rather than a curve nobody measured.
    pub fn from_samples(scale: f32, samples: &[(f32, f32)]) -> Self {
        let Some(kept) = compress(samples) else {
            return Self::linear(scale);
        };
        let mut curve = Self::linear(scale);
        for (index, knot) in kept.iter().enumerate() {
            curve.knots[index] = *knot;
        }
        curve.len = kept.len();
        curve.fingerprint = fingerprint(scale, &kept);
        curve
    }

    /// The setting itself — what the user chose, whatever the table then does
    /// with an individual size. This is the number to report, not the number to
    /// multiply by.
    pub fn scale(self) -> f32 {
        self.scale
    }

    /// Whether this is the plain multiplier, with no table behind it.
    pub fn is_linear(self) -> bool {
        self.len == 0
    }

    /// Whether resolving an `Sp` through this curve changes nothing at all.
    ///
    /// Asked of the conversion and not of the representation: a platform that
    /// hands back a table while the setting sits at 1.0 hands back a table
    /// whose every knot maps a size to itself, and that is still nothing to do.
    pub fn is_identity(self) -> bool {
        if self.len == 0 {
            return (self.scale - 1.0).abs() <= f32::EPSILON;
        }
        self.knots[..self.len]
            .iter()
            .all(|(sp, dp)| (dp - sp).abs() <= COLLINEAR_EPSILON_DP)
    }

    /// The knots, ascending by sp. Empty for a linear curve.
    pub fn knots(self) -> [(f32, f32); MAX_FONT_SCALE_KNOTS] {
        self.knots
    }

    /// How many of [`FontScaleCurve::knots`] are used.
    pub fn knot_count(self) -> usize {
        self.len
    }

    /// A cheap identity for cache keys: two curves that convert identically
    /// share it, and one that does not is overwhelmingly unlikely to.
    pub fn fingerprint(self) -> u32 {
        self.fingerprint ^ self.scale.to_bits()
    }

    /// A size in scale-independent pixels, in dp.
    ///
    /// This is `FontScaleConverterImpl.convertSpToDp`: the sign is carried
    /// separately, an exact hit on a knot returns that knot, a size outside the
    /// table scales by the ratio of the nearest end, and everything between two
    /// knots is interpolated.
    pub fn sp_to_dp(self, sp: f32) -> f32 {
        if !sp.is_finite() {
            return sp;
        }
        if self.len == 0 {
            return sp * self.scale;
        }
        let magnitude = sp.abs();
        let sign = if sp.is_sign_negative() { -1.0 } else { 1.0 };
        let knots = &self.knots[..self.len];
        let (first_sp, first_dp) = knots[0];
        if magnitude <= first_sp {
            return sign * magnitude * (first_dp / first_sp);
        }
        let (last_sp, last_dp) = knots[self.len - 1];
        if magnitude >= last_sp {
            return sign * magnitude * (last_dp / last_sp);
        }
        for window in knots.windows(2) {
            let (low_sp, low_dp) = window[0];
            let (high_sp, high_dp) = window[1];
            if magnitude <= high_sp {
                let t = (magnitude - low_sp) / (high_sp - low_sp);
                return sign * (low_dp + (high_dp - low_dp) * t);
            }
        }
        sign * magnitude * (last_dp / last_sp)
    }
}

fn compress(samples: &[(f32, f32)]) -> Option<Vec<(f32, f32)>> {
    if samples.len() < 2 {
        return None;
    }
    let mut previous_sp = 0.0f32;
    for (sp, dp) in samples {
        if !sp.is_finite() || !dp.is_finite() || *sp <= previous_sp || *dp <= 0.0 {
            return None;
        }
        previous_sp = *sp;
    }
    let mut kept: Vec<(f32, f32)> = Vec::with_capacity(samples.len());
    kept.push(samples[0]);
    for index in 1..samples.len() - 1 {
        let (low_sp, low_dp) = *kept.last().expect("the first sample was pushed");
        let (sp, dp) = samples[index];
        let (high_sp, high_dp) = samples[index + 1];
        let t = (sp - low_sp) / (high_sp - low_sp);
        let straight = low_dp + (high_dp - low_dp) * t;
        if (dp - straight).abs() > COLLINEAR_EPSILON_DP {
            kept.push(samples[index]);
        }
    }
    kept.push(samples[samples.len() - 1]);
    if kept.len() > MAX_FONT_SCALE_KNOTS {
        return None;
    }
    Some(kept)
}

fn fingerprint(scale: f32, knots: &[(f32, f32)]) -> u32 {
    let mut hash = 2166136261u32;
    let mut mix = |bits: u32| {
        hash ^= bits;
        hash = hash.wrapping_mul(16777619);
    };
    mix(scale.to_bits());
    for (sp, dp) in knots {
        mix(sp.to_bits());
        mix(dp.to_bits());
    }
    hash
}

#[cfg(test)]
#[path = "tests/font_scale_tests.rs"]
mod tests;
