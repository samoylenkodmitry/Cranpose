use std::{cell::RefCell, rc::Rc};

use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, ScrollState,
    text::{SpanStyle, TextStyle},
    widgets::{Box, BoxSpec, Row, RowSpec, Text},
};
use cranpose_ui_graphics::GraphicsLayer;
use cranpose_ui_layout::{Alignment, VerticalAlignment};

use crate::{
    material::{Glass, GlassDynamics, LiquidModifierExt, LiquidShape},
    theme::{liquid_colors, liquid_typography},
};

/// Configuration for [`LiquidNavBar`].
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidNavBarSpec {
    pub title: String,
    /// Scroll distance (dp) over which the large title collapses.
    pub collapse_range: f32,
}

impl LiquidNavBarSpec {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            collapse_range: 52.0,
        }
    }
}

const BAR_HEIGHT: f32 = 52.0;
const LARGE_TITLE_HEIGHT: f32 = 52.0;

/// A large-title navigation bar driven by the content's [`ScrollState`]
/// (offset 0 = top). `leading`/`trailing` compose the bar buttons.
///
/// The bar installs a settle policy on the scroll so it can never rest inside
/// the large-title collapse band — releasing (or wheel-idling) mid-band snaps
/// to fully expanded or fully collapsed, exactly like `UINavigationBar`. The
/// large title composes UNDER the glass band, so mid-collapse it slides
/// beneath the frost instead of floating readable next to the inline title.
///
/// Place the bar *after* the scrolling content inside a `Box` so its glass
/// samples the content sliding underneath.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidNavBar(
    modifier: Modifier,
    spec: LiquidNavBarSpec,
    scroll: ScrollState,
    leading: impl FnMut() + 'static,
    trailing: impl FnMut() + 'static,
) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    let leading = Rc::new(RefCell::new(leading));
    let trailing = Rc::new(RefCell::new(trailing));

    let collapse_range = spec.collapse_range;
    if std::env::var_os("CRANPOSE_DISABLE_NAV_SNAP").is_none() {
        scroll.set_settle_policy(Some(large_title_settle_policy(collapse_range)));
    }
    let scroll_offset = scroll.value();

    let progress = (scroll_offset / spec.collapse_range.max(1.0)).clamp(0.0, 1.0);
    let title = spec.title.clone();

    Box(modifier, BoxSpec::default(), move || {
        let large_alpha = (1.0 - progress * 1.6).clamp(0.0, 1.0);
        if large_alpha > 0.0 {
            let style = TextStyle {
                span_style: SpanStyle {
                    color: Some(colors.label.with_alpha(large_alpha)),
                    ..typography.large_title.span_style.clone()
                },
                ..typography.large_title.clone()
            };
            let slide = -scroll_offset.min(collapse_range);
            let large_title = title.clone();
            Box(
                Modifier::empty()
                    .offset(16.0, BAR_HEIGHT)
                    .height(LARGE_TITLE_HEIGHT)
                    .graphics_layer(move || GraphicsLayer {
                        translation_y: slide,
                        ..Default::default()
                    }),
                BoxSpec::default().content_alignment(Alignment::new(
                    cranpose_ui_layout::HorizontalAlignment::Start,
                    cranpose_ui_layout::VerticalAlignment::CenterVertically,
                )),
                move || {
                    Text(large_title.clone(), Modifier::empty(), style.clone());
                },
            );
        }

        let band = Modifier::empty()
            .fill_max_width()
            .height(BAR_HEIGHT)
            .glass_effect_with(
                Glass::regular()
                    .shape(LiquidShape::RoundedRect(0.0))
                    .blur_radius(30.0)
                    .saturation(1.15)
                    .refraction_depth(0.0)
                    .transmission_refraction(0.0)
                    .highlight(0.18)
                    .adaptive_frost(colors.label, 0.75)
                    .shadow(false),
                move || GlassDynamics {
                    activity: Some(progress),
                    ..Default::default()
                },
            );
        Box(band, BoxSpec::default(), || {});

        let inline_alpha = ((progress - 0.5) * 2.0).clamp(0.0, 1.0);
        let inline_title = title.clone();
        let inline_typography = typography.clone();
        let leading = Rc::clone(&leading);
        let trailing = Rc::clone(&trailing);
        Row(
            Modifier::empty()
                .fill_max_width()
                .height(BAR_HEIGHT)
                .padding_horizontal(10.0),
            RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
            move || {
                (leading.borrow_mut())();
                let style = TextStyle {
                    span_style: SpanStyle {
                        color: Some(colors.label.with_alpha(inline_alpha)),
                        ..inline_typography.headline.span_style.clone()
                    },
                    ..inline_typography.headline.clone()
                };
                let inline_title = inline_title.clone();
                Box(
                    Modifier::empty().weight(1.0),
                    BoxSpec::default().content_alignment(Alignment::CENTER),
                    move || {
                        Text(inline_title.clone(), Modifier::empty(), style.clone());
                    },
                );
                (trailing.borrow_mut())();
            },
        );
    });
}

pub fn large_title_settle_policy(collapse_range: f32) -> cranpose_ui::ScrollSettlePolicy {
    let range = collapse_range.max(1.0);
    Rc::new(move |proposed: f32, _velocity: f32| {
        if proposed <= 0.0 || proposed >= range {
            proposed
        } else if proposed < range * 0.5 {
            0.0
        } else {
            range
        }
    })
}

/// Total height the bar family occupies while expanded (inline bar + large
/// title) — content's top padding.
pub fn liquid_nav_bar_expanded_height() -> f32 {
    BAR_HEIGHT + LARGE_TITLE_HEIGHT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanded_height_covers_the_inline_bar_and_its_large_title() {
        assert_eq!(
            liquid_nav_bar_expanded_height(),
            BAR_HEIGHT + LARGE_TITLE_HEIGHT,
            "content's top padding has to clear both rows"
        );
        assert!(liquid_nav_bar_expanded_height() > BAR_HEIGHT);
    }

    #[test]
    fn a_rest_inside_the_collapse_band_snaps_to_the_nearer_edge() {
        let range = 52.0_f32;
        let settle = large_title_settle_policy(range);
        assert_eq!(settle(1.0, 0.0), 0.0, "just past expanded unfolds again");
        assert_eq!(
            settle(range - 1.0, 0.0),
            range,
            "nearly collapsed finishes collapsing"
        );
        assert_eq!(
            settle(range * 0.5, 0.0),
            range,
            "the midpoint commits to collapsed"
        );
        assert_eq!(
            settle(range * 0.5 - 0.1, 0.0),
            0.0,
            "a hair under it unfolds"
        );
    }

    #[test]
    fn a_rest_outside_the_collapse_band_is_left_alone() {
        let range = 52.0_f32;
        let settle = large_title_settle_policy(range);
        assert_eq!(settle(0.0, 0.0), 0.0);
        assert_eq!(
            settle(-30.0, 0.0),
            -30.0,
            "an overscrolled top keeps its bounce"
        );
        assert_eq!(settle(range, 0.0), range);
        assert_eq!(settle(900.0, 0.0), 900.0, "a deep scroll is never rewound");
    }

    #[test]
    fn a_degenerate_collapse_range_still_yields_a_usable_policy() {
        let settle = large_title_settle_policy(0.0);
        assert_eq!(
            settle(0.5, 0.0),
            1.0,
            "the range floors at one dp, not zero"
        );
        assert_eq!(settle(0.0, 0.0), 0.0);
    }
}
