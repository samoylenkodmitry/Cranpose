//! The tabs that show the screens the framework comparison measures every
//! night (`benchmarks/compose-vs-cranpose`), at the load tier the chips choose.

use std::rc::Rc;

use cranpose_core::{key, rememberMutableStateOf, MutableState};
use cranpose_ui::{
    composable, text::FontWeight, Button, ButtonSpec, Color, Column, ColumnSpec, LinearArrangement,
    Modifier, Row, RowSpec, Text,
};
use perf_compare::{GauntletLoad, GauntletScreen, WallLoad, WallScreen};

use super::{demo_text::text_style, demo_theme::DemoPalette};

const CHOSEN: Color = Color::from_rgb_u8(0x25, 0x63, 0xEB);
const CHIP: Color = Color::from_rgb_u8(0xE2, 0xE8, 0xF0);
const INK: Color = Color::from_rgb_u8(0x11, 0x18, 0x27);

// A screen of the comparison.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum BenchScreen {
    Gauntlet,
    StillWall,
}

impl BenchScreen {
    // The tiers the comparison defines.
    fn tiers(self) -> usize {
        match self {
            Self::Gauntlet => 16,
            Self::StillWall => 20,
        }
    }

    // The tier the tab opens at: the phones' gauntlet tier and the desktop
    // apps' wall tier of the nightly.
    fn first_tier(self) -> usize {
        match self {
            Self::Gauntlet => 12,
            Self::StillWall => 20,
        }
    }
}

// `screen` under chips for its tiers.
#[composable]
pub(super) fn BenchTab(screen: BenchScreen) {
    let tier = rememberMutableStateOf(|| screen.first_tier());
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            TierChips(tier, screen.tiers());
            let chosen = tier.get();
            // A new tier starts the screen over, as a launch at that tier does.
            key(chosen, move || match screen {
                BenchScreen::Gauntlet => GauntletScreen(GauntletLoad {
                    tier: chosen,
                    freeze: 0,
                }),
                BenchScreen::StillWall => WallScreen(WallLoad {
                    tier: chosen,
                    freeze: 0,
                }),
            });
        },
    );
}

// Chips for tiers 1 to `tiers`; a tap on one sets `tier`.
#[composable]
fn TierChips(tier: MutableState<usize>, tiers: usize) {
    let label = DemoPalette::current().text;
    let choose: Rc<dyn Fn(usize)> = Rc::new(move |chosen| tier.set(chosen));
    Row(
        Modifier::empty().fill_max_width().padding(8.0),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(4.0)),
        move || {
            Text(
                "Tier",
                Modifier::empty().padding_symmetric(4.0, 4.0),
                text_style(13.0, label, Some(FontWeight::BOLD)),
            );
            for chip in 1..=tiers {
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
                            text_style(12.0, if chosen { Color::WHITE } else { INK }, None),
                        );
                    },
                );
            }
        },
    );
}
