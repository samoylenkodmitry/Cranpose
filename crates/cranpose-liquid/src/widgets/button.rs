//! Glass buttons: capsule glass with a spring press (scale + specular boost)
//! and haptic feedback.

use crate::material::{Glass, GlassDynamics, LiquidModifierExt, LiquidShape};
use crate::motion::liquid_press_scale;
use crate::theme::{liquid_colors, liquid_typography};
use cranpose_macros::composable;
use cranpose_services::{default_haptics, HapticFeedback};
use cranpose_ui::rememberMutableInteractionSource;
use cranpose_ui::text::TextStyle;
use cranpose_ui::widgets::{Box, BoxSpec, Text};
use cranpose_ui::{Modifier, Size};
use cranpose_ui_graphics::{Color, GraphicsLayer};
use cranpose_ui_layout::Alignment;
use std::cell::RefCell;
use std::rc::Rc;

const ICON_BACKPLATE_DIAMETER_RATIO: f32 = 0.50;
const ICON_BACKPLATE_GLYPH_RATIO: f32 = 0.28;

/// Visual style of a [`GlassButton`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GlassButtonStyle {
    /// Translucent glass with the label in the accent color.
    #[default]
    Glass,
    /// Accent-tinted glass with white content — the primary action.
    Prominent,
    /// No material: just the label with press feedback (toolbar/text button).
    Plain,
    /// Destructive variant of `Plain`.
    Destructive,
}

/// Configuration for [`GlassButton`] / [`GlassIconButton`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GlassButtonSpec {
    pub style: GlassButtonStyle,
    /// Overrides the material (advanced).
    pub glass: Option<Glass>,
    /// Overrides the label/icon color (defaults per style).
    pub content_color: Option<Color>,
    /// Optional inner color disc for icon buttons. The outer material remains
    /// clear glass; the disc colors only the icon's compact foreground core.
    pub icon_backplate: Option<Color>,
}

impl GlassButtonSpec {
    pub fn glass() -> Self {
        Self::default()
    }

    pub fn prominent() -> Self {
        Self {
            style: GlassButtonStyle::Prominent,
            ..Self::default()
        }
    }

    pub fn plain() -> Self {
        Self {
            style: GlassButtonStyle::Plain,
            ..Self::default()
        }
    }

    pub fn destructive() -> Self {
        Self {
            style: GlassButtonStyle::Destructive,
            ..Self::default()
        }
    }

    pub fn with_glass(mut self, glass: Glass) -> Self {
        self.glass = Some(glass);
        self
    }

    pub fn with_content_color(mut self, color: Color) -> Self {
        self.content_color = Some(color);
        self
    }

    pub fn with_icon_backplate(mut self, color: Color) -> Self {
        self.icon_backplate = Some(color);
        self
    }

    /// The label color for this style under `colors`.
    pub fn content_color(&self, colors: &crate::theme::LiquidColors) -> Color {
        if let Some(color) = self.content_color {
            return color;
        }
        match self.style {
            GlassButtonStyle::Glass => colors.accent,
            GlassButtonStyle::Prominent => colors.on_accent,
            GlassButtonStyle::Plain => colors.accent,
            GlassButtonStyle::Destructive => colors.destructive,
        }
    }

    /// The glyph color for icon buttons: the reference nav/search circles
    /// draw BLACK glyphs on plain glass (only prominent tints stay white).
    pub fn icon_color(&self, colors: &crate::theme::LiquidColors) -> Color {
        if let Some(color) = self.content_color {
            return color;
        }
        match self.style {
            GlassButtonStyle::Glass | GlassButtonStyle::Plain => colors.label,
            GlassButtonStyle::Prominent => colors.on_accent,
            GlassButtonStyle::Destructive => colors.destructive,
        }
    }

    fn resolve_material(
        &self,
        colors: &crate::theme::LiquidColors,
        foreground: Color,
    ) -> Option<Glass> {
        if let Some(glass) = &self.glass {
            return Some(if glass.foreground.is_some() {
                glass.clone()
            } else {
                glass
                    .clone()
                    .adaptive_frost(foreground, glass.adaptive_frost)
            });
        }
        match self.style {
            GlassButtonStyle::Glass => Some(Glass::regular().adaptive_frost(foreground, 0.65)),
            GlassButtonStyle::Prominent => Some(
                Glass::regular()
                    .tint(colors.accent.with_alpha(0.75))
                    .adaptive_frost(foreground, 0.65),
            ),
            GlassButtonStyle::Plain | GlassButtonStyle::Destructive => None,
        }
    }
}

/// A glass button. `content` composes the label (see [`GlassButton`] with
/// [`Text`], or an [`crate::icons::Icon`] + text row).
#[composable]
#[allow(non_snake_case)]
pub fn GlassButton(
    modifier: Modifier,
    spec: GlassButtonSpec,
    on_click: impl Fn() + 'static,
    content: impl FnMut() + 'static,
) {
    let colors = liquid_colors();
    let interaction = rememberMutableInteractionSource();
    let (pressed_modifier, pressed, content_alpha) =
        liquid_press_scale(Modifier::empty(), interaction.clone(), 1.08);

    let material = spec.resolve_material(&colors, spec.content_color(&colors));
    let mut base = pressed_modifier;
    if let Some(glass) = material {
        let pressed_for_glass = pressed;
        base = base.glass_effect_with(glass, move || GlassDynamics {
            highlight_boost: if pressed_for_glass.get() { 0.85 } else { 0.0 },
            ..Default::default()
        });
    }

    let on_click = Rc::new(RefCell::new(on_click));
    let base = base
        .press_interaction_source(interaction)
        .clickable(move |_point| {
            default_haptics().perform(HapticFeedback::ImpactLight);
            (on_click.borrow_mut())();
        })
        .padding_symmetric(16.0, 10.0);

    // Touched glass turns more transparent: the label ghosts while pressed.
    let content_layer = Modifier::empty().graphics_layer(move || GraphicsLayer {
        alpha: content_alpha.get().clamp(0.0, 1.0),
        ..Default::default()
    });
    let content = Rc::new(RefCell::new(content));
    Box(
        base.then(modifier),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            let content = Rc::clone(&content);
            Box(
                content_layer.clone(),
                BoxSpec::default().content_alignment(Alignment::CENTER),
                move || (content.borrow_mut())(),
            );
        },
    );
}

/// Convenience text label styled for the enclosing button.
#[composable]
#[allow(non_snake_case)]
pub fn GlassButtonLabel(text: impl Into<String>, spec: GlassButtonSpec) {
    let typography = liquid_typography();
    let color = spec.content_color(&liquid_colors());
    let style = TextStyle {
        span_style: cranpose_ui::text::SpanStyle {
            color: Some(color),
            ..typography.headline.span_style.clone()
        },
        ..typography.headline.clone()
    };
    Text(text.into(), Modifier::empty(), style);
}

#[composable]
#[allow(non_snake_case)]
pub(crate) fn GlassIconForeground(spec: GlassButtonSpec, diameter: f32, icon_path: &'static str) {
    let colors = liquid_colors();
    let icon_color = spec.icon_color(&colors);
    if let Some(backplate) = spec.icon_backplate {
        let backplate_diameter = diameter * ICON_BACKPLATE_DIAMETER_RATIO;
        Box(
            Modifier::empty()
                .size(Size::new(backplate_diameter, backplate_diameter))
                .draw_behind(move |scope| {
                    scope.draw_circle(
                        cranpose_ui_graphics::Brush::solid(backplate),
                        cranpose_ui_graphics::Point::new(
                            backplate_diameter * 0.5,
                            backplate_diameter * 0.5,
                        ),
                        backplate_diameter * 0.5,
                    );
                }),
            BoxSpec::default().content_alignment(Alignment::CENTER),
            move || {
                crate::icons::Icon(icon_path, diameter * ICON_BACKPLATE_GLYPH_RATIO, icon_color);
            },
        );
    } else {
        crate::icons::Icon(icon_path, diameter * 0.5, icon_color);
    }
}

/// A circular glass icon button (44dp target).
#[composable]
#[allow(non_snake_case)]
pub fn GlassIconButton(
    modifier: Modifier,
    spec: GlassButtonSpec,
    diameter: f32,
    on_click: impl Fn() + 'static,
    icon_path: &'static str,
) {
    GlassIconButtonWithForegroundAlpha(modifier, spec, diameter, 1.0, on_click, icon_path);
}

#[composable]
#[allow(non_snake_case)]
pub(crate) fn GlassIconButtonWithForegroundAlpha(
    modifier: Modifier,
    spec: GlassButtonSpec,
    diameter: f32,
    foreground_alpha: f32,
    on_click: impl Fn() + 'static,
    icon_path: &'static str,
) {
    let colors = liquid_colors();
    let interaction = rememberMutableInteractionSource();
    let (pressed_modifier, pressed, content_alpha) =
        liquid_press_scale(Modifier::empty(), interaction.clone(), 1.12);

    let material = spec
        .resolve_material(&colors, spec.icon_color(&colors))
        .map(|glass| glass.shape(LiquidShape::Circle));
    let mut base = pressed_modifier;
    if let Some(glass) = material {
        let pressed_for_glass = pressed;
        base = base.glass_effect_with(glass, move || GlassDynamics {
            highlight_boost: if pressed_for_glass.get() { 0.85 } else { 0.0 },
            ..Default::default()
        });
    }

    let on_click = Rc::new(RefCell::new(on_click));
    let base = base
        .press_interaction_source(interaction)
        .clickable(move |_point| {
            default_haptics().perform(HapticFeedback::ImpactLight);
            (on_click.borrow_mut())();
        })
        .size(Size::new(diameter, diameter));

    // The reference press: the icon ghosts while the glass lifts.
    let content_layer = Modifier::empty().graphics_layer(move || GraphicsLayer {
        alpha: content_alpha.get().clamp(0.0, 1.0) * foreground_alpha.clamp(0.0, 1.0),
        ..Default::default()
    });
    Box(
        base.then(modifier),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            let foreground_spec = spec.clone();
            Box(
                content_layer.clone(),
                BoxSpec::default().content_alignment(Alignment::CENTER),
                move || GlassIconForeground(foreground_spec.clone(), diameter, icon_path),
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_backplate_colors_only_the_compact_foreground_core() {
        let blue = Color::from_rgb_u8(0, 122, 255);
        let spec = GlassButtonSpec::glass()
            .with_icon_backplate(blue)
            .with_content_color(Color::WHITE);
        assert_eq!(spec.style, GlassButtonStyle::Glass);
        assert_eq!(spec.icon_backplate, Some(blue));
        assert_eq!(spec.content_color, Some(Color::WHITE));
        assert!(spec.glass.is_none());
        assert!((0.49..=0.51).contains(&ICON_BACKPLATE_DIAMETER_RATIO));
        assert!((0.27..=0.29).contains(&ICON_BACKPLATE_GLYPH_RATIO));

        let colors = crate::theme::LiquidColors::light(blue);
        let material = GlassButtonSpec::glass()
            .resolve_material(&colors, colors.label)
            .expect("glass button material");
        assert_eq!(material.tint, None);
        assert_eq!(material.resolve(&colors).tint, colors.glass_tint);
    }

    #[test]
    fn neutral_button_tint_comes_from_the_theme_not_foreground_polarity() {
        let accent = Color::from_rgb_u8(0, 122, 255);
        for colors in [
            crate::theme::LiquidColors::light(accent),
            crate::theme::LiquidColors::dark(accent),
        ] {
            let material = GlassButtonSpec::glass()
                .resolve_material(&colors, colors.label)
                .expect("glass button material");
            assert_eq!(material.tint, None);
            assert_eq!(material.resolve(&colors).tint, colors.glass_tint);
        }
    }
}
