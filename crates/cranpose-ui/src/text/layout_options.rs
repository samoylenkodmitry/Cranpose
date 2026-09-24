use std::hash::{Hash, Hasher};

const MIN_SCALE_DOWN_FONT_SIZE_SP: f32 = 1.0;

/// How overflowing text should be handled.
#[derive(Clone, Copy, Debug, Default)]
pub enum TextOverflow {
    #[default]
    Clip,
    Ellipsis,
    Visible,
    StartEllipsis,
    MiddleEllipsis,
    ScaleDown {
        min_font_size_sp: f32,
    },
}

impl TextOverflow {
    pub fn normalized(self) -> Self {
        match self {
            Self::ScaleDown { min_font_size_sp } => Self::ScaleDown {
                min_font_size_sp: normalize_scale_down_min_font_size_sp(min_font_size_sp),
            },
            other => other,
        }
    }

    pub fn scale_down_min_font_size_sp(self) -> Option<f32> {
        match self.normalized() {
            Self::ScaleDown { min_font_size_sp } => Some(min_font_size_sp),
            _ => None,
        }
    }

    fn is_ellipsis(self) -> bool {
        matches!(
            self,
            Self::Ellipsis | Self::StartEllipsis | Self::MiddleEllipsis
        )
    }
}

impl PartialEq for TextOverflow {
    fn eq(&self, other: &Self) -> bool {
        match ((*self).normalized(), (*other).normalized()) {
            (Self::Clip, Self::Clip)
            | (Self::Ellipsis, Self::Ellipsis)
            | (Self::Visible, Self::Visible)
            | (Self::StartEllipsis, Self::StartEllipsis)
            | (Self::MiddleEllipsis, Self::MiddleEllipsis) => true,
            (
                Self::ScaleDown {
                    min_font_size_sp: left,
                },
                Self::ScaleDown {
                    min_font_size_sp: right,
                },
            ) => left.to_bits() == right.to_bits(),
            _ => false,
        }
    }
}

impl Eq for TextOverflow {}

impl Hash for TextOverflow {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match (*self).normalized() {
            Self::Clip => 0u8.hash(state),
            Self::Ellipsis => 1u8.hash(state),
            Self::Visible => 2u8.hash(state),
            Self::StartEllipsis => 3u8.hash(state),
            Self::MiddleEllipsis => 4u8.hash(state),
            Self::ScaleDown { min_font_size_sp } => {
                5u8.hash(state);
                min_font_size_sp.to_bits().hash(state);
            }
        }
    }
}

/// Text layout behavior options matching Compose `BasicText` controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextLayoutOptions {
    pub overflow: TextOverflow,
    pub soft_wrap: bool,
    pub max_lines: usize,
    pub min_lines: usize,
}

impl Default for TextLayoutOptions {
    fn default() -> Self {
        Self {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: usize::MAX,
            min_lines: 1,
        }
    }
}

impl TextLayoutOptions {
    pub fn normalized(self) -> Self {
        let min_lines = self.min_lines.max(1);
        // Compose's `finalMaxLines`: text that does not wrap cannot put an ellipsis on
        // every line, so it lays out only its first line.
        let max_lines = if !self.soft_wrap && self.overflow.is_ellipsis() {
            1
        } else {
            self.max_lines.max(min_lines)
        };
        Self {
            overflow: self.overflow.normalized(),
            soft_wrap: self.soft_wrap,
            max_lines,
            min_lines,
        }
    }
}

fn normalize_scale_down_min_font_size_sp(value: f32) -> f32 {
    if value.is_finite() && value >= MIN_SCALE_DOWN_FONT_SIZE_SP {
        value
    } else {
        MIN_SCALE_DOWN_FONT_SIZE_SP
    }
}

/// High-level text widget options for constrained UI text.
///
/// `None` for `max_lines` means no explicit line limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextOptions {
    pub overflow: TextOverflow,
    pub soft_wrap: bool,
    pub max_lines: Option<usize>,
    pub min_lines: usize,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            overflow: TextOverflow::Clip,
            soft_wrap: true,
            max_lines: None,
            min_lines: 1,
        }
    }
}

impl From<TextOptions> for TextLayoutOptions {
    fn from(options: TextOptions) -> Self {
        Self {
            overflow: options.overflow,
            soft_wrap: options.soft_wrap,
            max_lines: options.max_lines.unwrap_or(usize::MAX),
            min_lines: options.min_lines,
        }
        .normalized()
    }
}

impl From<TextLayoutOptions> for TextOptions {
    fn from(options: TextLayoutOptions) -> Self {
        let options = options.normalized();
        Self {
            overflow: options.overflow,
            soft_wrap: options.soft_wrap,
            max_lines: (options.max_lines != usize::MAX).then_some(options.max_lines),
            min_lines: options.min_lines,
        }
    }
}

#[cfg(test)]
#[path = "tests/layout_options_tests.rs"]
mod tests;
