//! Liquid theme: semantic colors, the iOS-style type ramp, and glass defaults,
//! provided to the subtree through composition locals — the analogue of
//! `MaterialTheme`.

#![allow(non_snake_case)]

use cranpose_core::{compositionLocalOf, CompositionLocal, CompositionLocalProvider};
use cranpose_macros::composable;
use cranpose_services::isSystemInDarkTheme;
use cranpose_ui::text::FontWeight;
use cranpose_ui::text::{SpanStyle, TextStyle, TextUnit};
use cranpose_ui_graphics::Color;
use std::cell::RefCell;

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
            background: Color::from_rgb_u8(242, 242, 247),
            surface: Color::WHITE,
            surface_pressed: Color::from_rgb_u8(226, 226, 231),
            accent,
            on_accent: Color::WHITE,
            destructive: Color::from_rgb_u8(255, 59, 48),
            success: Color::from_rgb_u8(52, 199, 89),
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
            background: Color::from_rgb_u8(10, 10, 12),
            surface: Color::from_rgb_u8(28, 28, 30),
            surface_pressed: Color::from_rgb_u8(44, 44, 46),
            accent,
            on_accent: Color::WHITE,
            destructive: Color::from_rgb_u8(255, 69, 58),
            success: Color::from_rgb_u8(48, 209, 88),
            warning: Color::from_rgb_u8(255, 159, 10),
            glass_tint: Color::from_rgba_u8(20, 20, 24, 40),
            glass_stroke: Color::from_rgba_u8(255, 255, 255, 46),
        }
    }
}

/// The iOS text-style ramp as ready-to-use [`TextStyle`]s (colors come from
/// [`LiquidColors::label`] by default at the call site).
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
}

impl Default for LiquidThemeSpec {
    fn default() -> Self {
        Self {
            scheme: SchemeMode::Auto,
            accent: Color::from_rgb_u8(0, 122, 255),
            typography: LiquidTypography::default(),
        }
    }
}

fn local_liquid_colors() -> CompositionLocal<LiquidColors> {
    thread_local! {
        static LOCAL: RefCell<Option<CompositionLocal<LiquidColors>>> = const { RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| {
                compositionLocalOf(|| LiquidColors::light(LiquidThemeSpec::default().accent))
            })
            .clone()
    })
}

fn local_liquid_typography() -> CompositionLocal<LiquidTypography> {
    thread_local! {
        static LOCAL: RefCell<Option<CompositionLocal<LiquidTypography>>> = const { RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(LiquidTypography::default))
            .clone()
    })
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
    let dark = match spec.scheme {
        SchemeMode::Auto => isSystemInDarkTheme(),
        SchemeMode::Light => false,
        SchemeMode::Dark => true,
    };
    let colors = if dark {
        LiquidColors::dark(spec.accent)
    } else {
        LiquidColors::light(spec.accent)
    };
    CompositionLocalProvider(
        vec![
            local_liquid_colors().provides(colors),
            local_liquid_typography().provides(spec.typography.clone()),
        ],
        move || {
            content();
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palettes_differ_and_share_accent() {
        let accent = Color::from_rgb_u8(0, 122, 255);
        let light = LiquidColors::light(accent);
        let dark = LiquidColors::dark(accent);
        assert!(!light.is_dark);
        assert!(dark.is_dark);
        assert_eq!(light.accent, dark.accent);
        assert_ne!(light.background, dark.background);
        assert_ne!(light.glass_tint, dark.glass_tint);
    }

    #[test]
    fn type_ramp_is_descending() {
        let t = LiquidTypography::default();
        let sizes = [
            &t.large_title,
            &t.title1,
            &t.title2,
            &t.title3,
            &t.body,
            &t.footnote,
            &t.caption2,
        ];
        let values: Vec<f32> = sizes
            .iter()
            .map(|style| style.span_style.font_size.value())
            .collect();
        assert!(values.windows(2).all(|pair| pair[0] >= pair[1]));
    }
}
