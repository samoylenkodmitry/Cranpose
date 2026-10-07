//! The Gauntlet tab: the screen the framework comparison measures every night
//! (`benchmarks/compose-vs-cranpose`), at the load tier the chips choose.

use std::rc::Rc;

use cranpose_core::{key, rememberMutableStateOf, MutableState};
use cranpose_ui::{
    composable, Button, ButtonSpec, Color, Column, ColumnSpec, LinearArrangement, Modifier, Row,
    RowSpec, Text,
};
use perf_compare::{GauntletLoad, GauntletScreen};

use super::demo_text::text_style;

/// The tiers the comparison defines, lightest first.
const TIERS: usize = 16;
/// The tier the phones are measured at.
const FIRST_TIER: usize = 12;
const CHOSEN: Color = Color::from_rgb_u8(0x25, 0x63, 0xEB);
const CHIP: Color = Color::from_rgb_u8(0xE2, 0xE8, 0xF0);
const INK: Color = Color::from_rgb_u8(0x11, 0x18, 0x27);

#[composable]
pub fn GauntletTab() {
    let tier = rememberMutableStateOf(|| FIRST_TIER);
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            TierChips(tier);
            let chosen = tier.get();
            // A new tier starts the screen over, as a launch at that tier does.
            key(chosen, move || {
                GauntletScreen(GauntletLoad {
                    tier: chosen,
                    freeze: 0,
                });
            });
        },
    );
}

#[composable]
fn TierChips(tier: MutableState<usize>) {
    let choose: Rc<dyn Fn(usize)> = Rc::new(move |chosen| tier.set(chosen));
    Row(
        Modifier::empty().fill_max_width().padding(8.0),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(4.0)),
        move || {
            Text(
                "Tier",
                Modifier::empty().padding_symmetric(4.0, 4.0),
                text_style(13.0, INK, true),
            );
            for chip in 1..=TIERS {
                let chosen = tier.get() == chip;
                let choose = Rc::clone(&choose);
                Button(
                    Modifier::empty()
                        .background(if chosen { CHOSEN } else { CHIP })
                        .rounded_corners(10.0)
                        // Tall enough to be a touch target the audit accepts.
                        .padding_symmetric(8.0, 7.0),
                    ButtonSpec::default(),
                    move || choose(chip),
                    move || {
                        Text(
                            chip.to_string(),
                            Modifier::empty(),
                            text_style(12.0, if chosen { Color::WHITE } else { INK }, false),
                        );
                    },
                );
            }
        },
    );
}
