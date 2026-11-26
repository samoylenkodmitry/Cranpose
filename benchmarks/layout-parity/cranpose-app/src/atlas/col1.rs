use cranpose::prelude::*;
use cranpose_ui_graphics::{Color, Size};
use cranpose_ui_layout::{Alignment, HorizontalAlignment, LinearArrangement, VerticalAlignment};

use super::common::{
    ACCENTS, AMBER, AMBER_SOFT, BG_SUBTLE, BG_TRACK, BLUE, BLUE_SOFT, BORDER_CARD, BORDER_STRONG,
    CYAN, EMERALD, EMERALD_SOFT, INDIGO, INDIGO_SOFT, INK, MUTED, RED, RED_SOFT, ROSE, SLATE, TEAL,
    TEAL_SOFT, VIOLET, VIOLET_SOFT, ZoneCard, atlas_text_style, rect_shape,
};

#[composable]
pub(super) fn AtlasCol1(modifier: Modifier) {
    Column(
        modifier.fill_max_height(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
        || {
            Zone1ModifierChains(Modifier::empty().fill_max_width().columnWeight(1.12, true));
            Zone2BoxAlignment(Modifier::empty().fill_max_width().columnWeight(0.92, true));
            Zone3Arrangements(Modifier::empty().fill_max_width().columnWeight(1.10, true));
            Zone4Weights(Modifier::empty().fill_max_width().columnWeight(0.86, true));
        },
    );
}

#[composable]
fn Zone1ModifierChains(modifier: Modifier) {
    ZoneCard(
        "z1_card",
        "1. Modifier Chain · Coercion · requiredSize · wrapContent",
        BLUE,
        modifier,
        || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                || {
                    Zone1RowA();
                    Zone1RowB();
                    Zone1RowC();
                    Zone1RowD();
                },
            );
        },
    );
}

#[composable]
fn Zone1RowA() {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(8.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        || {
            Box(
                Modifier::empty()
                    .test_tag("z1_chain_box")
                    .padding(4.0)
                    .background(BLUE_SOFT)
                    .border(1.0, BLUE, rect_shape())
                    .padding(5.0)
                    .background(AMBER_SOFT)
                    .border(1.0, AMBER, rect_shape())
                    .padding(4.0)
                    .background(EMERALD_SOFT)
                    .size_points(64.0, 22.0),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "Chain",
                        Modifier::empty().test_tag("z1_chain_inner"),
                        atlas_text_style(8.5, INK, true),
                    );
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z1_outer_wins")
                    .size_points(74.0, 28.0)
                    .size_points(36.0, 14.0)
                    .background(INDIGO_SOFT)
                    .border(1.0, INDIGO, rect_shape()),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "Outer74",
                        Modifier::empty(),
                        atlas_text_style(8.0, INDIGO, true),
                    );
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z1_req_parent")
                    .size_points(46.0, 22.0)
                    .background(RED_SOFT)
                    .border(1.0, RED, rect_shape()),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Box(
                        Modifier::empty()
                            .test_tag("z1_req_child")
                            .required_size(Size::new(62.0, 30.0))
                            .background(Color::from_rgba_u8(0xDC, 0x26, 0x26, 0x66))
                            .border(1.0, RED, rect_shape()),
                        BoxSpec::new().content_alignment(Alignment::CENTER),
                        || {
                            Text("Req62", Modifier::empty(), atlas_text_style(7.5, INK, true));
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn Zone1RowB() {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(8.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        || {
            Box(
                Modifier::empty()
                    .test_tag("z1_sizein_min")
                    .size_in(Size::new(58.0, 22.0), Size::new(90.0, 32.0))
                    .background(VIOLET_SOFT)
                    .border(1.0, VIOLET, rect_shape()),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Box(
                        Modifier::empty()
                            .test_tag("z1_sizein_min_c")
                            .size_points(16.0, 10.0)
                            .background(VIOLET),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z1_sizein_max")
                    .size_in(Size::new(20.0, 14.0), Size::new(52.0, 24.0))
                    .background(TEAL_SOFT)
                    .border(1.0, TEAL, rect_shape()),
                BoxSpec::new().content_alignment(Alignment::TOP_START),
                || {
                    Box(
                        Modifier::empty()
                            .test_tag("z1_sizein_max_c")
                            .size_points(90.0, 60.0)
                            .background(Color::from_rgba_u8(0x0D, 0x94, 0x88, 0x88)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z1_frac_parent")
                    .height(34.0)
                    .background(BG_SUBTLE)
                    .border(1.0, BORDER_STRONG, rect_shape()),
                BoxSpec::default(),
                || {
                    Box(
                        Modifier::empty()
                            .align(Alignment::CENTER_END)
                            .test_tag("z1_frac_child")
                            .fill_max_width_fraction(0.37)
                            .fill_max_height_fraction(0.65)
                            .background(CYAN),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

#[derive(Clone, Copy, PartialEq)]
struct WrapProbe {
    outer_tag: &'static str,
    child_tag: &'static str,
    start_pad: f32,
    clip: bool,
    fill: Color,
    stroke: Color,
}

#[composable]
fn UnboundedWrapBox(probe: WrapProbe) {
    let mut outer = Modifier::empty().test_tag(probe.outer_tag);
    if probe.start_pad > 0.0 {
        outer = outer.padding_relative(probe.start_pad, 0.0, 0.0, 0.0);
    }
    outer = outer
        .size_points(58.0, 28.0)
        .background(BG_TRACK)
        .border(1.0, SLATE, rect_shape());
    if probe.clip {
        outer = outer.clip_to_bounds();
    }
    Box(outer, BoxSpec::default(), move || {
        Box(
            Modifier::empty()
                .fill_max_size()
                .wrap_content_size(Alignment::BOTTOM_END, true),
            BoxSpec::default(),
            move || {
                Box(
                    Modifier::empty()
                        .test_tag(probe.child_tag)
                        .size_points(72.0, 36.0)
                        .background(probe.fill)
                        .border(1.0, probe.stroke, rect_shape()),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    });
}

#[composable]
fn Zone1RowC() {
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding_each(0.0, 6.0, 0.0, 0.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(26.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        || {
            UnboundedWrapBox(WrapProbe {
                outer_tag: "z1_wrap_unclipped",
                child_tag: "z1_wrap_child_a",
                start_pad: 16.0,
                clip: false,
                fill: Color::from_rgba_u8(0x7C, 0x3A, 0xED, 0x66),
                stroke: VIOLET,
            });
            UnboundedWrapBox(WrapProbe {
                outer_tag: "z1_wrap_clipped",
                child_tag: "z1_wrap_child_b",
                start_pad: 0.0,
                clip: true,
                fill: Color::from_rgba_u8(0x05, 0x96, 0x69, 0x66),
                stroke: EMERALD,
            });
            Text(
                "unbounded:\nunclip vs clip",
                Modifier::empty(),
                atlas_text_style(8.0, MUTED, false),
            );
        },
    );
}

#[composable]
fn Zone1RowD() {
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding_each(0.0, 4.0, 0.0, 0.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(16.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        || {
            Box(
                Modifier::empty()
                    .test_tag("z1_chain_fill_p")
                    .size_points(96.0, 20.0)
                    .background(BG_SUBTLE)
                    .border(1.0, BORDER_STRONG, rect_shape()),
                BoxSpec::default(),
                || {
                    Box(
                        Modifier::empty()
                            .test_tag("z1_chain_fill")
                            .fill_max_width_fraction(0.5)
                            .fill_max_width_fraction(0.5)
                            .fill_max_height()
                            .background(BLUE),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z1_req_w_p")
                    .size_points(46.0, 20.0)
                    .background(AMBER_SOFT)
                    .border(1.0, AMBER, rect_shape()),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Box(
                        Modifier::empty()
                            .test_tag("z1_req_w_only")
                            .required_width(72.0)
                            .fill_max_height()
                            .background(Color::from_rgba_u8(0xD9, 0x77, 0x06, 0x66))
                            .border(1.0, AMBER, rect_shape()),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z1_wh_in")
                    .width_in(58.0, f32::INFINITY)
                    .height_in(0.0, 18.0)
                    .background(EMERALD_SOFT)
                    .border(1.0, EMERALD, rect_shape()),
                BoxSpec::default(),
                || {
                    Box(
                        Modifier::empty()
                            .test_tag("z1_wh_in_c")
                            .size_points(24.0, 38.0)
                            .background(EMERALD),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

#[derive(Clone, Copy, PartialEq)]
struct PropBoxSpec {
    outer_tag: &'static str,
    child_tag: &'static str,
    propagate: bool,
    bg: Color,
    stroke: Color,
    child_bg: Color,
}

#[composable]
fn PropMinBox(spec: PropBoxSpec) {
    Box(
        Modifier::empty()
            .test_tag(spec.outer_tag)
            .size_in(
                Size::new(68.0, 28.0),
                Size::new(f32::INFINITY, f32::INFINITY),
            )
            .background(spec.bg)
            .border(1.0, spec.stroke, rect_shape()),
        BoxSpec::new()
            .content_alignment(Alignment::CENTER)
            .propagate_min_constraints(spec.propagate),
        move || {
            Box(
                Modifier::empty()
                    .test_tag(spec.child_tag)
                    .size_points(20.0, 12.0)
                    .background(spec.child_bg),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[composable]
fn Zone2ZStack(modifier: Modifier) {
    let layers: [(&str, &str, f32, f32, f32, Color); 4] = [
        ("z2_z_2", "z=2", 2.0, 4.0, 4.0, RED),
        ("z2_z_neg1", "z=-1", -1.0, 24.0, 12.0, BLUE),
        ("z2_z_05a", "z=.5a", 0.5, 44.0, 8.0, EMERALD),
        ("z2_z_05b", "z=.5b", 0.5, 64.0, 20.0, AMBER),
    ];
    Box(
        modifier
            .test_tag("z2_zstack")
            .background(BG_SUBTLE)
            .border(1.0, BORDER_STRONG, rect_shape())
            .padding(4.0),
        BoxSpec::default(),
        move || {
            for (tag, label, z, x, y, bg) in layers {
                Box(
                    Modifier::empty()
                        .z_index(z)
                        .offset(x, y)
                        .test_tag(tag)
                        .size_points(50.0, 28.0)
                        .background(bg)
                        .border(1.0, Color::WHITE, rect_shape()),
                    BoxSpec::new().content_alignment(Alignment::CENTER),
                    move || {
                        Text(
                            label,
                            Modifier::empty(),
                            atlas_text_style(8.0, Color::WHITE, true),
                        );
                    },
                );
            }
        },
    );
}

#[composable]
fn Zone2BoxAlignment(modifier: Modifier) {
    ZoneCard(
        "z2_card",
        "2. Box 9-Align · propagateMinConstraints · zIndex",
        INDIGO,
        modifier,
        || {
            Row(
                Modifier::empty().fill_max_size(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(8.0)),
                || {
                    Box(
                        Modifier::empty()
                            .rowWeight(1.12, true)
                            .test_tag("z2_align9")
                            .fill_max_height()
                            .background(BG_SUBTLE)
                            .border(1.0, BORDER_STRONG, rect_shape()),
                        BoxSpec::default(),
                        || {
                            let points: [(&str, Alignment, f32, f32); 9] = [
                                ("z2_a_ts", Alignment::TOP_START, 19.0, 13.0),
                                ("z2_a_tc", Alignment::TOP_CENTER, 23.0, 11.0),
                                ("z2_a_te", Alignment::TOP_END, 17.0, 15.0),
                                ("z2_a_cs", Alignment::CENTER_START, 21.0, 13.0),
                                ("z2_a_cc", Alignment::CENTER, 29.0, 19.0),
                                ("z2_a_ce", Alignment::CENTER_END, 15.0, 17.0),
                                ("z2_a_bs", Alignment::BOTTOM_START, 25.0, 11.0),
                                ("z2_a_bc", Alignment::BOTTOM_CENTER, 19.0, 15.0),
                                ("z2_a_be", Alignment::BOTTOM_END, 23.0, 13.0),
                            ];
                            for (idx, (tag, align, w, h)) in points.into_iter().enumerate() {
                                Box(
                                    Modifier::empty()
                                        .align(align)
                                        .test_tag(tag)
                                        .size_points(w, h)
                                        .background(ACCENTS[idx % ACCENTS.len()]),
                                    BoxSpec::default(),
                                    || {},
                                );
                            }
                        },
                    );
                    Column(
                        Modifier::empty().rowWeight(0.95, true).fill_max_height(),
                        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                        || {
                            Row(
                                Modifier::empty().fill_max_width(),
                                RowSpec::new()
                                    .horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
                                || {
                                    PropMinBox(PropBoxSpec {
                                        outer_tag: "z2_prop_false",
                                        child_tag: "z2_prop_false_c",
                                        propagate: false,
                                        bg: AMBER_SOFT,
                                        stroke: AMBER,
                                        child_bg: AMBER,
                                    });
                                    PropMinBox(PropBoxSpec {
                                        outer_tag: "z2_prop_true",
                                        child_tag: "z2_prop_true_c",
                                        propagate: true,
                                        bg: EMERALD_SOFT,
                                        stroke: EMERALD,
                                        child_bg: Color::from_rgba_u8(0x05, 0x96, 0x69, 0x99),
                                    });
                                },
                            );
                            Zone2ZStack(Modifier::empty().fill_max_width().columnWeight(1.0, true));
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn Zone3Arrangements(modifier: Modifier) {
    ZoneCard(
        "z3_card",
        "3. All Arrangements · SpacedByAligned · Overflow Clamp",
        CYAN,
        modifier,
        || {
            Row(
                Modifier::empty().fill_max_size(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
                || {
                    Zone3RowColumn();
                    Column(
                        Modifier::empty()
                            .rowWeight(0.60, true)
                            .test_tag("z3_col")
                            .fill_max_height()
                            .background(BG_SUBTLE)
                            .border(1.0, BORDER_STRONG, rect_shape())
                            .padding_horizontal(3.0),
                        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpaceEvenly),
                        || {
                            let aligns = [
                                HorizontalAlignment::Start,
                                HorizontalAlignment::CenterHorizontally,
                                HorizontalAlignment::End,
                                HorizontalAlignment::CenterHorizontally,
                            ];
                            for (i, align) in aligns.into_iter().enumerate() {
                                Box(
                                    Modifier::empty()
                                        .alignInColumn(align)
                                        .test_tag(format!("z3_col_{i}"))
                                        .size_points(24.0 + i as f32 * 5.0, 14.0)
                                        .background(ACCENTS[i + 2]),
                                    BoxSpec::default(),
                                    || {},
                                );
                            }
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn Zone3RowColumn() {
    Column(
        Modifier::empty().rowWeight(1.75, true).fill_max_height(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
        || {
            let specs: [(&str, LinearArrangement); 7] = [
                ("start", LinearArrangement::Start),
                ("center", LinearArrangement::Center),
                ("end", LinearArrangement::End),
                ("between", LinearArrangement::SpaceBetween),
                ("around", LinearArrangement::SpaceAround),
                ("evenly", LinearArrangement::SpaceEvenly),
                (
                    "sp_end",
                    LinearArrangement::spaced_by_aligned(7.0, HorizontalAlignment::End),
                ),
            ];
            let widths = [26.0, 38.0, 20.0];
            let colors = [BLUE, EMERALD, AMBER];
            for (name, arr) in specs {
                Row(
                    Modifier::empty()
                        .test_tag(format!("z3_{name}"))
                        .fill_max_width()
                        .height(17.0)
                        .background(BG_TRACK)
                        .padding_vertical(2.0),
                    RowSpec::new()
                        .horizontal_arrangement(arr)
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    move || {
                        for (i, w) in widths.into_iter().enumerate() {
                            Box(
                                Modifier::empty()
                                    .test_tag(format!("z3_{name}_{i}"))
                                    .size_points(w, 13.0)
                                    .background(colors[i]),
                                BoxSpec::default(),
                                || {},
                            );
                        }
                    },
                );
            }
            Row(
                Modifier::empty()
                    .test_tag("z3_overflow")
                    .fill_max_width()
                    .height(19.0)
                    .background(RED_SOFT)
                    .border(1.0, RED, rect_shape())
                    .clip_to_bounds()
                    .padding_vertical(2.0),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::spaced_by(12.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                || {
                    for (i, &color) in ACCENTS.iter().enumerate().take(5) {
                        Box(
                            Modifier::empty()
                                .test_tag(format!("z3_ov_{i}"))
                                .size_points(54.0, 13.0)
                                .background(color),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                },
            );
        },
    );
}

#[composable]
fn Zone4Weights(modifier: Modifier) {
    ZoneCard(
        "z4_card",
        "4. Weights (fill=true/false) · Mixed Fixed · Tight Remainder",
        TEAL,
        modifier,
        || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(5.0)),
                || {
                    Zone4StripA();
                    Zone4StripB();
                    Zone4StripC();
                    Zone4StripD();
                },
            );
        },
    );
}

#[composable]
fn Zone4StripA() {
    Row(
        Modifier::empty()
            .test_tag("z4_a")
            .fill_max_width()
            .height(24.0)
            .background(BG_TRACK)
            .padding(2.0),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(5.0)),
        || {
            Box(
                Modifier::empty()
                    .test_tag("z4_a_f1")
                    .width(34.0)
                    .fill_max_height()
                    .background(SLATE),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z4_a_w1")
                    .fill_max_height()
                    .background(BLUE),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .rowWeight(2.5, true)
                    .test_tag("z4_a_w2")
                    .fill_max_height()
                    .background(EMERALD),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .test_tag("z4_a_f2")
                    .width(26.0)
                    .fill_max_height()
                    .background(SLATE),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[composable]
fn Zone4StripB() {
    Row(
        Modifier::empty()
            .test_tag("z4_b")
            .fill_max_width()
            .height(24.0)
            .background(BG_TRACK)
            .padding(2.0),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpaceBetween),
        || {
            Box(
                Modifier::empty()
                    .rowWeight(1.0, false)
                    .test_tag("z4_b_nf1")
                    .size_points(24.0, 20.0)
                    .background(AMBER),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z4_b_f")
                    .fill_max_height()
                    .background(VIOLET),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .rowWeight(2.0, false)
                    .test_tag("z4_b_nf2")
                    .size_points(34.0, 20.0)
                    .background(ROSE),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[composable]
fn Zone4StripC() {
    Row(
        Modifier::empty()
            .test_tag("z4_c")
            .fill_max_width()
            .height(22.0)
            .background(BG_TRACK)
            .padding(2.0),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
        || {
            Box(
                Modifier::empty()
                    .test_tag("z4_c_f1")
                    .width(110.0)
                    .fill_max_height()
                    .background(INDIGO_SOFT)
                    .border(1.0, INDIGO, rect_shape()),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .test_tag("z4_c_f2")
                    .width(105.0)
                    .fill_max_height()
                    .background(INDIGO_SOFT)
                    .border(1.0, INDIGO, rect_shape()),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z4_c_w1")
                    .fill_max_height()
                    .background(RED),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .rowWeight(3.0, true)
                    .test_tag("z4_c_w3")
                    .fill_max_height()
                    .background(CYAN),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .test_tag("z4_c_f3")
                    .width(64.0)
                    .fill_max_height()
                    .background(INDIGO_SOFT)
                    .border(1.0, INDIGO, rect_shape()),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[composable]
fn Zone4StripD() {
    Row(
        Modifier::empty()
            .test_tag("z4_d")
            .fill_max_width()
            .columnWeight(1.0, true)
            .background(BG_SUBTLE)
            .border(1.0, BORDER_CARD, rect_shape())
            .padding(3.0),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
        || {
            Box(
                Modifier::empty()
                    .test_tag("z4_d_lead")
                    .width(42.0)
                    .fill_max_height()
                    .background(TEAL),
                BoxSpec::default(),
                || {},
            );
            Spacer(Modifier::empty().rowWeight(1.0, true));
            Column(
                Modifier::empty()
                    .rowWeight(1.5, true)
                    .test_tag("z4_d_col")
                    .fill_max_height(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(3.0)),
                || {
                    Box(
                        Modifier::empty()
                            .columnWeight(1.0, true)
                            .test_tag("z4_d_top")
                            .fill_max_width()
                            .background(BLUE),
                        BoxSpec::default(),
                        || {},
                    );
                    Box(
                        Modifier::empty()
                            .columnWeight(2.0, true)
                            .test_tag("z4_d_bot")
                            .fill_max_width()
                            .background(EMERALD),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}
