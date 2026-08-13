//! A Wear screen: content, with a curved scroll indicator over it.
//!
//! `ScreenScaffold` is thinner than it looks. It fills the display, puts the
//! content in it, and overlays the indicator at centre-end. Two things it does
//! **not** do are worth stating, because both are easy to add by mistake:
//!
//! - it does not adjust the content padding it was handed. The `edgeButton`
//!   overloads replace it; the plain one passes it straight through, so a list
//!   inside receives exactly the padding the caller wrote;
//! - it does not draw a `TimeText`. Wear puts one here by default and an app
//!   that does not want one passes `timeText = null`, which is the case this
//!   reproduces. Adding one would put a clock on every screen built with this.

#![allow(non_snake_case)]

use crate::composable;
use crate::modifier::Modifier;
use crate::widgets::wear::scaling_list::WearScalingListState;
use crate::widgets::wear::scroll_indicator::{ScrollIndicator, ScrollIndicatorSpec};
use crate::widgets::{Box as CranposeBox, BoxSpec};
use cranpose_core::NodeId;
use cranpose_ui_layout::Alignment;

/// How a [`ScreenScaffold`] is laid out.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ScreenScaffoldSpec {
    pub indicator: ScrollIndicatorSpec,
    /// Whether the indicator is drawn at all.
    ///
    /// Wear returns to `Idle` two seconds after the last scroll and fades the
    /// indicator out, so a settled screenshot shows none. An app with a
    /// standing accessibility requirement to keep it visible leaves this on and
    /// holds the alpha at one — which is a policy the scaffold takes rather
    /// than owns.
    pub show_indicator: bool,
}

impl ScreenScaffoldSpec {
    pub fn indicator(mut self, indicator: ScrollIndicatorSpec) -> Self {
        self.indicator = indicator;
        self.show_indicator = true;
        self
    }
}

/// The `IDLE_DELAY` after which Wear's own scaffold fades its indicator, in
/// milliseconds, and the spring it fades on.
pub const INDICATOR_IDLE_DELAY_MS: u64 = 2000;
pub const INDICATOR_FADE_STIFFNESS: f32 = 400.0;
pub const INDICATOR_FADE_DAMPING_RATIO: f32 = 1.0;

/// A full-screen Wear scaffold.
#[composable]
pub fn ScreenScaffold<F>(
    modifier: Modifier,
    state: WearScalingListState,
    spec: ScreenScaffoldSpec,
    content: F,
) -> NodeId
where
    F: FnMut() + 'static,
{
    CranposeBox(
        modifier.fill_max_size(),
        BoxSpec::default().content_alignment(Alignment::TOP_START),
        move || {
            content();
            if spec.show_indicator {
                ScrollIndicator(Modifier::empty(), state.clone(), spec.indicator);
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fade_is_the_spring_and_the_delay_wear_declares() {
        assert_eq!(INDICATOR_IDLE_DELAY_MS, 2000);
        assert_eq!(INDICATOR_FADE_STIFFNESS, 400.0, "StiffnessMediumLow");
        assert_eq!(INDICATOR_FADE_DAMPING_RATIO, 1.0);
    }

    #[test]
    fn a_scaffold_shows_no_indicator_until_one_is_asked_for() {
        assert!(!ScreenScaffoldSpec::default().show_indicator);
        assert!(
            ScreenScaffoldSpec::default()
                .indicator(ScrollIndicatorSpec::default())
                .show_indicator
        );
    }
}
