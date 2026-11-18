// `LiquidScrollEdgeEffect` over a list through the headless robot: a tap in
// the effect's area reaches the content under it, in either style, and the
// effect adds nothing for readers.

use std::rc::Rc;

use cranpose_liquid::prelude::*;
use cranpose_ui::{
    Modifier,
    widgets::{Box, BoxSpec, Text},
};

use crate::reader::{Log, Robot, liquid_screen, nodes, tap};

// A full-window row that logs its taps, under a 100 point effect at the top
// when `style` names one.
fn screen(style: Option<LiquidScrollEdgeEffectStyle>) -> (Robot, Log) {
    liquid_screen(move |log| {
        let record = Rc::clone(log);
        Box(
            Modifier::empty().fill_max_size(),
            BoxSpec::default(),
            move || {
                let record = Rc::clone(&record);
                Box(
                    Modifier::empty()
                        .fill_max_size()
                        .clickable(move |_| record.borrow_mut().push("row")),
                    BoxSpec::default(),
                    || {
                        Text("Row", Modifier::empty(), liquid_typography().body);
                    },
                );
                if let Some(style) = style {
                    LiquidScrollEdgeEffect(
                        Modifier::empty().fill_max_width().height(100.0),
                        LiquidScrollEdge::Top,
                        style,
                    );
                }
            },
        );
    })
}

#[test]
fn a_tap_under_the_effect_reaches_the_content_in_either_style() {
    for style in [
        LiquidScrollEdgeEffectStyle::Soft,
        LiquidScrollEdgeEffectStyle::Hard,
    ] {
        let (mut robot, log) = screen(Some(style));
        tap(&mut robot, 200.0, 50.0);
        assert_eq!(*log.borrow(), ["row"], "{style:?}: the row took the tap");
        let (mut bare, _) = screen(None);
        assert_eq!(
            nodes(&mut robot).len(),
            nodes(&mut bare).len(),
            "{style:?}: the effect adds no reader node"
        );
    }
}
