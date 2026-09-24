//! Liquid theme: semantic colors, the iOS-style type ramp, and glass defaults,
//! provided to the subtree through composition locals — the analogue of
//! `MaterialTheme`.

use std::cell::RefCell;

use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOf};
use cranpose_macros::composable;
use cranpose_services::isSystemInDarkTheme;
use cranpose_ui::text::{FontWeight, SpanStyle, TextStyle, TextUnit};
use cranpose_ui_graphics::Color;

use crate::appearance::GlassTintAmount;

/// Whether the theme follows the OS appearance or is pinned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SchemeMode {
    /// Follow [`cranpose_services::isSystemInDarkTheme`] live.
    #[default]
    Auto,
    Light,
    Dark,
}

/// Semantic color palette mirroring the iOS system palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiquidColors {
    /// True when this is the dark palette (drives glass exposure/tints).
    pub is_dark: bool,
    /// Primary text.
    pub label: Color,
    /// Secondary text (subtitles, captions).
    pub secondary_label: Color,
    /// Tertiary text (placeholders, disabled).
    pub tertiary_label: Color,
    /// Hairline separators.
    pub separator: Color,
    /// Filled control track (chips, search fields).
    pub fill: Color,
    /// Lighter fill for nested controls.
    pub secondary_fill: Color,
    /// Resting tint of a glass surface pane (cards, planks): the surface
    /// role at pane translucency, so the backdrop transmits through it.
    pub surface_glass: Color,
    /// Window background (grouped style).
    pub background: Color,
    /// Elevated surface (cards, list sections).
    pub surface: Color,
    /// Pressed surface wash.
    pub surface_pressed: Color,
    /// Accent (interactive) color.
    pub accent: Color,
    /// Content on accent fills.
    pub on_accent: Color,
    /// Destructive action color.
    pub destructive: Color,
    /// Success color.
    pub success: Color,
    /// Active switch track.
    pub toggle_on: Color,
    /// Inactive switch track.
    pub toggle_off: Color,
    /// Warning color.
    pub warning: Color,
    /// Tint mixed over glass materials.
    pub glass_tint: Color,
    /// Hairline stroke drawn around glass shapes.
    pub glass_stroke: Color,
}

impl LiquidColors {
    pub fn light(accent: Color) -> Self {
        Self {
            is_dark: false,
            label: Color::from_rgb_u8(17, 17, 20),
            secondary_label: Color::from_rgba_u8(60, 60, 67, 153),
            tertiary_label: Color::from_rgba_u8(60, 60, 67, 76),
            separator: Color::from_rgba_u8(60, 60, 67, 56),
            fill: Color::from_rgba_u8(120, 120, 128, 40),
            secondary_fill: Color::from_rgba_u8(120, 120, 128, 28),
            surface_glass: Color::from_rgba_u8(255, 255, 255, 191),
            background: Color::from_rgb_u8(242, 242, 247),
            surface: Color::WHITE,
            surface_pressed: Color::from_rgb_u8(226, 226, 231),
            accent,
            on_accent: Color::WHITE,
            destructive: Color::from_rgb_u8(255, 59, 48),
            success: Color::from_rgb_u8(52, 199, 89),
            toggle_on: accent,
            toggle_off: Color::from_rgb_u8(170, 170, 181),
            warning: Color::from_rgb_u8(255, 149, 0),
            glass_tint: Color::from_rgba_u8(255, 255, 255, 18),
            glass_stroke: Color::from_rgba_u8(255, 255, 255, 120),
        }
    }

    pub fn dark(accent: Color) -> Self {
        Self {
            is_dark: true,
            label: Color::from_rgb_u8(242, 242, 247),
            secondary_label: Color::from_rgba_u8(235, 235, 245, 153),
            tertiary_label: Color::from_rgba_u8(235, 235, 245, 76),
            separator: Color::from_rgba_u8(84, 84, 88, 130),
            fill: Color::from_rgba_u8(120, 120, 128, 70),
            secondary_fill: Color::from_rgba_u8(120, 120, 128, 50),
            surface_glass: Color::from_rgba_u8(28, 28, 30, 184),
            background: Color::from_rgb_u8(10, 10, 12),
            surface: Color::from_rgb_u8(28, 28, 30),
            surface_pressed: Color::from_rgb_u8(44, 44, 46),
            accent,
            on_accent: Color::WHITE,
            destructive: Color::from_rgb_u8(255, 69, 58),
            success: Color::from_rgb_u8(48, 209, 88),
            toggle_on: accent,
            toggle_off: Color::from_rgb_u8(99, 99, 102),
            warning: Color::from_rgb_u8(255, 159, 10),
            glass_tint: Color::from_rgba_u8(20, 20, 24, 40),
            glass_stroke: Color::from_rgba_u8(255, 255, 255, 46),
        }
    }
}

/// The iOS text-style ramp as ready-to-use [`TextStyle`]s (colors come from
/// [`LiquidColors::label`] by default at the call site).
impl LiquidColors {
    /// The palette for a person who asked the system for more contrast: the
    /// secondary and tertiary labels, the separator, the fills and the glass
    /// edge move toward the label color, and the off toggle darkens.
    pub fn with_more_contrast(mut self) -> Self {
        self.secondary_label = self.secondary_label.with_alpha(0.9);
        self.tertiary_label = self.tertiary_label.with_alpha(0.7);
        self.separator = self.separator.with_alpha(0.6);
        self.fill = self.fill.with_alpha(0.32);
        self.secondary_fill = self.secondary_fill.with_alpha(0.24);
        self.glass_stroke = self.glass_stroke.with_alpha(0.9);
        self.toggle_off = if self.is_dark {
            Color::from_rgb_u8(142, 142, 147)
        } else {
            Color::from_rgb_u8(99, 99, 108)
        };
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LiquidTypography {
    pub large_title: TextStyle,
    pub title1: TextStyle,
    pub title2: TextStyle,
    pub title3: TextStyle,
    pub headline: TextStyle,
    pub body: TextStyle,
    pub callout: TextStyle,
    pub subheadline: TextStyle,
    pub footnote: TextStyle,
    pub caption1: TextStyle,
    pub caption2: TextStyle,
}

fn ramp_style(size_sp: f32, weight: FontWeight) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(size_sp),
            font_weight: Some(weight),
            ..Default::default()
        },
        ..Default::default()
    }
}

impl LiquidTypography {
    /// The ramp for a person who asked the system for bold text: every style
    /// gains two hundred of weight, so body text reads semi bold and titles
    /// extra bold.
    pub fn bolder(self) -> Self {
        Self {
            large_title: bolder(self.large_title),
            title1: bolder(self.title1),
            title2: bolder(self.title2),
            title3: bolder(self.title3),
            headline: bolder(self.headline),
            body: bolder(self.body),
            callout: bolder(self.callout),
            subheadline: bolder(self.subheadline),
            footnote: bolder(self.footnote),
            caption1: bolder(self.caption1),
            caption2: bolder(self.caption2),
        }
    }
}

fn bolder(mut style: TextStyle) -> TextStyle {
    let weight = style.span_style.font_weight.unwrap_or(FontWeight::NORMAL);
    style.span_style.font_weight = Some(FontWeight((weight.0 + 200).min(900)));
    style
}

impl Default for LiquidTypography {
    fn default() -> Self {
        Self {
            large_title: ramp_style(34.0, FontWeight::BOLD),
            title1: ramp_style(28.0, FontWeight::BOLD),
            title2: ramp_style(22.0, FontWeight::BOLD),
            title3: ramp_style(20.0, FontWeight::SEMI_BOLD),
            headline: ramp_style(17.0, FontWeight::SEMI_BOLD),
            body: ramp_style(17.0, FontWeight::NORMAL),
            callout: ramp_style(16.0, FontWeight::NORMAL),
            subheadline: ramp_style(15.0, FontWeight::NORMAL),
            footnote: ramp_style(13.0, FontWeight::NORMAL),
            caption1: ramp_style(12.0, FontWeight::NORMAL),
            caption2: ramp_style(11.0, FontWeight::NORMAL),
        }
    }
}

/// Theme configuration passed to [`LiquidTheme`].
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidThemeSpec {
    pub scheme: SchemeMode,
    /// Accent color (iOS system blue by default).
    pub accent: Color,
    pub typography: LiquidTypography,
    /// Tint amount used by the floating navigation surfaces.
    pub glass_tint_amount: GlassTintAmount,
}

impl Default for LiquidThemeSpec {
    fn default() -> Self {
        Self {
            scheme: SchemeMode::Auto,
            accent: Color::from_rgb_u8(0, 122, 255),
            typography: LiquidTypography::default(),
            glass_tint_amount: GlassTintAmount::default(),
        }
    }
}

fn theme_local<T: Clone + PartialEq + 'static>(
    cell: &RefCell<Option<CompositionLocal<T>>>,
    default: fn() -> T,
) -> CompositionLocal<T> {
    cell.borrow_mut()
        .get_or_insert_with(|| compositionLocalOf(default))
        .clone()
}

fn local_liquid_colors() -> CompositionLocal<LiquidColors> {
    thread_local! {
        static LOCAL: RefCell<Option<CompositionLocal<LiquidColors>>> = const { RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        theme_local(cell, || {
            LiquidColors::light(LiquidThemeSpec::default().accent)
        })
    })
}

fn local_liquid_typography() -> CompositionLocal<LiquidTypography> {
    thread_local! {
        static LOCAL: RefCell<Option<CompositionLocal<LiquidTypography>>> = const { RefCell::new(None) };
    }
    LOCAL.with(|cell| theme_local(cell, LiquidTypography::default))
}

fn local_liquid_glass_tint_amount() -> CompositionLocal<GlassTintAmount> {
    thread_local! {
        static LOCAL: RefCell<Option<CompositionLocal<GlassTintAmount>>> = const { RefCell::new(None) };
    }
    LOCAL.with(|cell| theme_local(cell, GlassTintAmount::default))
}

/// The active navigation surface tint amount, defaulting to 25 percent.
#[composable]
pub fn liquid_glass_tint_amount() -> GlassTintAmount {
    local_liquid_glass_tint_amount().current()
}

/// The active semantic palette (light defaults outside a [`LiquidTheme`]).
#[composable]
pub fn liquid_colors() -> LiquidColors {
    local_liquid_colors().current()
}

/// The active type ramp.
#[composable]
pub fn liquid_typography() -> LiquidTypography {
    local_liquid_typography().current()
}

/// Provides the Liquid design system (colors, typography) to `content`.
///
/// `SchemeMode::Auto` follows the OS light/dark appearance live.
#[composable]
pub fn LiquidTheme(spec: LiquidThemeSpec, content: impl FnOnce()) {
    let options = cranpose_services::local_accessibility_options().current();
    let dark = match spec.scheme {
        SchemeMode::Auto => isSystemInDarkTheme(),
        SchemeMode::Light => false,
        SchemeMode::Dark => true,
    } != options.invert_colors;
    let colors = if dark {
        LiquidColors::dark(spec.accent)
    } else {
        LiquidColors::light(spec.accent)
    };
    let colors = if options.increase_contrast {
        colors.with_more_contrast()
    } else {
        colors
    };
    let typography = if options.bold_text {
        spec.typography.clone().bolder()
    } else {
        spec.typography.clone()
    };
    CompositionLocalProvider(
        vec![
            local_liquid_colors().provides(colors),
            local_liquid_typography().provides(typography),
            local_liquid_glass_tint_amount().provides(spec.glass_tint_amount),
        ],
        move || {
            content();
        },
    );
}

#[cfg(test)]
#[path = "tests/theme_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/theme_option_tests.rs"]
mod option_tests;
