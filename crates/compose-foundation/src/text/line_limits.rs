//! Line limit configuration for text fields.
//!
//! This module provides `TextFieldLineLimits` which controls whether a text field
//! is single-line (horizontal scroll, no newlines) or multi-line with optional
//! min/max line constraints.
//!
//! Matches Jetpack Compose's `TextFieldLineLimits` from
//! `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/input/TextFieldLineLimits.kt`.

/// Line limit configuration for text fields.
///
/// Controls whether a text field allows multiple lines of input and how many
/// lines are visible at minimum and maximum.
///
/// # SingleLine
///
/// When `SingleLine` is used:
/// - Newline characters (`\n`) are blocked from input
/// - Pasted text has newlines replaced with spaces
/// - The text field scrolls horizontally if content exceeds width
/// - The Enter key does NOT insert a newline (may trigger submit action)
///
/// # MultiLine
///
/// When `MultiLine` is used:
/// - Newline characters are allowed
/// - The text field scrolls vertically if content exceeds visible lines
/// - `min_lines` controls minimum visible height (default: 1)
/// - `max_lines` controls maximum visible height before scrolling (default: unlimited)
///
/// # Example
///
/// ```
/// use compose_foundation::text::TextFieldLineLimits;
///
/// // Single-line text field (like a search box)
/// let single = TextFieldLineLimits::SingleLine;
///
/// // Multi-line with default settings
/// let multi = TextFieldLineLimits::default();
///
/// // Multi-line with 3-5 visible lines
/// let constrained = TextFieldLineLimits::MultiLine { min_lines: 3, max_lines: 5 };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFieldLineLimits {
    /// Single line input - no newlines allowed, horizontal scrolling.
    SingleLine,
    /// Multi-line input with optional line constraints.
    ///
    /// - `min_lines`: Minimum number of visible lines (affects minimum height)
    /// - `max_lines`: Maximum number of visible lines before scrolling
    MultiLine {
        /// Minimum visible lines (default: 1)
        min_lines: usize,
        /// Maximum visible lines before scrolling (default: unlimited)
        max_lines: usize,
    },
}

impl TextFieldLineLimits {
    /// Default multi-line with no constraints (1 line minimum, unlimited maximum).
    pub const DEFAULT: Self = Self::MultiLine {
        min_lines: 1,
        max_lines: usize::MAX,
    };

    /// Returns true if this is single-line mode.
    #[inline]
    pub fn is_single_line(&self) -> bool {
        matches!(self, Self::SingleLine)
    }

    /// Returns true if this is multi-line mode.
    #[inline]
    pub fn is_multi_line(&self) -> bool {
        matches!(self, Self::MultiLine { .. })
    }

    /// Returns the minimum number of lines (1 for SingleLine).
    pub fn min_lines(&self) -> usize {
        match self {
            Self::SingleLine => 1,
            Self::MultiLine { min_lines, .. } => *min_lines,
        }
    }

    /// Returns the maximum number of lines (1 for SingleLine, configured value for MultiLine).
    pub fn max_lines(&self) -> usize {
        match self {
            Self::SingleLine => 1,
            Self::MultiLine { max_lines, .. } => *max_lines,
        }
    }
}

impl Default for TextFieldLineLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Filters text for single-line mode by replacing newlines with spaces.
///
/// This is used when:
/// - Pasting text into a SingleLine text field
/// - Programmatically setting text on a SingleLine field
///
/// # Example
///
/// ```
/// use compose_foundation::text::filter_for_single_line;
///
/// assert_eq!(filter_for_single_line("hello\nworld"), "hello world");
/// assert_eq!(filter_for_single_line("a\n\nb"), "a  b");
/// ```
pub fn filter_for_single_line(text: &str) -> String {
    text.replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_properties() {
        let limits = TextFieldLineLimits::SingleLine;
        assert!(limits.is_single_line());
        assert!(!limits.is_multi_line());
        assert_eq!(limits.min_lines(), 1);
        assert_eq!(limits.max_lines(), 1);
    }

    #[test]
    fn multi_line_default_properties() {
        let limits = TextFieldLineLimits::default();
        assert!(!limits.is_single_line());
        assert!(limits.is_multi_line());
        assert_eq!(limits.min_lines(), 1);
        assert_eq!(limits.max_lines(), usize::MAX);
    }

    #[test]
    fn multi_line_constrained_properties() {
        let limits = TextFieldLineLimits::MultiLine {
            min_lines: 3,
            max_lines: 10,
        };
        assert!(!limits.is_single_line());
        assert!(limits.is_multi_line());
        assert_eq!(limits.min_lines(), 3);
        assert_eq!(limits.max_lines(), 10);
    }

    #[test]
    fn filter_replaces_newlines() {
        assert_eq!(filter_for_single_line("hello\nworld"), "hello world");
        assert_eq!(filter_for_single_line("a\n\nb"), "a  b");
        assert_eq!(filter_for_single_line("no newlines"), "no newlines");
        assert_eq!(filter_for_single_line("\n\n\n"), "   ");
    }
}
