use cranpose_macros::composable;
use cranpose_ui::{
    Dp, Modifier, density,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{
    Brush, Color, GradientBlurDirection, GradientBlurLevels, GradientBlurProfile, Rect, TileMode,
};

use crate::theme::liquid_colors;

// The iOS 27 scroll edge effects, measured on the simulator at 3x over
// black and white columns and stripes 3 and 12 points wide, scrolled under
// the status bar. The soft effect blurs by a radius that holds over the
// outer quarter of the effect and then eases off to nothing at its inner
// edge; in dark mode it also darkens toward the edge. The hard effect
// frosts evenly and ends at a hairline.
const SOFT_RADIUS_LIGHT: f32 = 13.15;
const SOFT_RADIUS_DARK: f32 = 12.0;
// Native's soft kernel keeps fine detail where it blurs least: below half
// its radius the content shows through the half level.
const SOFT_PROFILE: GradientBlurProfile = GradientBlurProfile {
    ramp_start: 0.25,
    ramp_end: 1.0,
    levels: GradientBlurLevels::SharpAndHalf,
};
// How much of black the dark soft effect lays over the content, from its
// outer edge to its inner edge.
const SOFT_DIM_DARK: [(f32, f32); 6] = [
    (0.0, 0.46),
    (0.29, 0.43),
    (0.48, 0.35),
    (0.68, 0.19),
    (0.87, 0.03),
    (1.0, 0.0),
];
const HARD_RADIUS_LIGHT: f32 = 13.05;
const HARD_RADIUS_DARK: f32 = 11.8;
const HARD_TINT_LIGHT: Color = Color::rgba(1.0, 1.0, 1.0, 0.54);
const HARD_TINT_DARK: Color = Color::rgba(14.0 / 255.0, 14.0 / 255.0, 14.0 / 255.0, 0.60);
const HAIRLINE_LIGHT: Color = Color::rgba(0.0, 0.0, 0.0, 0.10);
const HAIRLINE_DARK: Color = Color::rgba(1.0, 1.0, 1.0, 0.10);

/// The edge of a scrolling container that a [`LiquidScrollEdgeEffect`]
/// covers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LiquidScrollEdge {
    /// The top edge, under the status bar or a bar above the content.
    #[default]
    Top,
    /// The bottom edge, under the home indicator or a bar below the content.
    Bottom,
}

/// How a [`LiquidScrollEdgeEffect`] sets the content apart from what
/// stands over the edge. UIKit's `UIScrollEdgeEffect.Style`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LiquidScrollEdgeEffectStyle {
    /// The content blurs more toward the edge, and in dark mode darkens
    /// too, with no line where the effect ends: `.soft`, the look of a bare
    /// edge under the status bar.
    #[default]
    Soft,
    /// The content frosts evenly under the whole effect, which ends at a
    /// hairline: `.hard`.
    Hard,
}

/// Blurs the scrolling content that passes under it, as iOS does under the
/// status bar and the bars of a scroll view. Place it after the scrolling
/// content in a `Box`, sized by `modifier` to the edge's safe area inset
/// plus the height of any bar over that edge, so its blur reads the content
/// sliding underneath. `edge` names the edge of the effect that lies on the
/// container's edge.
///
/// UIKit's `UIScrollView.topEdgeEffect` and `bottomEdgeEffect`, SwiftUI's
/// `scrollEdgeEffectStyle(_:for:)`.
#[composable]
pub fn LiquidScrollEdgeEffect(
    modifier: Modifier,
    edge: LiquidScrollEdge,
    style: LiquidScrollEdgeEffectStyle,
) {
    let dark = liquid_colors().is_dark;
    // The effect is decoration: readers reach the content under it.
    let modifier = modifier.hide_from_accessibility();
    let modifier = match style {
        LiquidScrollEdgeEffectStyle::Soft => soft(modifier, edge, dark),
        LiquidScrollEdgeEffectStyle::Hard => hard(modifier, edge, dark),
    };
    Box(modifier, BoxSpec::default(), || {});
}

fn soft(modifier: Modifier, edge: LiquidScrollEdge, dark: bool) -> Modifier {
    let radius = if dark {
        SOFT_RADIUS_DARK
    } else {
        SOFT_RADIUS_LIGHT
    };
    let direction = match edge {
        LiquidScrollEdge::Top => GradientBlurDirection::TopToBottom,
        LiquidScrollEdge::Bottom => GradientBlurDirection::BottomToTop,
    };
    let blurred = modifier.backdrop_gradient_blur(Dp(radius), Dp(0.0), direction, SOFT_PROFILE);
    if !dark {
        return blurred;
    }
    // The dim runs from the edge inward; an infinite coordinate names the far
    // side of the painted area.
    let (from, to) = match edge {
        LiquidScrollEdge::Top => (0.0, f32::INFINITY),
        LiquidScrollEdge::Bottom => (f32::INFINITY, 0.0),
    };
    let dim = Brush::vertical_gradient_stops(
        SOFT_DIM_DARK
            .iter()
            .map(|&(at, alpha)| (at, Color::BLACK.with_alpha(alpha)))
            .collect(),
        from,
        to,
        TileMode::Clamp,
    );
    blurred.draw_behind(move |scope| scope.draw_rect(dim.clone()))
}

fn hard(modifier: Modifier, edge: LiquidScrollEdge, dark: bool) -> Modifier {
    let (radius, tint, line) = if dark {
        (HARD_RADIUS_DARK, HARD_TINT_DARK, HAIRLINE_DARK)
    } else {
        (HARD_RADIUS_LIGHT, HARD_TINT_LIGHT, HAIRLINE_LIGHT)
    };
    // One physical pixel.
    let thickness = 1.0 / density().density();
    modifier
        .backdrop_blur(Dp(radius))
        .draw_behind(move |scope| {
            let size = scope.size();
            scope.draw_rect(Brush::solid(tint));
            let y = match edge {
                LiquidScrollEdge::Top => size.height - thickness,
                LiquidScrollEdge::Bottom => 0.0,
            };
            scope.draw_rect_at(
                Rect {
                    x: 0.0,
                    y,
                    width: size.width,
                    height: thickness,
                },
                Brush::solid(line),
            );
        })
}
