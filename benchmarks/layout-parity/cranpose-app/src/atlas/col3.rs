use cranpose::prelude::*;
use cranpose_ui::{
    LayoutDirection, ProvideLayoutDirection,
    widgets::{FlowColumn, FlowRow, FlowSpec},
};
use cranpose_ui_graphics::{
    Brush, Color, CornerRadii, GraphicsLayer, LayerShape, Point, Rect, RoundedCornerShape, Size,
    Stroke, TransformOrigin,
};
use cranpose_ui_layout::{Alignment, LinearArrangement, VerticalAlignment};

use super::common::{
    ACCENTS, AMBER, AMBER_SOFT, BG_CARD, BG_SUBTLE, BG_TRACK, BLUE, BLUE_SOFT, BORDER_CARD,
    BORDER_STRONG, CYAN, CYAN_SOFT, ChipSpec, EMERALD, EMERALD_SOFT, INDIGO, INDIGO_SOFT, INK,
    LabeledChip, MUTED, RED, ROSE, SLATE, TEAL, TEAL_SOFT, VIOLET, VIOLET_SOFT, ZoneCard,
    atlas_text_style, rect_shape,
};

#[composable]
pub(super) fn AtlasCol3(modifier: Modifier) {
    Column(
        modifier.fill_max_height(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
        || {
            Zone9RtlLtr(Modifier::empty().fill_max_width().columnWeight(1.18, true));
            Zone10FlowLayouts(Modifier::empty().fill_max_width().columnWeight(0.96, true));
            Zone11OffsetsAndLayers(Modifier::empty().fill_max_width().columnWeight(0.94, true));
            Zone12DrawAndShapes(Modifier::empty().fill_max_width().columnWeight(0.92, true));
        },
    );
}

#[composable]
fn Zone9RtlLtr(modifier: Modifier) {
    ZoneCard(
        "z9_card",
        "9. RTL vs LTR Mirroring · Relative Padding/Offset · LTR Island",
        BLUE,
        modifier,
        || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(5.0)),
                || {
                    Row(
                        Modifier::empty().fill_max_width().columnWeight(1.0, true),
                        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
                        || {
                            DirectionalSample(
                                "ltr",
                                LayoutDirection::Ltr,
                                Modifier::empty().rowWeight(1.0, true).fill_max_height(),
                            );
                            DirectionalSample(
                                "rtl",
                                LayoutDirection::Rtl,
                                Modifier::empty().rowWeight(1.0, true).fill_max_height(),
                            );
                        },
                    );
                    ProvideLayoutDirection(LayoutDirection::Rtl, || {
                        Row(
                            Modifier::empty()
                                .test_tag("z9_island_outer")
                                .fill_max_width()
                                .height(26.0)
                                .background(INDIGO_SOFT)
                                .border(1.0, INDIGO, rect_shape())
                                .padding_horizontal(4.0),
                            RowSpec::new()
                                .horizontal_arrangement(LinearArrangement::spaced_by(6.0))
                                .vertical_alignment(VerticalAlignment::CenterVertically),
                            || {
                                LabeledChip(
                                    "z9_island_rtl_tag".to_owned(),
                                    "RTL-1".to_owned(),
                                    ChipSpec {
                                        width: 38.0,
                                        height: 18.0,
                                        bg: INDIGO,
                                        radius: 0.0,
                                        font_size: 8.0,
                                    },
                                );
                                ProvideLayoutDirection(LayoutDirection::Ltr, || {
                                    Row(
                                        Modifier::empty()
                                            .test_tag("z9_island_ltr")
                                            .background(BG_CARD)
                                            .border(1.0, BLUE, rect_shape())
                                            .padding_symmetric(4.0, 2.0),
                                        RowSpec::new().horizontal_arrangement(
                                            LinearArrangement::spaced_by(4.0),
                                        ),
                                        || {
                                            for (i, &color) in ACCENTS.iter().enumerate().take(3) {
                                                LabeledChip(
                                                    format!("z9_isl_{i}"),
                                                    format!("#{i}"),
                                                    ChipSpec {
                                                        width: 24.0,
                                                        height: 14.0,
                                                        bg: color,
                                                        radius: 0.0,
                                                        font_size: 7.5,
                                                    },
                                                );
                                            }
                                        },
                                    );
                                });
                            },
                        );
                    });
                },
            );
        },
    );
}

#[composable]
fn DirectionalOffsetAndFlow(prefix: &'static str) {
    Box(
        Modifier::empty()
            .test_tag(format!("z9_{prefix}_off_box"))
            .fill_max_width()
            .height(30.0)
            .background(BG_TRACK),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .offset(14.0, 2.0)
                    .test_tag(format!("z9_{prefix}_off_rel"))
                    .size_points(28.0, 11.0)
                    .background(ROSE),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .absolute_offset(14.0, 16.0)
                    .test_tag(format!("z9_{prefix}_off_abs"))
                    .size_points(28.0, 11.0)
                    .background(CYAN),
                BoxSpec::default(),
                || {},
            );
        },
    );
    FlowRow(
        Modifier::empty()
            .test_tag(format!("z9_{prefix}_flow"))
            .fill_max_width(),
        FlowSpec::new()
            .main_axis_spacing(4.0)
            .cross_axis_spacing(3.0),
        move || {
            for (i, w) in [42.0, 54.0, 38.0, 48.0].into_iter().enumerate() {
                Box(
                    Modifier::empty()
                        .test_tag(format!("z9_{prefix}_f{i}"))
                        .size_points(w, 12.0)
                        .background(ACCENTS[i + 3]),
                    BoxSpec::default(),
                    || {},
                );
            }
        },
    );
}

#[composable]
fn DirectionalSample(prefix: &'static str, direction: LayoutDirection, modifier: Modifier) {
    let label = if direction == LayoutDirection::Ltr {
        "LTR"
    } else {
        "RTL"
    };
    ProvideLayoutDirection(direction, move || {
        Column(
            modifier
                .clone()
                .test_tag(format!("z9_{prefix}_panel"))
                .background(BG_SUBTLE)
                .border(1.0, BORDER_STRONG, rect_shape())
                .padding(4.0),
            ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
            move || {
                Text(label, Modifier::empty(), atlas_text_style(8.0, MUTED, true));
                Row(
                    Modifier::empty()
                        .test_tag(format!("z9_{prefix}_row"))
                        .fill_max_width()
                        .height(15.0)
                        .background(BG_TRACK),
                    RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
                    move || {
                        for (i, (w, bg)) in [(24.0, BLUE), (36.0, EMERALD), (16.0, AMBER)]
                            .into_iter()
                            .enumerate()
                        {
                            Box(
                                Modifier::empty()
                                    .test_tag(format!("z9_{prefix}_r{i}"))
                                    .size_points(w, 15.0)
                                    .background(bg),
                                BoxSpec::default(),
                                || {},
                            );
                        }
                    },
                );
                Box(
                    Modifier::empty()
                        .test_tag(format!("z9_{prefix}_col_bar"))
                        .size_points(44.0, 11.0)
                        .background(VIOLET),
                    BoxSpec::default(),
                    || {},
                );
                Box(
                    Modifier::empty()
                        .test_tag(format!("z9_{prefix}_pad_outer"))
                        .fill_max_width()
                        .height(19.0)
                        .background(AMBER_SOFT)
                        .padding_relative(18.0, 3.0, 4.0, 3.0),
                    BoxSpec::default(),
                    move || {
                        Box(
                            Modifier::empty()
                                .test_tag(format!("z9_{prefix}_pad_inner"))
                                .fill_max_size()
                                .background(AMBER),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
                DirectionalOffsetAndFlow(prefix);
            },
        );
    });
}

#[composable]
fn Zone10FlowLayouts(modifier: Modifier) {
    ZoneCard(
        "z10_card",
        "10. FlowRow & FlowColumn (Mixed Cross-Axis Sizes)",
        EMERALD,
        modifier,
        || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                || {
                    FlowRow(
                        Modifier::empty()
                            .test_tag("z10_flow_row")
                            .fill_max_width()
                            .background(BG_SUBTLE)
                            .border(1.0, BORDER_STRONG, rect_shape())
                            .padding(4.0),
                        FlowSpec::new()
                            .main_axis_spacing(5.0)
                            .cross_axis_spacing(4.0),
                        || {
                            let items = [
                                (50.0, 14.0),
                                (66.0, 22.0),
                                (42.0, 12.0),
                                (78.0, 16.0),
                                (56.0, 20.0),
                                (72.0, 14.0),
                                (46.0, 24.0),
                                (60.0, 14.0),
                                (52.0, 16.0),
                            ];
                            for (i, (w, h)) in items.into_iter().enumerate() {
                                LabeledChip(
                                    format!("z10_fr_{i}"),
                                    format!("R{i}"),
                                    ChipSpec {
                                        width: w,
                                        height: h,
                                        bg: ACCENTS[i % ACCENTS.len()],
                                        radius: 3.0,
                                        font_size: 7.5,
                                    },
                                );
                            }
                        },
                    );
                    FlowColumn(
                        Modifier::empty()
                            .test_tag("z10_flow_col")
                            .fill_max_width()
                            .height(74.0)
                            .background(TEAL_SOFT)
                            .border(1.0, TEAL, rect_shape())
                            .padding(4.0),
                        FlowSpec::new()
                            .main_axis_spacing(4.0)
                            .cross_axis_spacing(6.0),
                        || {
                            let cols = [
                                (54.0, 18.0),
                                (44.0, 26.0),
                                (62.0, 16.0),
                                (48.0, 22.0),
                                (68.0, 20.0),
                                (42.0, 24.0),
                                (52.0, 16.0),
                            ];
                            for (i, (w, h)) in cols.into_iter().enumerate() {
                                LabeledChip(
                                    format!("z10_fc_{i}"),
                                    format!("C{i}"),
                                    ChipSpec {
                                        width: w,
                                        height: h,
                                        bg: ACCENTS[(i + 2) % ACCENTS.len()],
                                        radius: 3.0,
                                        font_size: 7.5,
                                    },
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
fn Zone11OffsetsAndLayers(modifier: Modifier) {
    ZoneCard(
        "z11_card",
        "11. Offsets (Static/Lambda/Split) · GraphicsLayer",
        VIOLET,
        modifier,
        || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
                || {
                    Zone11OffsetsRow();
                    Zone11LayersRow();
                    Zone11NestedLayersRow();
                },
            );
        },
    );
}

#[composable]
fn Zone11OffsetsRow() {
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding_each(0.0, 4.0, 0.0, 0.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(12.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        || {
            Box(
                Modifier::empty()
                    .padding_relative(6.0, 0.0, 0.0, 0.0)
                    .test_tag("z11_off_neg_p")
                    .size_points(52.0, 30.0)
                    .background(BG_TRACK)
                    .border(1.0, SLATE, rect_shape()),
                BoxSpec::default(),
                || {
                    Box(
                        Modifier::empty()
                            .offset(-6.0, -4.0)
                            .test_tag("z11_off_neg")
                            .size_points(28.0, 18.0)
                            .background(ROSE),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z11_off_clip_p")
                    .size_points(52.0, 30.0)
                    .background(BG_TRACK)
                    .border(1.0, SLATE, rect_shape())
                    .clip_to_bounds(),
                BoxSpec::default(),
                || {
                    Box(
                        Modifier::empty()
                            .offset(30.0, 14.0)
                            .test_tag("z11_off_clip")
                            .size_points(34.0, 24.0)
                            .background(EMERALD),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z11_off_lam_p")
                    .size_points(52.0, 30.0)
                    .background(BG_TRACK)
                    .border(1.0, SLATE, rect_shape()),
                BoxSpec::default(),
                || {
                    Box(
                        Modifier::empty()
                            .offset_with(|| Point::new(10.0, 6.0))
                            .test_tag("z11_off_lam")
                            .size_points(28.0, 16.0)
                            .background(BLUE),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z11_bg_off_bg")
                    .size_points(48.0, 28.0)
                    .background(CYAN_SOFT)
                    .offset(8.0, 5.0)
                    .background(CYAN),
                BoxSpec::default(),
                || {
                    Box(
                        Modifier::empty()
                            .test_tag("z11_bg_off_inner")
                            .fill_max_size(),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

#[composable]
fn Zone11LayersRow() {
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding_each(0.0, 6.0, 0.0, 0.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(14.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        || {
            Box(
                Modifier::empty()
                    .test_tag("z11_gl_rot_c")
                    .graphics_layer_value(GraphicsLayer {
                        rotation_z: 14.0,
                        transform_origin: TransformOrigin::new(0.5, 0.5),
                        ..GraphicsLayer::DEFAULT
                    })
                    .size_points(54.0, 28.0)
                    .background(INDIGO)
                    .rounded_corners(4.0),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "Rot C",
                        Modifier::empty(),
                        atlas_text_style(8.0, Color::WHITE, true),
                    );
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z11_gl_rot_tl")
                    .graphics_layer_value(GraphicsLayer {
                        rotation_z: 14.0,
                        transform_origin: TransformOrigin::new(0.0, 0.0),
                        ..GraphicsLayer::DEFAULT
                    })
                    .size_points(54.0, 28.0)
                    .background(VIOLET)
                    .rounded_corners(4.0),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "Rot TL",
                        Modifier::empty(),
                        atlas_text_style(8.0, Color::WHITE, true),
                    );
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z11_gl_scale")
                    .graphics_layer_value(GraphicsLayer {
                        scale_x: 1.22,
                        scale_y: 0.78,
                        alpha: 0.55,
                        ..GraphicsLayer::DEFAULT
                    })
                    .size_points(52.0, 28.0)
                    .background(AMBER)
                    .rounded_corners(4.0),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "Scale",
                        Modifier::empty(),
                        atlas_text_style(8.0, Color::WHITE, true),
                    );
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z11_gl_clip")
                    .graphics_layer_value(GraphicsLayer {
                        translation_x: 6.0,
                        translation_y: -4.0,
                        clip: true,
                        shape: LayerShape::Rounded(RoundedCornerShape::uniform(10.0)),
                        ..GraphicsLayer::DEFAULT
                    })
                    .size_points(54.0, 28.0)
                    .background(TEAL_SOFT)
                    .rounded_corners(10.0)
                    .border(1.0, TEAL, RoundedCornerShape::uniform(10.0)),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Box(
                        Modifier::empty()
                            .required_size(Size::new(70.0, 12.0))
                            .background(TEAL),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

#[composable]
fn Zone11NestedLayersRow() {
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding_each(0.0, 8.0, 0.0, 0.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(24.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        || {
            Box(
                Modifier::empty()
                    .test_tag("z11_gl_nest_p")
                    .padding_relative(8.0, 0.0, 0.0, 0.0)
                    .graphics_layer_value(GraphicsLayer {
                        rotation_z: 10.0,
                        ..GraphicsLayer::DEFAULT
                    })
                    .size_points(96.0, 28.0)
                    .background(BLUE_SOFT)
                    .rounded_corners(4.0)
                    .border(1.0, BLUE, RoundedCornerShape::uniform(4.0)),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Box(
                        Modifier::empty()
                            .test_tag("z11_gl_nest_c")
                            .graphics_layer_value(GraphicsLayer {
                                rotation_z: -10.0,
                                ..GraphicsLayer::DEFAULT
                            })
                            .size_points(64.0, 16.0)
                            .background(BLUE)
                            .rounded_corners(3.0),
                        BoxSpec::new().content_alignment(Alignment::CENTER),
                        || {
                            Text(
                                "Level 0°",
                                Modifier::empty(),
                                atlas_text_style(7.5, Color::WHITE, true),
                            );
                        },
                    );
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z11_gl_combo")
                    .graphics_layer_value(GraphicsLayer {
                        rotation_z: -10.0,
                        scale_x: 1.15,
                        translation_x: 6.0,
                        transform_origin: TransformOrigin::new(1.0, 0.5),
                        ..GraphicsLayer::DEFAULT
                    })
                    .size_points(62.0, 24.0)
                    .background(ROSE)
                    .rounded_corners(4.0),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "Pivot R",
                        Modifier::empty(),
                        atlas_text_style(8.0, Color::WHITE, true),
                    );
                },
            );
        },
    );
}

#[composable]
fn Zone12DrawAndShapes(modifier: Modifier) {
    ZoneCard(
        "z12_card",
        "12. drawBehind · drawWithContent · drawWithCache · Shapes",
        ROSE,
        modifier,
        || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
                || {
                    Zone12DrawRow();
                    Zone12ShapeRow();
                    Zone12GradientRow();
                },
            );
        },
    );
}

#[composable]
fn Zone12DrawRow() {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(8.0)),
        || {
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z12_draw_behind")
                    .height(42.0)
                    .draw_behind(|scope| {
                        let sz = scope.size();
                        scope.draw_round_rect(Brush::solid(BLUE_SOFT), CornerRadii::uniform(6.0));
                        scope.draw_circle(
                            Brush::solid(Color::from_rgba_u8(0x25, 0x63, 0xEB, 0x55)),
                            Point::new(sz.width * 0.25, sz.height * 0.5),
                            14.0,
                        );
                        scope.draw_line(
                            Brush::solid(BLUE),
                            Point::new(4.0, sz.height - 4.0),
                            Point::new(sz.width - 4.0, 4.0),
                            Stroke::new(1.5),
                        );
                    }),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "Behind",
                        Modifier::empty(),
                        atlas_text_style(8.5, INK, true),
                    );
                },
            );
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z12_draw_content")
                    .height(42.0)
                    .draw_with_content(|scope| {
                        let sz = scope.size();
                        scope.draw_round_rect(Brush::solid(AMBER_SOFT), CornerRadii::uniform(6.0));
                        scope.draw_content();
                        scope.draw_line(
                            Brush::solid(Color::from_rgba_u8(0xDC, 0x26, 0x26, 0xBB)),
                            Point::new(4.0, 4.0),
                            Point::new(sz.width - 4.0, sz.height - 4.0),
                            Stroke::new(2.0),
                        );
                        scope.draw_circle(Brush::solid(RED), Point::new(sz.width - 8.0, 8.0), 4.0);
                    }),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "WithContent",
                        Modifier::empty(),
                        atlas_text_style(8.5, INK, true),
                    );
                },
            );
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z12_draw_cache")
                    .height(42.0)
                    .draw_with_cache(|builder| {
                        builder.on_draw_behind(|scope| {
                            let sz = scope.size();
                            scope.draw_round_rect(
                                Brush::solid(EMERALD_SOFT),
                                CornerRadii::uniform(8.0),
                            );
                            scope.draw_round_rect_at_stroked(
                                Rect {
                                    x: 4.0,
                                    y: 4.0,
                                    width: sz.width - 8.0,
                                    height: sz.height - 8.0,
                                },
                                Brush::solid(EMERALD),
                                CornerRadii::uniform(4.0),
                                Stroke::new(1.5),
                            );
                        });
                    }),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "Cache",
                        Modifier::empty(),
                        atlas_text_style(8.5, EMERALD, true),
                    );
                },
            );
        },
    );
}

#[composable]
fn Zone12ShapeRow() {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(8.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        || {
            let asym = RoundedCornerShape::new(14.0, 2.0, 14.0, 2.0);
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z12_asym")
                    .height(34.0)
                    .background(VIOLET_SOFT)
                    .rounded_corner_shape(asym)
                    .border(2.0, VIOLET, asym),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "Asym 14/2",
                        Modifier::empty(),
                        atlas_text_style(8.0, VIOLET, true),
                    );
                },
            );
            let pill = RoundedCornerShape::uniform(50.0);
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z12_pill")
                    .height(34.0)
                    .background(CYAN_SOFT)
                    .rounded_corner_shape(pill)
                    .border(2.0, CYAN, pill),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "Clamp Pill",
                        Modifier::empty(),
                        atlas_text_style(8.0, CYAN, true),
                    );
                },
            );
            let sh = RoundedCornerShape::uniform(8.0);
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z12_shadow")
                    .height(34.0)
                    .shadow_with(
                        4.0,
                        LayerShape::Rounded(sh),
                        true,
                        Color::BLACK,
                        Color::BLACK,
                    )
                    .background(BG_CARD)
                    .rounded_corner_shape(sh)
                    .border(1.0, BORDER_CARD, sh),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "Shadow 4",
                        Modifier::empty(),
                        atlas_text_style(8.0, INK, true),
                    );
                },
            );
        },
    );
}

#[composable]
fn Zone12GradientRow() {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(8.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        || {
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z12_grad_h")
                    .height(28.0)
                    .draw_behind(|scope| {
                        scope.draw_round_rect(
                            Brush::horizontal_gradient_default(vec![BLUE, CYAN, EMERALD]),
                            CornerRadii::uniform(6.0),
                        );
                    }),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "H-Grad",
                        Modifier::empty(),
                        atlas_text_style(8.0, Color::WHITE, true),
                    );
                },
            );
            let g_shape = RoundedCornerShape::uniform(6.0);
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z12_grad_v")
                    .height(28.0)
                    .background(BG_CARD)
                    .rounded_corner_shape(g_shape)
                    .border(
                        2.0,
                        Brush::vertical_gradient_default(vec![ROSE, AMBER]),
                        g_shape,
                    ),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "V-Border",
                        Modifier::empty(),
                        atlas_text_style(8.0, INK, true),
                    );
                },
            );
            Box(
                Modifier::empty()
                    .rowWeight(1.0, true)
                    .test_tag("z12_arc")
                    .height(28.0)
                    .draw_behind(|scope| {
                        let sz = scope.size();
                        scope.draw_rect(Brush::solid(BG_SUBTLE));
                        scope.draw_rect_stroked(Brush::solid(BORDER_STRONG), Stroke::new(1.0));
                        scope.draw_arc(
                            Brush::solid(INDIGO),
                            Point::new(sz.width * 0.5, sz.height * 0.5),
                            10.0,
                            -std::f32::consts::FRAC_PI_2,
                            240.0_f32.to_radians(),
                            Stroke::new(3.0),
                        );
                    }),
                BoxSpec::default(),
                || {},
            );
        },
    );
}
