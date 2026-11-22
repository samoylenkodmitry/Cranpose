//! How a control shows that a pointer rests on it or presses it: Compose's
//! `Indication` and `LocalIndication`.
//!
//! Every [`clickable`](crate::Modifier::clickable) draws the indication that
//! [`local_indication`] holds where the modifier was made. An app sets its own
//! look once, at its root, and every button, row and toggle under it shows it.

use cranpose_core::{CompositionLocal, compositionLocalOf, try_with_composer};
use cranpose_ui_graphics::Color;

/// What a control shows while a pointer rests on it and while one presses
/// it: a color laid over the control's surface. Compose's `Indication`.
///
/// The default is Compose's `DefaultDebugIndication`: black at 10% while a
/// pointer hovers and at 30% while one presses. A dark interface takes a
/// light overlay with [`Indication::overlay`], and
/// [`Indication::NONE`] shows nothing. The overlay covers the control's
/// surface: the rect and corner shape of the background of the control's
/// modifier chain, or the control's own rect when it has no background.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Indication {
    hovered: Color,
    pressed: Color,
}

impl Indication {
    /// Shows nothing: Compose's `indication = null`.
    pub const NONE: Self = Self {
        hovered: Color::TRANSPARENT,
        pressed: Color::TRANSPARENT,
    };

    /// Lays `hovered` over the control while a pointer rests on it, and
    /// `pressed` while one presses it.
    pub const fn overlay(hovered: Color, pressed: Color) -> Self {
        Self { hovered, pressed }
    }

    /// The color over a control that a pointer hovers, or `None` for no
    /// overlay.
    pub fn hovered(&self) -> Option<Color> {
        visible(self.hovered)
    }

    /// The color over a control that a pointer presses, or `None` for no
    /// overlay.
    pub fn pressed(&self) -> Option<Color> {
        visible(self.pressed)
    }

    /// The color over a control in `state`, or `None` while it shows
    /// nothing. A press shows over a hover.
    pub fn color(&self, state: IndicationState) -> Option<Color> {
        match state {
            IndicationState::Idle => None,
            IndicationState::Hovered => self.hovered(),
            IndicationState::Pressed => self.pressed(),
        }
    }
}

impl Default for Indication {
    fn default() -> Self {
        Self::overlay(
            Color::from_rgba_u8(0, 0, 0, 26),
            Color::from_rgba_u8(0, 0, 0, 77),
        )
    }
}

fn visible(color: Color) -> Option<Color> {
    (color.3 > 0.0).then_some(color)
}

/// Where a control's interaction stands, as its indication shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IndicationState {
    /// No pointer rests on the control or presses it.
    #[default]
    Idle,
    /// A pointer rests on the control.
    Hovered,
    /// A pointer presses the control, and its release would click it.
    Pressed,
}

/// The [`CompositionLocal`] that holds the [`Indication`] the clickables
/// under it draw. Compose's `LocalIndication`.
///
/// ```ignore
/// CompositionLocalProvider(
///     [local_indication().provides(Indication::overlay(
///         Color::from_rgba_u8(255, 255, 255, 30),
///         Color::from_rgba_u8(255, 255, 255, 70),
///     ))],
///     || App(),
/// );
/// ```
pub fn local_indication() -> CompositionLocal<Indication> {
    crate::environment_locals::cached_local(
        |locals| &locals.indication,
        || compositionLocalOf(Indication::default),
    )
}

// The indication that a clickable made now takes: the local's value in a
// composition, and the default outside one.
pub(crate) fn current_indication() -> Indication {
    let local = local_indication();
    try_with_composer(|composer| composer.read_composition_local(&local))
        .unwrap_or_else(|| local.default_value())
}

#[cfg(test)]
#[path = "tests/indication_tests.rs"]
mod tests;
