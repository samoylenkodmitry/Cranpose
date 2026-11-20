//! How the level of a sound falls with the distance of its source.

/// How the level of a point source falls with its distance from the listener. Jetpack XR's
/// `DistanceAttenuation`, which has a `Custom` curve that Cranpose does not offer.
///
/// Distances are in meters. A gain of 1 is the level of the sound's own recording. The panner
/// applies the gain to both ears alike.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum DistanceAttenuation {
    /// Lets Cranpose choose: [`Natural`](Self::Natural) from one meter.
    #[default]
    Auto,
    /// The level does not change with the distance.
    None,
    /// The gain is 1 up to `min_distance` and falls in a straight line to `gain_at_max_distance`
    /// at `max_distance`. It stays there beyond.
    Linear {
        /// The distance where the fall begins. It is at least
        /// [`MIN_DISTANCE`](DistanceAttenuation::MIN_DISTANCE).
        min_distance: f32,
        /// The distance where the fall ends. It is at least `min_distance`.
        max_distance: f32,
        /// The gain from `max_distance` on, between 0 and 1.
        gain_at_max_distance: f32,
    },
    /// The gain is 1 up to `min_distance` and falls as the inverse of the distance beyond it,
    /// which is 6 dB for each doubling, as the level of a point source in open air does.
    Natural {
        /// The distance where the fall begins. It is at least
        /// [`MIN_DISTANCE`](DistanceAttenuation::MIN_DISTANCE).
        min_distance: f32,
    },
}

impl DistanceAttenuation {
    /// The smallest `min_distance` that a curve honors, in meters. A smaller one counts as this.
    pub const MIN_DISTANCE: f32 = 0.1;

    // The distance where `Auto` starts to fall, in meters.
    const AUTO_MIN_DISTANCE: f32 = 1.0;

    /// The gain at `distance` meters.
    pub fn gain(self, distance: f32) -> f32 {
        let distance = if distance.is_finite() {
            distance.max(0.0)
        } else {
            0.0
        };
        match self {
            Self::None => 1.0,
            Self::Auto => natural_gain(Self::AUTO_MIN_DISTANCE, distance),
            Self::Natural { min_distance } => natural_gain(min_distance, distance),
            Self::Linear {
                min_distance,
                max_distance,
                gain_at_max_distance,
            } => {
                let min = clean_min(min_distance);
                let max = if max_distance.is_finite() {
                    max_distance.max(min)
                } else {
                    min
                };
                let floor = if gain_at_max_distance.is_finite() {
                    gain_at_max_distance.clamp(0.0, 1.0)
                } else {
                    1.0
                };
                if distance <= min {
                    1.0
                } else if distance >= max {
                    floor
                } else {
                    1.0 + (floor - 1.0) * (distance - min) / (max - min)
                }
            }
        }
    }

    // The curve as four words: its kind and its three numbers.
    pub(crate) fn to_words(self) -> [u32; 4] {
        let (kind, a, b, c) = match self {
            Self::Auto => (0, 0.0, 0.0, 0.0),
            Self::None => (1, 0.0, 0.0, 0.0),
            Self::Linear {
                min_distance,
                max_distance,
                gain_at_max_distance,
            } => (2, min_distance, max_distance, gain_at_max_distance),
            Self::Natural { min_distance } => (3, min_distance, 0.0, 0.0),
        };
        [kind, a.to_bits(), b.to_bits(), c.to_bits()]
    }

    pub(crate) fn from_words(words: [u32; 4]) -> Self {
        let [kind, a, b, c] = words;
        let [a, b, c] = [a, b, c].map(f32::from_bits);
        match kind {
            1 => Self::None,
            2 => Self::Linear {
                min_distance: a,
                max_distance: b,
                gain_at_max_distance: c,
            },
            3 => Self::Natural { min_distance: a },
            _ => Self::Auto,
        }
    }
}

fn clean_min(min_distance: f32) -> f32 {
    if min_distance.is_finite() {
        min_distance.max(DistanceAttenuation::MIN_DISTANCE)
    } else {
        DistanceAttenuation::MIN_DISTANCE
    }
}

fn natural_gain(min_distance: f32, distance: f32) -> f32 {
    let min = clean_min(min_distance);
    if distance <= min { 1.0 } else { min / distance }
}

#[cfg(test)]
#[path = "tests/distance_attenuation_tests.rs"]
mod tests;
