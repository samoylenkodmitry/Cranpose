/// A text measurement given in scaled pixels or as a multiple of the font size.
/// Compose's `TextUnit`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum TextUnit {
    /// No value is set. The property takes its value from the enclosing style or a default.
    #[default]
    Unspecified,
    /// Scaled pixels: logical pixels that also follow the user's font scale setting.
    Sp(f32),
    /// A multiple of the font size, so `Em(1.5)` is one and a half times the font size.
    Em(f32),
}

impl TextUnit {
    /// Returns true for `Unspecified`.
    pub fn is_unspecified(&self) -> bool {
        matches!(self, Self::Unspecified)
    }

    /// Returns true for `Sp` and `Em`.
    pub fn is_specified(&self) -> bool {
        !self.is_unspecified()
    }

    /// Returns the number inside `Sp` or `Em`, or `NaN` for `Unspecified`.
    pub fn value(&self) -> f32 {
        match self {
            Self::Unspecified => f32::NAN,
            Self::Sp(v) => *v,
            Self::Em(v) => *v,
        }
    }
}
