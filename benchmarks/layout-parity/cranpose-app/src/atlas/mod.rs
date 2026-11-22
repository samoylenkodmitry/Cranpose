mod col1;
mod col2;
mod col3;
mod col4;
mod col5;
mod common;

use cranpose::prelude::*;
use cranpose_ui_graphics::{Color, RoundedCornerShape};
use cranpose_ui_layout::{LinearArrangement, VerticalAlignment};

use self::common::{BG_CANVAS, FOOTER_BG, HEADER_BG, atlas_text_style};

#[derive(Clone, PartialEq)]
pub struct AtlasConfig {
    pub width_dp: u32,
    pub height_dp: u32,
    pub density_label: String,
}

#[composable]
pub fn ParityAtlasScreen(config: AtlasConfig) {
    Column(
        Modifier::empty()
            .test_tag("root")
            .fill_max_size()
            .background(BG_CANVAS),
        ColumnSpec::default(),
        move || {
            HeaderBar(config.clone());
            Row(
                Modifier::empty()
                    .test_tag("main_body")
                    .fill_max_width()
                    .columnWeight(1.0, true)
                    .padding(8.0),
                RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(8.0)),
                || {
                    col1::AtlasCol1(Modifier::empty().rowWeight(1.06, true).test_tag("col_1"));
                    col2::AtlasCol2(Modifier::empty().rowWeight(0.99, true).test_tag("col_2"));
                    col3::AtlasCol3(Modifier::empty().rowWeight(1.00, true).test_tag("col_3"));
                    col4::AtlasCol4(Modifier::empty().rowWeight(0.93, true).test_tag("col_4"));
                    col5::AtlasCol5(Modifier::empty().rowWeight(1.02, true).test_tag("col_5"));
                },
            );
            FooterBar();
        },
    );
}

#[composable]
fn HeaderBar(config: AtlasConfig) {
    let pill_text = format!(
        "{}x{} @ {}x",
        config.width_dp, config.height_dp, config.density_label
    );
    Row(
        Modifier::empty()
            .test_tag("header_bar")
            .fill_max_width()
            .height(38.0)
            .background(HEADER_BG)
            .padding_symmetric(12.0, 4.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(10.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let pill_text = pill_text.clone();
            Row(
                Modifier::empty(),
                RowSpec::new().vertical_alignment(VerticalAlignment::Bottom),
                || {
                    Text(
                        "LAYOUT PARITY ATLAS",
                        Modifier::empty().align_by_baseline().test_tag("hdr_title"),
                        atlas_text_style(15.0, Color::from_rgb_u8(0xF8, 0xFA, 0xFC), true),
                    );
                    Text(
                        "Compose vs Cranpose",
                        Modifier::empty()
                            .align_by_baseline()
                            .padding_relative(8.0, 0.0, 0.0, 0.0)
                            .test_tag("hdr_sub"),
                        atlas_text_style(10.5, Color::from_rgb_u8(0x94, 0xA3, 0xB8), false),
                    );
                    Text(
                        "20 ZONES · 80+ MECHANICS & WRAPPED COMBOS",
                        Modifier::empty()
                            .align_by_baseline()
                            .padding_relative(8.0, 0.0, 0.0, 0.0)
                            .test_tag("hdr_meta"),
                        atlas_text_style(9.0, Color::from_rgb_u8(0x38, 0xBD, 0xF8), true),
                    );
                },
            );
            Spacer(Modifier::empty().rowWeight(1.0, true));
            Box(
                Modifier::empty()
                    .test_tag("hdr_pill")
                    .background(Color::from_rgb_u8(0x1E, 0x29, 0x3B))
                    .rounded_corners(10.0)
                    .border(
                        1.0,
                        Color::from_rgb_u8(0x33, 0x41, 0x55),
                        RoundedCornerShape::uniform(10.0),
                    )
                    .padding_symmetric(10.0, 3.0),
                BoxSpec::default(),
                move || {
                    Text(
                        pill_text.clone(),
                        Modifier::empty(),
                        atlas_text_style(9.5, Color::from_rgb_u8(0xE2, 0xE8, 0xF0), true),
                    );
                },
            );
        },
    );
}

#[composable]
fn FooterBar() {
    Row(
        Modifier::empty()
            .test_tag("footer_bar")
            .fill_max_width()
            .height(26.0)
            .background(FOOTER_BG)
            .padding_symmetric(12.0, 4.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpaceBetween)
            .vertical_alignment(VerticalAlignment::CenterVertically),
        || {
            Text(
                "Constraints · Intrinsics · Baselines · Custom Layout · Subcompose · RTL/LTR · Flow · Offsets · GraphicsLayer · Draw · Scroll · StickyHeader · Grids · Pager",
                Modifier::empty().test_tag("ftr_left"),
                atlas_text_style(8.5, Color::from_rgb_u8(0xCB, 0xD5, 0xE1), false),
            );
            Text(
                "Deterministic Layout Parity Suite",
                Modifier::empty().test_tag("ftr_right"),
                atlas_text_style(8.5, Color::from_rgb_u8(0x38, 0xBD, 0xF8), true),
            );
        },
    );
}
