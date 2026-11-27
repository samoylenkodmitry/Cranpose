use cranpose::prelude::*;
use cranpose_core::SlotId;
use cranpose_ui::{
    AlignmentLines, BasicText, Layout, LayoutModifierMeasureResult, MeasureResult,
    SubcomposeLayout, SubcomposeLayoutScope, SubcomposeMeasureScope,
    text::{AnnotatedString, FontWeight, SpanStyle, TextAlign, TextOverflow, TextUnit},
    widgets::{BoxWithConstraints, BoxWithConstraintsScope},
};
use cranpose_ui_graphics::{Brush, Color, Point, Size, Stroke};
use cranpose_ui_layout::{
    Alignment, Constraints, IntrinsicSize, LinearArrangement, Measurables, MeasurePolicy,
    MeasureScope, Placement, VerticalAlignment,
};

use super::common::{
    ACCENTS, AMBER, AMBER_SOFT, BG_CARD, BG_SUBTLE, BG_TRACK, BLUE, BLUE_SOFT, BORDER_STRONG, CYAN,
    CYAN_SOFT, ChipSpec, EMERALD, EMERALD_SOFT, INDIGO, INK, LabeledChip, MUTED, RED, RED_SOFT,
    ROSE, ROSE_SOFT, TEAL, VIOLET, VIOLET_SOFT, ZoneCard, atlas_text_style, rect_shape,
};

#[composable]
pub(super) fn AtlasCol2(modifier: Modifier) {
    Column(
        modifier.fill_max_height(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
        || {
            Zone5Intrinsics(Modifier::empty().fill_max_width().columnWeight(1.04, true));
            Zone6BaselinesAndText(Modifier::empty().fill_max_width().columnWeight(1.02, true));
            Zone7CustomLayoutAndBwc(Modifier::empty().fill_max_width().columnWeight(1.00, true));
            Zone8SubcomposeLayout(Modifier::empty().fill_max_width().columnWeight(0.94, true));
        },
    );
}

#[composable]
fn Zone5Intrinsics(modifier: Modifier) {
    ZoneCard(
        "z5_card",
        "5. Intrinsics (Min/Max Height & Width · Nested)",
        VIOLET,
        modifier,
        || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                || {
                    Zone5HeightMinRow();
                    Zone5WidthRow();
                    Zone5HeightMaxRow();
                },
            );
        },
    );
}

#[composable]
fn Zone5HeightMinRow() {
    Row(
        Modifier::empty()
            .test_tag("z5_imin_row")
            .fill_max_width()
            .height_intrinsic(IntrinsicSize::Min)
            .background(BG_SUBTLE)
            .border(1.0, BORDER_STRONG, rect_shape())
            .padding(4.0),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
        || {
            Text(
                "IntrinsicSize.Min stretches the red divider to match wrapped text height.",
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z5_imin_left"),
                atlas_text_style(8.5, INK, false),
            );
            Box(
                Modifier::empty()
                    .test_tag("z5_imin_div")
                    .width(3.0)
                    .fill_max_height()
                    .background(RED),
                BoxSpec::default(),
                || {},
            );
            Text(
                "Centered",
                Modifier::empty()
                    .rowWeight(0.55, true)
                    .alignInRow(VerticalAlignment::CenterVertically)
                    .test_tag("z5_imin_right"),
                atlas_text_style(8.5, VIOLET, true),
            );
        },
    );
}

#[composable]
fn Zone5WidthRow() {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(8.0)),
        || {
            Column(
                Modifier::empty()
                    .test_tag("z5_wmax_col")
                    .width_intrinsic(IntrinsicSize::Max)
                    .background(BLUE_SOFT)
                    .border(1.0, BLUE, rect_shape())
                    .padding(4.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(2.0)),
                || {
                    Text("Hi", Modifier::empty(), atlas_text_style(8.5, INK, false));
                    Text(
                        "Widest label here",
                        Modifier::empty(),
                        atlas_text_style(8.5, BLUE, true),
                    );
                    Box(
                        Modifier::empty()
                            .test_tag("z5_wmax_bar")
                            .fill_max_width()
                            .height(5.0)
                            .background(BLUE),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            Column(
                Modifier::empty()
                    .test_tag("z5_wmin_col")
                    .width_intrinsic(IntrinsicSize::Min)
                    .background(AMBER_SOFT)
                    .border(1.0, AMBER, rect_shape())
                    .padding(4.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(2.0)),
                || {
                    Text(
                        "Min wraps words",
                        Modifier::empty().test_tag("z5_wmin_txt"),
                        atlas_text_style(8.5, INK, false),
                    );
                    Box(
                        Modifier::empty()
                            .test_tag("z5_wmin_bar")
                            .fill_max_width()
                            .height(5.0)
                            .background(AMBER),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

#[composable]
fn Zone5HeightMaxRow() {
    Row(
        Modifier::empty()
            .test_tag("z5_hmax_row")
            .fill_max_width()
            .height_intrinsic(IntrinsicSize::Max)
            .background(EMERALD_SOFT)
            .border(1.0, EMERALD, rect_shape())
            .padding(4.0),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
        || {
            Column(
                Modifier::empty(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(3.0)),
                || {
                    Box(
                        Modifier::empty()
                            .test_tag("z5_hmax_p1")
                            .size_points(56.0, 12.0)
                            .background(EMERALD),
                        BoxSpec::default(),
                        || {},
                    );
                    Box(
                        Modifier::empty()
                            .test_tag("z5_hmax_p2")
                            .size_points(42.0, 15.0)
                            .background(TEAL),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z5_hmax_bar")
                    .width(6.0)
                    .fill_max_height()
                    .background(INK),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z5_hmax_fill")
                    .fill_max_height()
                    .background(Color::from_rgba_u8(0x05, 0x96, 0x69, 0x55)),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[derive(Clone, Copy, PartialEq)]
struct CustomBaselinePolicy;

impl MeasurePolicy for CustomBaselinePolicy {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        _measurables: Measurables<'_>,
        constraints: Constraints,
    ) -> MeasureResult {
        let (w, h) = constraints.constrain(28.0, 24.0);
        MeasureResult::new(Size::new(w, h), Vec::new())
            .with_alignment_lines(AlignmentLines::new(Some(18.0), Some(18.0)))
    }
    fn min_intrinsic_width(&self, _: Measurables<'_>, _: f32) -> f32 {
        28.0
    }
    fn max_intrinsic_width(&self, _: Measurables<'_>, _: f32) -> f32 {
        28.0
    }
    fn min_intrinsic_height(&self, _: Measurables<'_>, _: f32) -> f32 {
        24.0
    }
    fn max_intrinsic_height(&self, _: Measurables<'_>, _: f32) -> f32 {
        24.0
    }
}

#[composable]
fn Zone6BaselinesAndText(modifier: Modifier) {
    ZoneCard(
        "z6_card",
        "6. alignByBaseline · Custom Line · Text Wrapping/Ellipsis",
        ROSE,
        modifier,
        || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                || {
                    Zone6BaselineRow();
                    Zone6OverflowRow();
                    Zone6AnnotatedAndAlign();
                },
            );
        },
    );
}

#[composable]
fn Zone6BaselineRow() {
    Row(
        Modifier::empty()
            .test_tag("z6_base_row")
            .fill_max_width()
            .background(BG_SUBTLE)
            .border(1.0, BORDER_STRONG, rect_shape())
            .padding(4.0),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
        || {
            Text(
                "Ag",
                Modifier::empty()
                    .align_by_baseline()
                    .test_tag("z6_base_big"),
                atlas_text_style(22.0, INK, true),
            );
            Text(
                "base",
                Modifier::empty()
                    .align_by_baseline()
                    .test_tag("z6_base_small"),
                atlas_text_style(9.5, MUTED, false),
            );
            Text(
                "42",
                Modifier::empty()
                    .align_by_baseline()
                    .test_tag("z6_base_med"),
                atlas_text_style(14.0, BLUE, true),
            );
            Box(
                Modifier::empty()
                    .test_tag("z6_base_nobase")
                    .size_points(14.0, 14.0)
                    .background(AMBER),
                BoxSpec::default(),
                || {},
            );
            Text(
                "Wrap\nline2",
                Modifier::empty()
                    .width(36.0)
                    .align_by_baseline()
                    .test_tag("z6_base_wrap"),
                atlas_text_style(9.0, EMERALD, false),
            );
            Layout(
                Modifier::empty()
                    .align_by_baseline()
                    .test_tag("z6_base_custom")
                    .size_points(28.0, 24.0)
                    .background(RED_SOFT)
                    .draw_behind(|scope| {
                        let sz = scope.size();
                        scope.draw_line(
                            Brush::solid(RED),
                            Point::new(0.0, 18.0),
                            Point::new(sz.width, 18.0),
                            Stroke::new(1.5),
                        );
                    }),
                CustomBaselinePolicy,
                || {},
            );
        },
    );
}

#[composable]
fn Zone6OverflowRow() {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(5.0)),
        || {
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z6_txt_ellipsis_box")
                    .background(BLUE_SOFT)
                    .padding(3.0),
                BoxSpec::default(),
                || {
                    BasicText(
                        "Ellipsis clamps this longer multi-line paragraph cleanly at line two with dots.",
                        Modifier::empty().test_tag("z6_txt_ellipsis"),
                        atlas_text_style(8.5, INK, false),
                        TextOverflow::Ellipsis,
                        true,
                        2,
                        1,
                    );
                },
            );
            Box(
                Modifier::empty()
                    .rowWeight(0.8, true)
                    .test_tag("z6_txt_minlines_box")
                    .background(AMBER_SOFT)
                    .padding(3.0),
                BoxSpec::default(),
                || {
                    BasicText(
                        "Min 3 lines",
                        Modifier::empty().test_tag("z6_txt_minlines"),
                        atlas_text_style(8.5, INK, false),
                        TextOverflow::Clip,
                        true,
                        usize::MAX,
                        3,
                    );
                },
            );
            Box(
                Modifier::empty()
                    .rowWeight(0.9, true)
                    .test_tag("z6_txt_nowrap_box")
                    .background(ROSE_SOFT)
                    .clip_to_bounds()
                    .padding(3.0),
                BoxSpec::default(),
                || {
                    BasicText(
                        "NoWrap clips right past edge",
                        Modifier::empty().test_tag("z6_txt_nowrap"),
                        atlas_text_style(8.5, ROSE, false),
                        TextOverflow::Clip,
                        false,
                        usize::MAX,
                        1,
                    );
                },
            );
        },
    );
}

#[composable]
fn Zone6AnnotatedAndAlign() {
    let rich = AnnotatedString::builder()
        .push_style(SpanStyle {
            color: Some(VIOLET),
            font_size: TextUnit::Sp(12.0),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        })
        .append("Rich ")
        .pop()
        .push_style(SpanStyle {
            color: Some(INK),
            font_size: TextUnit::Sp(9.0),
            ..Default::default()
        })
        .append("annotated spans ")
        .pop()
        .push_style(SpanStyle {
            color: Some(RED),
            font_size: TextUnit::Sp(10.5),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        })
        .append("1.4em")
        .pop()
        .to_annotated_string();
    Text(
        rich,
        Modifier::empty()
            .test_tag("z6_txt_annotated")
            .fill_max_width()
            .background(VIOLET_SOFT)
            .padding_symmetric(4.0, 2.0),
        atlas_text_style(9.5, INK, false),
    );
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(5.0)),
        || {
            let specs = [
                ("z6_ta_start", "Start", INK, false, TextAlign::Start),
                ("z6_ta_center", "Center", BLUE, true, TextAlign::Center),
                ("z6_ta_end", "End", ROSE, true, TextAlign::End),
            ];
            for (tag, label, color, bold, align) in specs {
                let mut style = atlas_text_style(8.5, color, bold);
                style.paragraph_style.text_align = align;
                Text(
                    label,
                    Modifier::empty()
                        .rowWeight(1.0, true)
                        .test_tag(tag)
                        .background(BG_TRACK)
                        .padding_symmetric(4.0, 2.0),
                    style,
                );
            }
        },
    );
}

#[derive(Clone, Copy, PartialEq)]
struct DiagonalCascadePolicy;

impl MeasurePolicy for DiagonalCascadePolicy {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        measurables: Measurables<'_>,
        constraints: Constraints,
    ) -> MeasureResult {
        let loose = Constraints {
            min_width: 0.0,
            max_width: constraints.max_width,
            min_height: 0.0,
            max_height: constraints.max_height,
        };
        let mut max_r = 0.0_f32;
        let mut max_b = 0.0_f32;
        let mut placements = Vec::with_capacity(measurables.len());
        for (i, m) in measurables.iter().enumerate() {
            let p = m.measure(loose);
            let x = i as f32 * 22.0;
            let y = i as f32 * 11.0;
            max_r = max_r.max(x + p.width());
            max_b = max_b.max(y + p.height());
            placements.push(Placement::new(p.node_id(), x, y, 0.0));
        }
        let (w, h) = constraints.constrain(max_r, max_b);
        MeasureResult::new(Size::new(w, h), placements)
    }
    fn min_intrinsic_width(&self, m: Measurables<'_>, _: f32) -> f32 {
        m.len() as f32 * 22.0
    }
    fn max_intrinsic_width(&self, m: Measurables<'_>, _: f32) -> f32 {
        m.len() as f32 * 22.0 + 38.0
    }
    fn min_intrinsic_height(&self, m: Measurables<'_>, _: f32) -> f32 {
        m.len() as f32 * 11.0 + 16.0
    }
    fn max_intrinsic_height(&self, m: Measurables<'_>, _: f32) -> f32 {
        m.len() as f32 * 11.0 + 16.0
    }
}

fn halo_layout_modifier(modifier: Modifier, dx: f32, dy: f32) -> Modifier {
    modifier.layout(move |m, c| {
        let p = m.measure(Constraints {
            min_width: 0.0,
            max_width: c.max_width,
            min_height: 0.0,
            max_height: c.max_height,
        });
        let (w, h) = c.constrain(p.width() + dx * 2.0, p.height() + dy * 2.0);
        LayoutModifierMeasureResult::new(Size::new(w, h), dx, dy)
    })
}

#[composable]
fn Zone7CustomLayoutAndBwc(modifier: Modifier) {
    ZoneCard(
        "z7_card",
        "7. Custom Layout · Modifier.layout · BoxWithConstraints",
        EMERALD,
        modifier,
        || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                || {
                    Zone7TopRow();
                    Zone7BwcArea();
                },
            );
        },
    );
}

#[composable]
fn Zone7TopRow() {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(8.0)),
        || {
            Layout(
                Modifier::empty()
                    .test_tag("z7_custom_layout")
                    .background(BG_SUBTLE)
                    .border(1.0, BORDER_STRONG, rect_shape()),
                DiagonalCascadePolicy,
                || {
                    for (i, &color) in ACCENTS.iter().enumerate().take(4) {
                        LabeledChip(
                            format!("z7_diag_{i}"),
                            format!("D{i}"),
                            ChipSpec {
                                width: 38.0,
                                height: 16.0,
                                bg: color,
                                radius: 0.0,
                                font_size: 8.0,
                            },
                        );
                    }
                },
            );
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z7_mod_layout_outer")
                    .background(CYAN_SOFT)
                    .border(1.0, CYAN, rect_shape())
                    .layout(|measurable, constraints| {
                        let inner_max_w = (constraints.max_width - 24.0).max(0.0);
                        let inner_max_h = if constraints.max_height.is_finite() {
                            (constraints.max_height - 12.0).max(0.0)
                        } else {
                            f32::INFINITY
                        };
                        let p = measurable.measure(Constraints {
                            min_width: 0.0,
                            max_width: inner_max_w,
                            min_height: 0.0,
                            max_height: inner_max_h,
                        });
                        let (w, h) = constraints.constrain(p.width() + 24.0, p.height() + 12.0);
                        LayoutModifierMeasureResult::new(Size::new(w, h), 12.0, 6.0)
                    }),
                BoxSpec::default(),
                || {
                    Row(
                        Modifier::empty()
                            .test_tag("z7_mod_layout_inner")
                            .fill_max_width()
                            .height(26.0)
                            .background(CYAN),
                        RowSpec::new()
                            .horizontal_arrangement(LinearArrangement::Center)
                            .vertical_alignment(VerticalAlignment::CenterVertically),
                        || {
                            Text(
                                "Mod.layout",
                                Modifier::empty(),
                                atlas_text_style(8.5, Color::WHITE, true),
                            );
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn Zone7NestedBwcRow() {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(6.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        || {
            BoxWithConstraints(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z7_bwc_nested")
                    .height(26.0)
                    .background(BG_CARD)
                    .border(1.0, TEAL, rect_shape())
                    .padding(3.0),
                |inner_scope| {
                    let inner_w = inner_scope.max_width().0;
                    Box(
                        Modifier::empty()
                            .test_tag("z7_bwc_nested_bar")
                            .width(inner_w * 0.45)
                            .fill_max_height()
                            .background(INDIGO),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            let chain_mod = halo_layout_modifier(
                halo_layout_modifier(
                    Modifier::empty()
                        .test_tag("z7_mod_chain_outer")
                        .background(AMBER_SOFT)
                        .border(1.0, AMBER, rect_shape()),
                    5.0,
                    2.0,
                ),
                4.0,
                3.0,
            );
            Box(chain_mod, BoxSpec::default(), || {
                LabeledChip(
                    "z7_mod_chain_inner".to_owned(),
                    "2xLayout".to_owned(),
                    ChipSpec {
                        width: 58.0,
                        height: 16.0,
                        bg: AMBER,
                        radius: 0.0,
                        font_size: 7.5,
                    },
                );
            });
        },
    );
}

#[composable]
fn Zone7BwcArea() {
    BoxWithConstraints(
        Modifier::empty()
            .test_tag("z7_bwc")
            .fill_max_width()
            .columnWeight(1.0, true)
            .background(EMERALD_SOFT)
            .border(1.0, EMERALD, rect_shape())
            .padding(4.0),
        |scope| {
            let mw = scope.max_width().0;
            let chips = if mw >= 220.0 { 3 } else { 2 };
            Column(
                Modifier::empty(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
                move || {
                    Box(
                        Modifier::empty()
                            .test_tag("z7_bwc_bar")
                            .width(mw * 0.62)
                            .height(14.0)
                            .background(EMERALD),
                        BoxSpec::new().content_alignment(Alignment::CENTER_START),
                        move || {
                            Text(
                                format!(" 62% of {}dp", mw.round() as i32),
                                Modifier::empty(),
                                atlas_text_style(8.0, Color::WHITE, true),
                            );
                        },
                    );
                    Row(
                        Modifier::empty(),
                        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(5.0)),
                        move || {
                            for i in 0..chips {
                                LabeledChip(
                                    format!("z7_bwc_c{i}"),
                                    format!("BWC {i}"),
                                    ChipSpec {
                                        width: 56.0,
                                        height: 18.0,
                                        bg: TEAL,
                                        radius: 0.0,
                                        font_size: 8.0,
                                    },
                                );
                            }
                        },
                    );
                    Zone7NestedBwcRow();
                },
            );
        },
    );
}

#[composable]
fn Zone8SubcomposeLayout(modifier: Modifier) {
    ZoneCard(
        "z8_card",
        "8. SubcomposeLayout (Two-Pass Dependent & Dynamic Slots)",
        AMBER,
        modifier,
        || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                || {
                    Zone8TwoPassSubcompose();
                    Zone8DynamicSlotSubcompose();
                },
            );
        },
    );
}

#[composable]
fn Zone8TrackBoxes(anchor_w: f32, anchor_h: f32, rem_w: f32, half_w: f32, rest_w: f32) {
    Box(
        Modifier::empty()
            .test_tag("z8_sub_under")
            .size_points(anchor_w, 6.0)
            .background(RED),
        BoxSpec::default(),
        || {},
    );
    Box(
        Modifier::empty()
            .test_tag("z8_sub_rem")
            .size_points(rem_w, anchor_h)
            .background(BLUE_SOFT)
            .border(1.0, BLUE, rect_shape()),
        BoxSpec::default(),
        || {},
    );
    Box(
        Modifier::empty()
            .test_tag("z8_sub_bot_l")
            .size_points(half_w, 14.0)
            .background(VIOLET),
        BoxSpec::default(),
        || {},
    );
    Box(
        Modifier::empty()
            .test_tag("z8_sub_bot_r")
            .size_points(rest_w, 14.0)
            .background(TEAL),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn Zone8TwoPassSubcompose() {
    SubcomposeLayout(
        Modifier::empty()
            .test_tag("z8_subcompose")
            .fill_max_width()
            .height(68.0)
            .background(BG_SUBTLE)
            .border(1.0, BORDER_STRONG, rect_shape()),
        |scope, constraints| {
            let pad = 4.0_f32;
            let gap = 6.0_f32;
            let inner_w = (constraints.max_width - pad * 2.0).max(0.0);
            let loose = Constraints {
                min_width: 0.0,
                max_width: inner_w,
                min_height: 0.0,
                max_height: constraints.max_height,
            };

            let anchor_ms = scope.subcompose(SlotId::new(1), loose, || {
                Box(
                    Modifier::empty()
                        .test_tag("z8_sub_anchor")
                        .background(AMBER)
                        .rounded_corners(4.0)
                        .padding_symmetric(8.0, 3.0),
                    BoxSpec::default(),
                    || {
                        Text(
                            "Subcomposed Anchor",
                            Modifier::empty(),
                            atlas_text_style(8.5, Color::WHITE, true),
                        );
                    },
                );
            });
            let mut anchor_ps = Vec::with_capacity(anchor_ms.len());
            let mut anchor_w = 0.0_f32;
            let mut anchor_h = 0.0_f32;
            for m in anchor_ms {
                let p = scope.measure(m, loose);
                anchor_w = anchor_w.max(p.width());
                anchor_h = anchor_h.max(p.height());
                anchor_ps.push(p);
            }
            let rem_w = (inner_w - anchor_w - gap).max(0.0);
            let half_w = (anchor_w * 0.5).round();
            let rest_w = (inner_w - half_w - gap).max(0.0);

            let track_ps: Vec<_> = scope
                .subcompose(SlotId::new(2), loose, move || {
                    Zone8TrackBoxes(anchor_w, anchor_h, rem_w, half_w, rest_w);
                })
                .into_iter()
                .map(|m| scope.measure(m, loose))
                .collect();

            let mut placements = Vec::with_capacity(anchor_ps.len() + track_ps.len());
            for p in &anchor_ps {
                placements.push(Placement::new(p.node_id(), pad, pad, 0.0));
            }
            if track_ps.len() >= 4 {
                let bot_y = pad + anchor_h + 14.0;
                let coords = [
                    (pad, pad + anchor_h + 3.0),
                    (pad + anchor_w + gap, pad),
                    (pad, bot_y),
                    (pad + half_w + gap, bot_y),
                ];
                for (p, (x, y)) in track_ps.iter().zip(coords) {
                    placements.push(Placement::new(p.node_id(), x, y, 0.0));
                }
            }
            scope.layout(constraints.max_width, constraints.max_height, placements)
        },
    );
}

#[composable]
fn Zone8DynamicSlotSubcompose() {
    SubcomposeLayout(
        Modifier::empty()
            .test_tag("z8_sub_dyn")
            .fill_max_width()
            .height(34.0)
            .background(AMBER_SOFT)
            .border(1.0, AMBER, rect_shape()),
        |scope, constraints| {
            let (pad, tile_w, tile_h, gap) = (4.0_f32, 36.0_f32, 22.0_f32, 5.0_f32);
            let usable = (constraints.max_width - pad * 2.0).max(0.0);
            let count = (((usable + gap) / (tile_w + gap)).floor() as usize).max(1);
            let fixed = Constraints::tight(tile_w, tile_h);
            let top_y = ((constraints.max_height - tile_h) / 2.0).floor().max(0.0);
            let mut placements = Vec::with_capacity(count);
            for i in 0..count {
                let ms = scope.subcompose(SlotId::new(100 + i as u64), fixed, move || {
                    LabeledChip(
                        format!("z8_sub_t{i}"),
                        format!("S{i}"),
                        ChipSpec {
                            width: 36.0,
                            height: 22.0,
                            bg: ACCENTS[i % ACCENTS.len()],
                            radius: 0.0,
                            font_size: 8.0,
                        },
                    );
                });
                if let Some(m) = ms.into_iter().next() {
                    let p = scope.measure(m, fixed);
                    let x = pad + i as f32 * (tile_w + gap);
                    placements.push(Placement::new(p.node_id(), x, top_y, 0.0));
                }
            }
            scope.layout(constraints.max_width, constraints.max_height, placements)
        },
    );
}
