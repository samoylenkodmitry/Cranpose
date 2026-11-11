// The sound of an instrument, as the partials a voice adds up and the way
// each of them fades.

pub(super) const PARTIALS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Timbre {
    Piano,
    Organ,
    Bells,
    Pure,
}

impl Timbre {
    pub const ALL: [Self; 4] = [Self::Piano, Self::Organ, Self::Bells, Self::Pure];

    pub fn label(self) -> &'static str {
        match self {
            Self::Piano => "Piano",
            Self::Organ => "Organ",
            Self::Bells => "Bells",
            Self::Pure => "Pure tone",
        }
    }

    pub(super) fn recipe(self) -> &'static Recipe {
        match self {
            Self::Piano => &PIANO,
            Self::Organ => &ORGAN,
            Self::Bells => &BELLS,
            Self::Pure => &PURE,
        }
    }
}

pub(super) struct Recipe {
    // The frequency of each partial as a multiple of the note's frequency.
    pub ratios: [f32; PARTIALS],
    // The loudness of each partial at the hardest strike.
    pub levels: [f32; PARTIALS],
    // How far a string's stiffness raises its upper partials: partial `n`
    // sounds at `ratio * sqrt(1 + stiffness * n * n)`.
    pub stiffness: f32,
    // How much a softer strike darkens the sound: the top partial falls with
    // the velocity raised to this power, the first partial not at all.
    pub brightness: f32,
    pub attack_seconds: f32,
    // Seconds the first partial of the A above middle C takes to fall 60 dB
    // while the key is down. `None` holds the sound as long as the key.
    pub fade_seconds: Option<f32>,
    // How much faster each next partial fades than the one below it.
    pub fade_spread: f32,
    // Seconds the sound takes to fall 60 dB after the damper falls.
    pub release_seconds: f32,
}

const HARMONICS: [f32; PARTIALS] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];

const PIANO: Recipe = Recipe {
    ratios: HARMONICS,
    levels: [1.0, 0.56, 0.34, 0.2, 0.13, 0.08, 0.05, 0.03],
    stiffness: 0.0004,
    brightness: 1.6,
    attack_seconds: 0.003,
    fade_seconds: Some(9.0),
    fade_spread: 0.45,
    release_seconds: 0.22,
};

const ORGAN: Recipe = Recipe {
    ratios: HARMONICS,
    levels: [1.0, 0.6, 0.42, 0.3, 0.0, 0.2, 0.0, 0.14],
    stiffness: 0.0,
    brightness: 0.0,
    attack_seconds: 0.02,
    fade_seconds: None,
    fade_spread: 0.0,
    release_seconds: 0.12,
};

const BELLS: Recipe = Recipe {
    ratios: [1.0, 2.0, 2.76, 4.0, 5.4, 6.0, 8.93, 11.34],
    levels: [1.0, 0.3, 0.5, 0.15, 0.25, 0.08, 0.1, 0.05],
    stiffness: 0.0,
    brightness: 0.8,
    attack_seconds: 0.001,
    fade_seconds: Some(3.5),
    fade_spread: 0.9,
    release_seconds: 0.6,
};

const PURE: Recipe = Recipe {
    ratios: HARMONICS,
    levels: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    stiffness: 0.0,
    brightness: 0.0,
    attack_seconds: 0.008,
    fade_seconds: Some(5.0),
    fade_spread: 0.0,
    release_seconds: 0.25,
};
