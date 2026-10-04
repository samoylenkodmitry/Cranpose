//! Optional multilingual font data for Cranpose applications.

/// A font family and its requested variable weights, backed by static bytes.
#[derive(Clone, Copy, Debug)]
pub struct FontPack {
    family_name: &'static str,
    bytes: &'static [u8],
    weights: &'static [u16],
}

impl FontPack {
    /// Creates a font pack from a family name, static font bytes, and `wght`
    /// axis coordinates for the app's text styles.
    pub const fn new(
        family_name: &'static str,
        bytes: &'static [u8],
        weights: &'static [u16],
    ) -> Self {
        Self {
            family_name,
            bytes,
            weights,
        }
    }

    /// Returns the family name used to register the font.
    pub const fn family_name(&self) -> &'static str {
        self.family_name
    }

    /// Returns the static font bytes shared by all registered weights.
    pub const fn bytes(&self) -> &'static [u8] {
        self.bytes
    }

    /// Returns the variable weights the pack registers.
    pub const fn weights(&self) -> &'static [u16] {
        self.weights
    }
}

const UI_WEIGHTS: &[u16] = &[400, 500, 600, 700, 800];

/// The Noto Sans Arabic variable font pack, enabled by the `arabic` feature.
#[cfg(feature = "arabic")]
pub const ARABIC: FontPack = FontPack::new(
    "Noto Sans Arabic",
    include_bytes!("../assets/NotoSansArabic-VF.ttf"),
    UI_WEIGHTS,
);

/// The Noto Sans Devanagari variable font pack, enabled by the `devanagari` feature.
#[cfg(feature = "devanagari")]
pub const DEVANAGARI: FontPack = FontPack::new(
    "Noto Sans Devanagari",
    include_bytes!("../assets/NotoSansDevanagari-VF.ttf"),
    UI_WEIGHTS,
);

/// The Noto Sans Simplified Chinese variable font pack, enabled by the `cjk` feature.
#[cfg(feature = "cjk")]
pub const CJK: FontPack = FontPack::new(
    "Noto Sans CJK SC",
    include_bytes!(concat!(env!("OUT_DIR"), "/CJK.ttf")),
    UI_WEIGHTS,
);
