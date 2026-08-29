#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum TextUnit {
    #[default]
    Unspecified,
    Sp(f32),
    Em(f32),
}

impl TextUnit {
    pub fn is_unspecified(&self) -> bool {
        matches!(self, TextUnit::Unspecified)
    }

    pub fn is_specified(&self) -> bool {
        !self.is_unspecified()
    }

    pub fn value(&self) -> f32 {
        match self {
            TextUnit::Unspecified => f32::NAN,
            TextUnit::Sp(v) => *v,
            TextUnit::Em(v) => *v,
        }
    }
}
