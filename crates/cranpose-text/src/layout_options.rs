use std::hash::{Hash, Hasher};

const MIN_SCALE_DOWN_FONT_SIZE_SP: f32 = 1.0;

/// How overflowing text should be handled.
#[derive(Clone, Copy, Debug, Default)]
pub enum TextOverflow {
    /// Text that does not fit is cut off at the bounds of the text.
    #[default]
    Clip,
    /// The end of the last visible line is replaced with an ellipsis.
    Ellipsis,
    /// Text is laid out in full and drawn beyond its bounds.
    Visible,
    /// The start of the line is replaced with an ellipsis. Takes effect only when `max_lines`
    /// is 1; otherwise no ellipsis is added.
    StartEllipsis,
    /// The middle of the line is replaced with an ellipsis. Takes effect only when `max_lines`
    /// is 1; otherwise no ellipsis is added.
    MiddleEllipsis,
    /// The font size shrinks until the text fits, down to `min_font_size_sp`. Text that
    /// still overflows at that size is clipped.
    ScaleDown {
        /// Smallest font size to shrink to, in sp. Values below 1.0 and non-finite values count
        /// as 1.0.
        min_font_size_sp: f32,
    },
}

impl TextOverflow {
    /// Returns the overflow with the `ScaleDown` minimum size raised to at least 1.0 sp.
    pub fn normalized(self) -> Self {
        match self {
            Self::ScaleDown { min_font_size_sp } => Self::ScaleDown {
                min_font_size_sp: normalize_scale_down_min_font_size_sp(min_font_size_sp),
            },
            other => other,
        }
    }

    /// Returns the minimum font size of `ScaleDown` in sp after normalizing, and `None` for
    /// the other kinds.
    pub fn scale_down_min_font_size_sp(self) -> Option<f32> {
        match self.normalized() {
            Self::ScaleDown { min_font_size_sp } => Some(min_font_size_sp),
            _ => None,
        }
    }

    pub(crate) fn is_ellipsis(self) -> bool {
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
    /// How text that does not fit is handled.
    pub overflow: TextOverflow,
    /// Whether lines wrap at the available width. When false, only explicit newlines break
    /// lines.
    pub soft_wrap: bool,
    /// Largest number of lines to lay out. `usize::MAX` means no limit.
    pub max_lines: usize,
    /// Number of lines the text's height covers at least, even when it has fewer lines.
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
    /// Returns these options with `min_lines` at least 1 and `max_lines` at
    /// least `min_lines`.
    ///
    /// Ellipsized text that does not soft wrap lays out one line, as Compose's
    /// `finalMaxLines` does, since each unwrapped line cannot end in its own
    /// ellipsis. `min_lines` still sets its height.
    pub fn normalized(self) -> Self {
        let min_lines = self.min_lines.max(1);
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
    /// How text that does not fit is handled.
    pub overflow: TextOverflow,
    /// Whether lines wrap at the available width. When false, only explicit newlines break
    /// lines.
    pub soft_wrap: bool,
    /// Largest number of lines to lay out. `None` means no limit.
    pub max_lines: Option<usize>,
    /// Number of lines the text's height covers at least, even when it has fewer lines.
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
