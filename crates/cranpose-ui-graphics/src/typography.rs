//! Typography data structures (font styles, weights, text styles)
//!
//! These are the *drawing-side* text types: the smallest description of a run
//! of text that [`crate::DrawScope`] can hand to a renderer. The full typography
//! model (annotated strings, span/paragraph styles, decorations, hyphenation)
//! lives in `cranpose-ui`, which is above this crate in the dependency graph —
//! `cranpose-ui` maps a [`TextStyle`] onto that richer model, and both
//! measurement and rasterization go through that one mapping.

use crate::geometry::Size;

/// Font style (normal, italic, oblique)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    /// Rendered as [`FontStyle::Italic`]; no font in the stack ships a separate
    /// oblique face.
    Oblique,
}

/// Font weight (100-900)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontWeight(pub u16);

impl FontWeight {
    pub const THIN: FontWeight = FontWeight(100);
    pub const EXTRA_LIGHT: FontWeight = FontWeight(200);
    pub const LIGHT: FontWeight = FontWeight(300);
    pub const NORMAL: FontWeight = FontWeight(400);
    pub const MEDIUM: FontWeight = FontWeight(500);
    pub const SEMI_BOLD: FontWeight = FontWeight(600);
    pub const BOLD: FontWeight = FontWeight(700);
    pub const EXTRA_BOLD: FontWeight = FontWeight(800);
    pub const BLACK: FontWeight = FontWeight(900);

    /// Clamps to the `1..=1000` range every font backend accepts.
    pub const fn new(weight: u16) -> Self {
        if weight < 1 {
            Self(1)
        } else if weight > 1000 {
            Self(1000)
        } else {
            Self(weight)
        }
    }

    pub const fn value(self) -> u16 {
        self.0
    }
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::NORMAL
    }
}

/// Horizontal placement of the text block inside the box it is drawn in.
///
/// This aligns the *block*, not the individual lines: every line of a
/// multi-line string starts at the block's left edge, matching how the
/// framework's `Text` composable is laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Vertical placement of the text block inside the box it is drawn in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextVerticalAlign {
    #[default]
    Top,
    Center,
    Bottom,
    /// The box's **top edge** is the first line's baseline. Use this when a
    /// layout is specified in baselines rather than boxes; the text then
    /// extends above the edge by
    /// [`TextMeasurement::first_baseline`](crate::TextMeasurement::first_baseline).
    Baseline,
}

/// Where the leading — the difference between a line's box and the font's own
/// ascent-plus-descent extent — is spent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LineHeightAlignment {
    /// All of it below the glyphs.
    Top,
    /// Split evenly, with the odd whole pixel below the baseline.
    Center,
    #[default]
    /// Split in the font's own ascent-to-descent ratio.
    Proportional,
    /// All of it above the glyphs.
    Bottom,
}

/// Which edges of a text block give their leading back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LineHeightTrim {
    FirstLineTop,
    LastLineBottom,
    #[default]
    Both,
    None,
}

/// What a requested line height means when the font does not fit inside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LineHeightMode {
    /// The request wins and the glyphs overflow their box.
    #[default]
    Fixed,
    /// The font's own extent is a floor. This is what Android does.
    Minimum,
    /// The font's extent, whatever was requested.
    Tight,
}

/// How a line of text sits inside the height it was given.
///
/// A style that names one is laid out by AOSP's `StaticLayout` rule — whole-pixel
/// metrics, a font that a short line height cannot shrink, and the odd pixel of
/// leading below the baseline. A style that names none keeps the framework's
/// plain arithmetic: the box is exactly the requested height with the leading
/// split evenly. The two disagree by a device pixel on most faces, so a screen
/// that draws through a [`crate::DrawScope`] and composes `Text` in the same
/// frame has to state the same policy on both or the two sets of rows will not
/// line up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LineHeightStyle {
    pub alignment: LineHeightAlignment,
    pub trim: LineHeightTrim,
    pub mode: LineHeightMode,
}

impl Default for LineHeightStyle {
    fn default() -> Self {
        Self {
            alignment: LineHeightAlignment::Proportional,
            trim: LineHeightTrim::Both,
            mode: LineHeightMode::Fixed,
        }
    }
}

/// Everything [`crate::DrawScope`] needs to measure and draw a run of text.
///
/// A style is a plain value: sizes are already in scope (logical) units, and
/// every field is resolved — there is no inheritance or theme lookup at draw
/// time. Build one once and reuse it; measurement is cached on the
/// `(text, style)` pair, so a style rebuilt with identical values still hits
/// the cache.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    /// Family name to resolve against the fonts the app registered. `None`
    /// asks for the framework's default family.
    ///
    /// Only *named* families resolve: file-backed families are loaded by the
    /// app at startup and looked up by the name in their font tables.
    pub font_family: Option<String>,
    pub font_size: f32,
    pub font_weight: FontWeight,
    pub font_style: FontStyle,
    /// Extra advance inserted between characters, in scope units.
    pub letter_spacing: f32,
    /// Distance between consecutive baselines. `None` uses the font's natural
    /// line height.
    pub line_height: Option<f32>,
    /// How the line sits inside that height. `None` takes the framework's plain
    /// split; naming one asks for the platform rule, and is what makes a drawn
    /// run land on the same rows as a composed `Text` of the same style.
    pub line_height_style: Option<LineHeightStyle>,
    pub align: TextAlign,
    pub vertical_align: TextVerticalAlign,
}

impl TextStyle {
    /// The size used when a style carries a non-positive or non-finite one.
    /// Matches the framework-wide text default.
    pub const DEFAULT_FONT_SIZE: f32 = 14.0;

    pub fn new(font_size: f32) -> Self {
        Self {
            font_size,
            ..Self::default()
        }
    }

    pub fn with_font_family(mut self, family: impl Into<String>) -> Self {
        let family = family.into();
        self.font_family = (!family.is_empty()).then_some(family);
        self
    }

    pub fn with_font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }

    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.font_weight = weight;
        self
    }

    pub fn with_style(mut self, style: FontStyle) -> Self {
        self.font_style = style;
        self
    }

    pub fn with_letter_spacing(mut self, letter_spacing: f32) -> Self {
        self.letter_spacing = letter_spacing;
        self
    }

    pub fn with_line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height.is_finite().then_some(line_height);
        self
    }

    /// Asks for a line-height policy, which is what makes this run resolve its
    /// line box by the same rule a `Text` composable of the same style does.
    pub fn with_line_height_style(mut self, line_height_style: LineHeightStyle) -> Self {
        self.line_height_style = Some(line_height_style);
        self
    }

    pub fn with_align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    pub fn with_vertical_align(mut self, vertical_align: TextVerticalAlign) -> Self {
        self.vertical_align = vertical_align;
        self
    }

    /// The font size a measurer/rasterizer will actually use. Non-finite and
    /// non-positive sizes fall back to [`TextStyle::DEFAULT_FONT_SIZE`] instead
    /// of producing NaN geometry.
    pub fn resolved_font_size(&self) -> f32 {
        if self.font_size.is_finite() && self.font_size > 0.0 {
            self.font_size
        } else {
            Self::DEFAULT_FONT_SIZE
        }
    }

    /// The letter spacing a measurer will actually use.
    pub fn resolved_letter_spacing(&self) -> f32 {
        if self.letter_spacing.is_finite() {
            self.letter_spacing
        } else {
            0.0
        }
    }

    /// The line height a measurer will actually use, given the font's natural
    /// one. `natural` is only consulted when the style leaves it unset.
    pub fn resolved_line_height(&self, natural: f32) -> f32 {
        match self.line_height {
            Some(height) if height.is_finite() && height > 0.0 => height,
            _ => natural,
        }
    }
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: Self::DEFAULT_FONT_SIZE,
            font_weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            letter_spacing: 0.0,
            line_height: None,
            line_height_style: None,
            align: TextAlign::Left,
            vertical_align: TextVerticalAlign::Top,
        }
    }
}

/// What a string occupies once laid out — the answer
/// [`DrawScope::measure_text`](crate::DrawScope::measure_text) gives, and
/// exactly the box `draw_text` fills.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextMeasurement {
    /// Tight block size: the widest line by its total advance, and
    /// `line_count * line_height` tall.
    pub size: Size,
    /// Baseline-to-baseline distance.
    pub line_height: f32,
    /// Distance from the top of the block down to the first line's baseline.
    /// Subtract it from a baseline y to get the top-left a `_at` draw wants.
    pub first_baseline: f32,
    /// Number of laid-out lines. `1` for an empty string.
    pub line_count: usize,
}

impl TextMeasurement {
    /// The measurement of an empty string: no extent, but still one line's
    /// worth of vertical metrics so callers can lay out an empty label.
    pub fn empty(line_height: f32, first_baseline: f32) -> Self {
        Self {
            size: Size::ZERO,
            line_height,
            first_baseline,
            line_count: 1,
        }
    }
}

/// Font-backed measurement, injected into a [`crate::DrawScopeDefault`] by the
/// UI layer.
///
/// This crate holds no fonts, so a draw scope cannot measure text on its own.
/// `cranpose-ui` installs an implementation that forwards to the very text
/// stack the `Text` composable uses, which is what keeps
/// [`DrawScope::measure_text`](crate::DrawScope::measure_text) and the glyphs
/// the renderer rasterizes in agreement. Without one installed a scope falls
/// back to [`estimate_text_measurement`], which is good enough for layout
/// smoke tests and wrong for anything that has to line up with real glyphs.
pub trait DrawTextMeasurer {
    fn measure_text(&self, text: &str, style: &TextStyle) -> TextMeasurement;
}

/// Font-free estimate used when no [`DrawTextMeasurer`] is installed.
///
/// Assumes a 0.6 em advance per character and the 0.8/-0.2 em ascent/descent
/// split typical of a UI sans face, so the shape of the result (and the
/// baseline formula) matches what a real font measurer returns even though the
/// numbers do not.
pub fn estimate_text_measurement(text: &str, style: &TextStyle) -> TextMeasurement {
    const CHAR_WIDTH_RATIO: f32 = 0.6;
    const ASCENT_RATIO: f32 = 0.8;
    const NATURAL_LINE_HEIGHT_RATIO: f32 = 1.0;

    let font_size = style.resolved_font_size();
    let letter_spacing = style.resolved_letter_spacing().max(0.0);
    let natural_line_height = font_size * NATURAL_LINE_HEIGHT_RATIO;
    let line_height = style.resolved_line_height(font_size * 1.4);
    let first_baseline = font_size * ASCENT_RATIO + (line_height - natural_line_height) * 0.5;

    if text.is_empty() {
        return TextMeasurement::empty(line_height, first_baseline);
    }

    let mut line_count = 0usize;
    let mut width = 0.0f32;
    for line in text.split('\n') {
        line_count += 1;
        let chars = line.chars().count();
        // One letter space per character, not per gap: Android's Minikin puts
        // half a letter space on each side of every cluster, so a run of `n`
        // characters carries `n` of them. See `run_tracking` in
        // `cranpose-render-common`'s `software_text_raster`.
        let advance = chars as f32 * (font_size * CHAR_WIDTH_RATIO + letter_spacing);
        width = width.max(advance);
    }

    TextMeasurement {
        size: Size::new(width, line_count as f32 * line_height),
        line_height,
        first_baseline,
        line_count,
    }
}

#[cfg(test)]
mod tests {

    /// A measurer must never be handed a spacing it cannot use. A style built
    /// from an unset or arithmetic-error value resolves to no spacing rather
    /// than laying every glyph out at NaN, which renders as nothing at all.
    #[test]
    fn letter_spacing_resolves_to_something_a_measurer_can_use() {
        let style = TextStyle::default().with_font_size(18.0);
        assert_eq!(style.font_size, 18.0);
        assert_eq!(style.resolved_font_size(), 18.0);
        assert_eq!(style.resolved_letter_spacing(), 0.0);

        assert_eq!(
            style
                .clone()
                .with_letter_spacing(1.5)
                .resolved_letter_spacing(),
            1.5
        );
        assert_eq!(
            style
                .clone()
                .with_letter_spacing(-0.5)
                .resolved_letter_spacing(),
            -0.5,
            "tightening is a real request, not an error"
        );
        for broken in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                style
                    .clone()
                    .with_letter_spacing(broken)
                    .resolved_letter_spacing(),
                0.0
            );
        }

        // And a font size that is not a size falls back rather than producing
        // NaN geometry for every glyph.
        for broken in [0.0, -12.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                TextStyle::default()
                    .with_font_size(broken)
                    .resolved_font_size(),
                TextStyle::DEFAULT_FONT_SIZE
            );
        }
    }
    use super::*;

    #[test]
    fn text_style_resolves_degenerate_sizes_to_the_framework_default() {
        for size in [0.0, -12.0, f32::NAN, f32::INFINITY] {
            assert_eq!(
                TextStyle::new(size).resolved_font_size(),
                TextStyle::DEFAULT_FONT_SIZE,
                "font size {size} must not reach a font backend"
            );
        }
        assert_eq!(TextStyle::new(19.0).resolved_font_size(), 19.0);
    }

    #[test]
    fn text_style_line_height_falls_back_to_the_natural_one() {
        let style = TextStyle::new(20.0);
        assert_eq!(style.resolved_line_height(28.0), 28.0);
        assert_eq!(
            style
                .clone()
                .with_line_height(40.0)
                .resolved_line_height(28.0),
            40.0
        );
        // A non-finite request is refused at the builder, not silently kept.
        assert_eq!(
            style.with_line_height(f32::NAN).resolved_line_height(28.0),
            28.0
        );
    }

    #[test]
    fn empty_font_family_name_means_the_default_family() {
        assert_eq!(TextStyle::new(14.0).with_font_family("").font_family, None);
        assert_eq!(
            TextStyle::new(14.0)
                .with_font_family("Fira Sans")
                .font_family,
            Some("Fira Sans".to_string())
        );
    }

    #[test]
    fn estimated_measurement_grows_with_the_longest_line() {
        let style = TextStyle::new(10.0);
        let one = estimate_text_measurement("AAAA", &style);
        let two = estimate_text_measurement("AAAA\nAAAAAAAA", &style);
        assert_eq!(one.line_count, 1);
        assert_eq!(two.line_count, 2);
        assert!(two.size.width > one.size.width);
        assert!((two.size.height - one.size.height * 2.0).abs() < 1e-3);
    }

    #[test]
    fn estimated_measurement_of_an_empty_string_keeps_one_line_of_metrics() {
        let measurement = estimate_text_measurement("", &TextStyle::new(16.0));
        assert_eq!(measurement.size, Size::ZERO);
        assert_eq!(measurement.line_count, 1);
        assert!(measurement.line_height > 0.0);
        assert!(measurement.first_baseline > 0.0);
    }

    #[test]
    fn estimated_baseline_sits_inside_the_line_slot() {
        let measurement = estimate_text_measurement("Ag", &TextStyle::new(24.0));
        assert!(measurement.first_baseline > 0.0);
        assert!(measurement.first_baseline < measurement.line_height);
    }
}
