use std::{cell::RefCell, rc::Rc};

use cranpose_core::State;
use cranpose_macros::composable;
use cranpose_services::HapticFeedback;
use cranpose_ui::{
    Modifier, Size,
    text::TextStyle,
    widgets::{Box, BoxSpec, Text},
};
use cranpose_ui_graphics::{Color, GraphicsLayer};
use cranpose_ui_layout::Alignment;

use super::floating_button::{FloatingButtonSurface, floating_button_content};
use crate::{
    material::{Glass, LiquidShape},
    theme::{liquid_colors, liquid_typography},
};

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
    /// The look of the button: glass, prominent, plain or destructive.
    pub style: GlassButtonStyle,
    /// Text button sizing; icon buttons use their explicit diameter.
    pub size: GlassButtonSize,
    /// Overrides the material (advanced).
    pub glass: Option<Glass>,
    /// Overrides the label/icon color (defaults per style).
    pub content_color: Option<Color>,
    /// Optional inner color disc for icon buttons. The outer material remains
    /// clear glass; the disc colors only the icon's compact foreground core.
    pub icon_backplate: Option<Color>,
}

/// Native text button sizes, including their label style and content insets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GlassButtonSize {
    /// Compact action with a 15 point label.
    Small,
    /// Standard action with a 17 point label.
    #[default]
    Regular,
    /// Large action with a 17 point label.
    Large,
}

// The least height of a text button and its horizontal and vertical padding.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct ButtonMetrics {
    pub(super) height: f32,
    pub(super) padding: (f32, f32),
}

impl GlassButtonSize {
    pub(super) fn metrics(self) -> ButtonMetrics {
        let (height, padding) = match self {
            Self::Small => (28.0, (10.0, 5.0)),
            Self::Regular => (34.0, (12.0, 7.0)),
            Self::Large => (50.0, (20.0, 15.0)),
        };
        ButtonMetrics { height, padding }
    }

    fn label_style(self, typography: crate::theme::LiquidTypography) -> TextStyle {
        match self {
            Self::Small => typography.subheadline,
            Self::Regular | Self::Large => typography.body,
        }
    }
}

impl GlassButtonSpec {
    /// A translucent glass button with a label in the accent color.
    pub fn glass() -> Self {
        Self::default()
    }

    /// A button of accent-tinted glass with white content, for the primary action.
    pub fn prominent() -> Self {
        Self {
            style: GlassButtonStyle::Prominent,
            ..Self::default()
        }
    }

    /// A button without material: only the label, with press feedback.
    pub fn plain() -> Self {
        Self {
            style: GlassButtonStyle::Plain,
            ..Self::default()
        }
    }

    /// A plain button in the destructive color.
    pub fn destructive() -> Self {
        Self {
            style: GlassButtonStyle::Destructive,
            ..Self::default()
        }
    }

    /// Returns the spec with `glass` as its material instead of the material of its style.
    pub fn with_glass(mut self, glass: Glass) -> Self {
        self.glass = Some(glass);
        self
    }

    /// Returns the spec with `color` as its label and icon color instead of the default of its
    /// style.
    pub fn with_content_color(mut self, color: Color) -> Self {
        self.content_color = Some(color);
        self
    }

    /// Sets the label style and padding used by a text button.
    pub fn with_size(mut self, size: GlassButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Returns the spec with a color disc behind the icon of an icon button.
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

    pub(super) fn resolve_material(
        &self,
        colors: &crate::theme::LiquidColors,
        foreground: Color,
        height: f32,
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
            GlassButtonStyle::Glass => Some(super::glass_surface::floating_material(
                colors.label,
                height,
            )),
            // Over a dark backdrop iOS keeps the accent at full strength.
            GlassButtonStyle::Prominent => Some(
                super::glass_surface::floating_material(colors.label, height)
                    .transmission_tint(colors.accent, if colors.is_dark { 1.0 } else { 0.6 }),
            ),
            GlassButtonStyle::Plain | GlassButtonStyle::Destructive => None,
        }
    }
}

/// A glass button. `content` composes the label (see [`GlassButton`] with
/// [`Text`], or an [`crate::icons::Icon`] + text row).
#[composable]
pub fn GlassButton(
    modifier: Modifier,
    spec: GlassButtonSpec,
    on_click: impl Fn() + 'static,
    content: impl FnMut() + 'static,
) {
    let metrics = spec.size.metrics();
    GlassButtonWithFeedback(
        modifier,
        spec,
        metrics,
        HapticFeedback::ImpactLight,
        None,
        on_click,
        content,
    );
}

#[composable]
pub(crate) fn GlassButtonWithFeedback(
    modifier: Modifier,
    spec: GlassButtonSpec,
    metrics: ButtonMetrics,
    feedback: HapticFeedback,
    tint_progress: Option<State<f32>>,
    on_click: impl Fn() + 'static,
    content: impl FnMut() + 'static,
) {
    let colors = liquid_colors();
    let material = spec.resolve_material(&colors, spec.content_color(&colors), metrics.height);
    let (pressed_modifier, base, gesture, illumination, content_activity) = FloatingButtonSurface(
        material,
        spec.glass.is_none(),
        0.0,
        feedback,
        tint_progress.map(|state| {
            (
                state,
                super::glass_surface::floating_material(colors.label, metrics.height)
                    .tint
                    .unwrap_or(Color::TRANSPARENT),
            )
        }),
        on_click,
    );

    let (horizontal_padding, vertical_padding) = metrics.padding;
    let base = base
        .height_in(metrics.height, f32::INFINITY)
        .padding_symmetric(horizontal_padding, vertical_padding);

    let content_fade = if matches!(
        spec.style,
        GlassButtonStyle::Glass | GlassButtonStyle::Prominent
    ) {
        0.0
    } else {
        0.35
    };
    let content_layer = illumination.graphics_layer(move || GraphicsLayer {
        alpha: (1.0 - content_fade * content_activity.get()).clamp(0.0, 1.0),
        ..Default::default()
    });
    let content = Rc::new(RefCell::new(content));
    floating_button_content(
        modifier.then(gesture),
        pressed_modifier,
        base,
        content_layer,
        move || (content.borrow_mut())(),
    );
}

/// Convenience text label styled for the enclosing button.
#[composable]
pub fn GlassButtonLabel(text: cranpose_ui::SharedText, spec: GlassButtonSpec) {
    let mut style = spec.size.label_style(liquid_typography());
    let color = spec.content_color(&liquid_colors());
    style.span_style.color = Some(color);
    Text(text, Modifier::empty(), style);
}

#[composable]
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
                crate::icons::Icon(
                    icon_path,
                    None,
                    diameter * ICON_BACKPLATE_GLYPH_RATIO,
                    icon_color,
                );
            },
        );
    } else {
        crate::icons::Icon(icon_path, None, diameter * 0.5, icon_color);
    }
}

/// A circular glass icon button (44dp target).
#[composable]
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
pub(crate) fn GlassIconButtonWithForegroundAlpha(
    modifier: Modifier,
    spec: GlassButtonSpec,
    diameter: f32,
    foreground_alpha: f32,
    on_click: impl Fn() + 'static,
    icon_path: &'static str,
) {
    let colors = liquid_colors();
    let material = spec
        .resolve_material(&colors, spec.icon_color(&colors), diameter)
        .map(|glass| glass.shape(LiquidShape::Circle));
    let (pressed_modifier, base, gesture, illumination, content_activity) = FloatingButtonSurface(
        material,
        spec.glass.is_none(),
        diameter * 0.25,
        HapticFeedback::ImpactLight,
        None,
        on_click,
    );

    let base = base.size(Size::new(diameter, diameter));

    let content_layer = illumination.graphics_layer(move || GraphicsLayer {
        alpha: (1.0 - 0.85 * content_activity.get()).clamp(0.0, 1.0)
            * foreground_alpha.clamp(0.0, 1.0),
        ..Default::default()
    });
    floating_button_content(
        modifier.then(gesture),
        pressed_modifier,
        base,
        content_layer,
        move || GlassIconForeground(spec.clone(), diameter, icon_path),
    );
}

#[cfg(test)]
#[path = "tests/button_tests.rs"]
mod tests;
