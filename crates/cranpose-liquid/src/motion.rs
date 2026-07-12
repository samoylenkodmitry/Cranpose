//! Liquid motion: the spring presets every component shares, and the press
//! interaction (scale + specular boost) that makes glass feel physical.

use cranpose_animation::{spring, AnimationType};
use cranpose_core::State;
use cranpose_macros::composable;
use cranpose_ui::Modifier;
use cranpose_ui::MutableInteractionSource;
use cranpose_ui_graphics::GraphicsLayer;

/// Named springs used across the Liquid components (value-space, velocity
/// preserving).
pub struct LiquidMotion;

impl LiquidMotion {
    /// Snappy interactions: presses, toggles, selection moves.
    pub fn snappy() -> AnimationType {
        spring(0.85, 900.0)
    }

    /// The droplet feel: visible overshoot for morphing shapes.
    pub fn bouncy() -> AnimationType {
        spring(0.55, 500.0)
    }

    /// Gentle settle for large surfaces (sheets, menus).
    pub fn smooth() -> AnimationType {
        spring(1.0, 400.0)
    }

    /// The leading edge of a stretching selection blob (runs ahead).
    pub fn blob_leading() -> AnimationType {
        spring(0.8, 900.0)
    }

    /// The trailing edge of a stretching selection blob (drags behind, giving
    /// the droplet elongation while in motion).
    pub fn blob_trailing() -> AnimationType {
        spring(0.9, 380.0)
    }

    /// A released lens flying to its committed slot: the reference tab-bar
    /// transit crosses ~3 cells in ~330 ms (measured from the iphone17
    /// recording) — much gentler than the finger-chase spring.
    pub fn glide() -> AnimationType {
        spring(0.9, 260.0)
    }
}

/// Press feedback for glass controls, per the Liquid Glass law: touched glass
/// GROWS (spring scale toward `pressed_scale` — never smaller) and turns MORE
/// TRANSPARENT (the returned content alpha dips while pressed, the reference
/// "…" dots fading as the button lifts). Returns the pressed state so callers
/// can also boost the specular highlight.
///
/// Apply the returned modifier *outside* the glass effect so the whole lens
/// scales together; apply the content alpha to the label/icon layer.
#[composable]
pub fn liquid_press_scale(
    modifier: Modifier,
    interaction_source: MutableInteractionSource,
    pressed_scale: f32,
) -> (Modifier, State<bool>, State<f32>) {
    let pressed = interaction_source.collectIsPressedAsState();
    let scale = cranpose_animation::animateFloatAsState(
        if pressed.get() {
            pressed_scale.max(1.0)
        } else {
            1.0
        },
        LiquidMotion::snappy(),
        "liquid-press-scale",
    );
    let content_alpha = cranpose_animation::animateFloatAsState(
        // The reference down-state ghosts glyphs hard (the menu button's
        // dots drop to ~30% while held).
        if pressed.get() { 0.35 } else { 1.0 },
        LiquidMotion::smooth(),
        "liquid-press-content",
    );
    let modifier = modifier.graphics_layer(move || {
        let scale = scale.get();
        GraphicsLayer {
            scale_x: scale,
            scale_y: scale,
            ..Default::default()
        }
    });
    (modifier, pressed, content_alpha)
}
