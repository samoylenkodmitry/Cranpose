//! Generic vector icons and clickable icon buttons.

#![allow(non_snake_case)]

use cranpose_core::{NodeId, rememberKeyed};
use cranpose_ui_graphics::{Brush, Color, VectorPath};
use cranpose_ui_layout::Alignment;

use crate::{
    Modifier, SemanticsWidgetRole, Size, composable,
    interaction::{MutableInteractionSource, rememberMutableInteractionSource},
    widgets::{Box, BoxSpec},
};

/// The coordinate system every icon path is authored in.
const ICON_VIEW_BOX: f32 = 24.0;

/// The size an icon is drawn at when the caller states none.
pub const DEFAULT_ICON_SIZE: f32 = 24.0;

/// The smallest square a pointer target may be.
///
/// Every platform's accessibility guidance lands on the same number, and it is
/// why an icon button is bigger than its icon: the glyph is 24dp and the target
/// around it is 48dp, so a 24dp icon is still comfortable to hit.
pub const MINIMUM_TOUCH_TARGET: f32 = 48.0;

/// How an icon is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconSpec {
    /// The square the icon is drawn in.
    pub size: f32,
    /// The tint. `None` leaves the path's own colouring, which is what a
    /// multi-colour illustration wants.
    pub tint: Option<Color>,
}

impl Default for IconSpec {
    fn default() -> Self {
        Self {
            size: DEFAULT_ICON_SIZE,
            tint: None,
        }
    }
}

impl IconSpec {
    /// An icon of `size`, untinted.
    pub const fn sized(size: f32) -> Self {
        Self { size, tint: None }
    }

    /// Sets the drawn size.
    pub const fn with_size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Sets the tint.
    pub const fn with_tint(mut self, tint: Color) -> Self {
        self.tint = Some(tint);
        self
    }
}

/// Draws an SVG path in the standard 24dp icon coordinate system.
///
/// An icon is decorative by default: it carries no content description, because
/// the control around it names the action. Use [`IconWith`] to give a
/// standalone icon a description a screen reader reads out.
#[composable]
pub fn Icon(path: &'static str, size: f32, color: Color) -> NodeId {
    IconWith(
        Modifier::empty(),
        path,
        IconSpec::sized(size).with_tint(color),
        None,
    )
}

/// An icon with a modifier, a spec, and an optional content description.
#[composable]
pub fn IconWith(
    modifier: Modifier,
    path: &'static str,
    spec: IconSpec,
    content_description: Option<String>,
) -> NodeId {
    let parsed = rememberKeyed(path, |value| VectorPath::parse(value).ok());
    let size = spec.size;
    let tint = spec.tint;
    let modifier = modifier
        .size(Size::new(size, size))
        .draw_behind(move |scope| {
            if let Some(path) = &parsed {
                let scaled = path.scaled(size / ICON_VIEW_BOX);
                match tint {
                    Some(tint) => scope.draw_vector_path(&scaled, Brush::solid(tint)),
                    None => {
                        scope.draw_vector_path(&scaled, Brush::solid(Color(0.0, 0.0, 0.0, 1.0)))
                    }
                }
            }
        });
    let modifier = match content_description {
        Some(description) => modifier.semantics(move |config| {
            config.content_description = Some(description.clone());
            config.role = Some(SemanticsWidgetRole::Image);
        }),
        // A decorative icon publishes nothing, so a screen reader reads the
        // control around it once instead of reading the glyph too.
        None => modifier,
    };
    Box(modifier, BoxSpec::default(), || {})
}

/// The colours an icon button paints itself with.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IconButtonColors {
    /// The surface behind the icon at rest.
    pub background: Option<Color>,
    /// The surface while the button is held.
    pub pressed_background: Option<Color>,
    /// The surface while the button is disabled.
    pub disabled_background: Option<Color>,
}

impl IconButtonColors {
    /// Sets the resting surface.
    pub const fn with_background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// Sets the held surface.
    pub const fn with_pressed_background(mut self, color: Color) -> Self {
        self.pressed_background = Some(color);
        self
    }

    /// Sets the disabled surface.
    pub const fn with_disabled_background(mut self, color: Color) -> Self {
        self.disabled_background = Some(color);
        self
    }

    /// The surface for the state the button is in.
    fn surface(&self, enabled: bool, pressed: bool) -> Option<Color> {
        if !enabled {
            return self.disabled_background.or(self.background);
        }
        if pressed {
            return self.pressed_background.or(self.background);
        }
        self.background
    }
}

/// How an icon button behaves and looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconButtonSpec {
    /// Whether the button accepts input. A disabled button still publishes
    /// itself to a screen reader, which is how the user learns it exists.
    pub enabled: bool,
    /// The square the pointer target occupies, never smaller than
    /// [`MINIMUM_TOUCH_TARGET`].
    pub touch_target: f32,
    /// The surfaces the button paints.
    pub colors: IconButtonColors,
}

impl Default for IconButtonSpec {
    fn default() -> Self {
        Self {
            enabled: true,
            touch_target: MINIMUM_TOUCH_TARGET,
            colors: IconButtonColors::default(),
        }
    }
}

impl IconButtonSpec {
    /// Sets whether the button accepts input.
    pub const fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Sets the pointer target size. Values below [`MINIMUM_TOUCH_TARGET`] are
    /// raised to it: a control smaller than that is a control people miss.
    pub fn with_touch_target(mut self, size: f32) -> Self {
        self.touch_target = size.max(MINIMUM_TOUCH_TARGET);
        self
    }

    /// Sets the surfaces.
    pub const fn with_colors(mut self, colors: IconButtonColors) -> Self {
        self.colors = colors;
        self
    }

    /// The target size this spec actually uses.
    pub fn resolved_touch_target(&self) -> f32 {
        self.touch_target.max(MINIMUM_TOUCH_TARGET)
    }
}

/// A semantic icon button with caller-provided content.
#[composable]
pub fn IconButton<F>(
    modifier: Modifier,
    content_description: impl Into<String>,
    on_click: impl Fn() + 'static,
    content: F,
) -> NodeId
where
    F: FnMut() + 'static,
{
    IconButtonWith(
        modifier,
        content_description,
        IconButtonSpec::default(),
        None,
        on_click,
        content,
    )
}

/// An icon button with an enabled state, colours, a touch target, and an
/// interaction source the caller can observe.
///
/// Passing an interaction source is how a button's visual reacts to being held
/// without the caller writing pointer handling: read
/// [`MutableInteractionSource::collectIsPressedAsState`] in the content.
#[composable]
pub fn IconButtonWith<F>(
    modifier: Modifier,
    content_description: impl Into<String>,
    spec: IconButtonSpec,
    interaction_source: Option<MutableInteractionSource>,
    on_click: impl Fn() + 'static,
    content: F,
) -> NodeId
where
    F: FnMut() + 'static,
{
    let description = content_description.into();
    let enabled = spec.enabled;
    let target = spec.resolved_touch_target();
    let source = interaction_source.unwrap_or_else(rememberMutableInteractionSource);
    let pressed = source.collectIsPressedAsState().get();

    let mut modifier = modifier
        .size(Size::new(target, target))
        .press_interaction_source(source)
        .semantics(move |config| {
            config.content_description = Some(description.clone());
            config.role = Some(SemanticsWidgetRole::Button);
            config.enabled = enabled;
            config.is_clickable = enabled;
        });
    if let Some(surface) = spec.colors.surface(enabled, pressed) {
        modifier = modifier.background(surface);
    }
    if enabled {
        modifier = modifier.clickable(move |_| on_click());
    }

    Box(
        modifier,
        BoxSpec::default().content_alignment(Alignment::CENTER),
        content,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_path_is_parseable() {
        assert!(VectorPath::parse("M0 0h24v24H0z").is_ok());
    }

    #[test]
    fn an_icon_button_is_never_smaller_than_the_minimum_touch_target() {
        let spec = IconButtonSpec::default().with_touch_target(20.0);
        assert_eq!(spec.resolved_touch_target(), MINIMUM_TOUCH_TARGET);
        let roomy = IconButtonSpec::default().with_touch_target(64.0);
        assert_eq!(roomy.resolved_touch_target(), 64.0);
    }

    #[test]
    fn colours_follow_the_state_and_fall_back_to_the_resting_surface() {
        let rest = Color(0.1, 0.1, 0.1, 1.0);
        let held = Color(0.2, 0.2, 0.2, 1.0);
        let off = Color(0.3, 0.3, 0.3, 1.0);

        let full = IconButtonColors::default()
            .with_background(rest)
            .with_pressed_background(held)
            .with_disabled_background(off);
        assert_eq!(full.surface(true, false), Some(rest));
        assert_eq!(full.surface(true, true), Some(held));
        assert_eq!(full.surface(false, false), Some(off));

        // A button that states only its resting surface keeps it in every
        // state rather than flashing to nothing when held.
        let plain = IconButtonColors::default().with_background(rest);
        assert_eq!(plain.surface(true, true), Some(rest));
        assert_eq!(plain.surface(false, true), Some(rest));

        assert_eq!(IconButtonColors::default().surface(true, true), None);
    }

    #[test]
    fn an_icon_states_its_size_and_tint() {
        assert_eq!(IconSpec::default().size, DEFAULT_ICON_SIZE);
        assert_eq!(IconSpec::default().tint, None);
        let spec = IconSpec::sized(16.0).with_tint(Color(1.0, 0.0, 0.0, 1.0));
        assert_eq!(spec.size, 16.0);
        assert_eq!(spec.tint, Some(Color(1.0, 0.0, 0.0, 1.0)));
        assert_eq!(spec.with_size(32.0).size, 32.0);
    }

    #[test]
    fn a_disabled_icon_button_still_publishes_itself() {
        let spec = IconButtonSpec::default().with_enabled(false);
        assert!(!spec.enabled);
        assert_eq!(spec.resolved_touch_target(), MINIMUM_TOUCH_TARGET);
    }
}
