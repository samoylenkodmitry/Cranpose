use std::{cell::Cell, rc::Rc};

use cranpose_core::SlotId;
use cranpose_ui::{
    Box, BoxSpec, LayoutBox, Modifier, Row, RowSpec, Size, SubcomposeLayoutScope,
    SubcomposeMeasureScope, Text, TextMeasurer, TextMetrics, TextStyle,
    text::{
        AnnotatedString, LineBox, LineHeightAlignment, LineHeightStyle, LineHeightTrim, SpanStyle,
        TextUnit,
    },
    text_layout_result::TextLayoutResult,
};
use cranpose_ui_layout::{Placement, VerticalAlignment};

use crate::{
    text_contract_measurer::ContractMeasurer,
    text_measured_layout_integration::{lay_out_in_column_with_measurer, layout_composition},
};

struct BaselineMeasurer;

#[derive(Clone, Copy, PartialEq)]
enum CustomBaselinePolicy {
    Explicit,
    Reordered,
    DirectPlacement,
}

fn custom_baseline_result(constraints: cranpose_ui::Constraints) -> cranpose_ui::MeasureResult {
    let (width, height) = constraints.constrain(12.0, 24.0);
    cranpose_ui::MeasureResult::new(Size::new(width, height), Vec::new())
        .with_alignment_lines(cranpose_ui::AlignmentLines::new(Some(18.0), Some(18.0)))
}

impl cranpose_ui_layout::MeasurePolicy for CustomBaselinePolicy {
    fn measure(
        &self,
        _: &dyn cranpose_ui_layout::MeasureScope,
        measurables: &[std::boxed::Box<dyn cranpose_ui_layout::Measurable>],
        constraints: cranpose_ui::Constraints,
    ) -> cranpose_ui::MeasureResult {
        if !matches!(self, Self::Explicit) {
            let mut placements = Vec::new();
            for (index, child) in measurables.iter().enumerate().rev() {
                let placeable = child.measure(constraints);
                let y = if index == 0 { 30.0 } else { 0.0 };
                if matches!(self, Self::DirectPlacement) {
                    placeable.place(0.0, y);
                } else {
                    placements.push(Placement::new(placeable.node_id(), 0.0, y, 0));
                    if index == 0 {
                        placements.push(Placement::new(placeable.node_id(), 0.0, -20.0, 0));
                    }
                }
            }
            return cranpose_ui::MeasureResult::new(Size::new(60.0, 40.0), placements);
        }
        custom_baseline_result(constraints)
    }
    fn min_intrinsic_width(
        &self,
        _: &[std::boxed::Box<dyn cranpose_ui_layout::Measurable>],
        _: f32,
    ) -> f32 {
        12.0
    }
    fn max_intrinsic_width(
        &self,
        _: &[std::boxed::Box<dyn cranpose_ui_layout::Measurable>],
        _: f32,
    ) -> f32 {
        12.0
    }
    fn min_intrinsic_height(
        &self,
        _: &[std::boxed::Box<dyn cranpose_ui_layout::Measurable>],
        _: f32,
    ) -> f32 {
        24.0
    }
    fn max_intrinsic_height(
        &self,
        _: &[std::boxed::Box<dyn cranpose_ui_layout::Measurable>],
        _: f32,
    ) -> f32 {
        24.0
    }
}

fn custom_baseline_layout(modifier: Modifier, subcompose: bool) {
    if subcompose {
        cranpose_ui::SubcomposeLayout(modifier, |_, constraints| {
            custom_baseline_result(constraints)
        });
    } else {
        cranpose_ui::Layout(modifier, CustomBaselinePolicy::Explicit, || {});
    }
}

impl TextMeasurer for BaselineMeasurer {
    fn measure(&self, text: &AnnotatedString, style: &TextStyle) -> TextMetrics {
        let size = style.resolve_font_size(10.0);
        let (line_count, columns) = text
            .text
            .split('\n')
            .fold((0usize, 0usize), |(count, width), line| {
                (count + 1, width.max(line.chars().count()))
            });
        let width = columns as f32 * size * 0.5;
        let line = self.line_box(style).expect("fixture line metrics");
        TextMetrics {
            width,
            height: line.block_height(line_count),
            line_height: line.height,
            line_count,
        }
    }

    fn line_box(&self, style: &TextStyle) -> Option<LineBox> {
        let size = style.resolve_font_size(10.0);
        let trimmed = !style
            .paragraph_style
            .line_height_style
            .is_some_and(|line| line.trim == LineHeightTrim::None);
        let trim = if trimmed { size * 0.3 } else { 0.0 };
        Some(LineBox {
            height: size * 1.6,
            baseline: size * 1.1,
            trim_top: trim,
            trim_bottom: trim,
        })
    }

    fn first_baseline(&self, style: &TextStyle) -> Option<f32> {
        self.line_box(style).map(|line| line.baseline)
    }

    fn get_offset_for_position(
        &self,
        text: &AnnotatedString,
        style: &TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        ContractMeasurer.get_offset_for_position(text, style, x, y)
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &AnnotatedString,
        style: &TextStyle,
        offset: usize,
    ) -> f32 {
        ContractMeasurer.get_cursor_x_for_offset(text, style, offset)
    }

    fn layout(&self, text: &AnnotatedString, style: &TextStyle) -> TextLayoutResult {
        let size = style.resolve_font_size(10.0);
        TextLayoutResult::monospaced(&text.text, size * 0.5, size * 1.6)
    }
}

fn style(size: f32) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(size),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn find_text<'a>(node: &'a LayoutBox, text: &str) -> Option<&'a LayoutBox> {
    if node.node_data.modifier_slices().text_content() == Some(text) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_text(child, text))
}

fn text_top(container: &LayoutBox, text: &str) -> f32 {
    let node = find_text(container, text).expect("text in layout");
    let content = node
        .node_data
        .modifier_slices()
        .text_content_rect(Size::new(node.rect.width, node.rect.height));
    node.rect.y + content.y - container.rect.y
}

fn baseline_row(
    small_text: &'static str,
    small_modifier: fn() -> Modifier,
    wrapped: bool,
    untrimmed: bool,
    inspect: impl FnOnce(&LayoutBox, f32, f32),
) {
    lay_out_in_column_with_measurer(
        move || {
            Row(Modifier::empty(), RowSpec::default(), move || {
                let small_modifier = small_modifier();
                let text_style = move |size| {
                    let mut text_style = style(size);
                    if untrimmed {
                        text_style.paragraph_style.line_height_style = Some(LineHeightStyle {
                            alignment: LineHeightAlignment::Center,
                            trim: LineHeightTrim::None,
                            ..Default::default()
                        });
                    }
                    text_style
                };
                if wrapped {
                    Box(
                        small_modifier.align_by_baseline(),
                        BoxSpec::default(),
                        move || {
                            Text(small_text, Modifier::empty(), text_style(10.0));
                        },
                    );
                } else {
                    Text(
                        small_text,
                        small_modifier.align_by_baseline(),
                        text_style(10.0),
                    );
                }
                Text(
                    "large",
                    Modifier::empty().align_by_baseline(),
                    text_style(20.0),
                );
            });
        },
        BaselineMeasurer,
        |layout| {
            let row = layout.root().children.first().expect("row in column");
            inspect(row, text_top(row, small_text), text_top(row, "large"));
        },
    );
}

#[test]
fn row_aligns_different_font_sizes_by_their_first_baselines() {
    baseline_row(
        "small",
        Modifier::empty,
        false,
        false,
        |row, small, large| {
            assert_eq!(small + 8.0, large + 16.0);
            assert_eq!(small, 8.0);
            assert_eq!(large, 0.0);
            assert_eq!(row.rect.height, 20.0);
        },
    );
}

#[test]
fn row_inherits_a_baseline_through_a_padded_wrapper() {
    baseline_row(
        "small",
        || Modifier::empty().padding_each(0.0, 18.0, 0.0, 0.0),
        true,
        false,
        |row, small, large| {
            assert_eq!(small + 8.0, large + 16.0);
            assert_eq!(small, 18.0);
            assert_eq!(large, 10.0);
            assert_eq!(row.rect.height, 30.0);
        },
    );
}

#[test]
fn row_keeps_every_line_when_first_baselines_align() {
    baseline_row(
        "small\nsecond",
        Modifier::empty,
        false,
        false,
        |row, small, large| {
            assert_eq!(small + 8.0, large + 16.0);
            assert_eq!(small, 8.0);
            assert_eq!(large, 0.0);
            assert_eq!(row.rect.height, 34.0);
        },
    );
}

#[test]
fn row_aligns_the_drawn_baselines_when_line_leading_is_retained() {
    baseline_row(
        "small",
        Modifier::empty,
        false,
        true,
        |row, small, large| {
            assert_eq!(small + 11.0, large + 22.0);
            assert_eq!(small, 11.0);
            assert_eq!(large, 0.0);
            assert_eq!(row.rect.height, 32.0);
        },
    );
}

#[test]
fn subcomposed_layout_can_align_to_the_last_drawn_line_inside_padding_and_offset() {
    lay_out_in_column_with_measurer(
        || {
            Row(Modifier::empty(), RowSpec::default(), || {
                cranpose_ui::SubcomposeLayout(
                    Modifier::empty().align_by_baseline(),
                    |scope, constraints| {
                        let children = scope.subcompose(SlotId::new(0), (), || {
                            Box(
                                Modifier::empty()
                                    .padding_each(0.0, 3.0, 0.0, 7.0)
                                    .offset(0.0, 5.0),
                                BoxSpec::default(),
                                || {
                                    cranpose_ui::TextWithOptions(
                                        "small\nsecond",
                                        Modifier::empty(),
                                        style(10.0),
                                        cranpose_ui::TextOptions {
                                            min_lines: 5,
                                            ..Default::default()
                                        },
                                    );
                                },
                            );
                            Text("marker", Modifier::empty(), style(10.0));
                        });
                        let source = scope.measure(children[0], constraints);
                        let marker = scope.measure(children[1], constraints);
                        let marker_y = source
                            .alignment_lines()
                            .last_baseline()
                            .expect("text last baseline")
                            - marker
                                .alignment_lines()
                                .first_baseline()
                                .expect("marker baseline");
                        scope.layout(
                            source.width() + marker.width(),
                            source.height().max(marker_y + marker.height()),
                            [
                                Placement::new(source.node_id(), 0.0, 0.0, 0),
                                Placement::new(marker.node_id(), source.width(), marker_y, 0),
                            ],
                        )
                    },
                );
                Text("large", Modifier::empty().align_by_baseline(), style(20.0));
            });
        },
        BaselineMeasurer,
        |layout| {
            let row = layout.root().children.first().expect("row in column");
            let source = text_top(row, "small\nsecond");
            let marker = text_top(row, "marker");
            assert_eq!(source + 24.0, marker + 8.0);
            assert_eq!(source + 8.0, text_top(row, "large") + 16.0);
            assert_eq!(source, 8.0);
            assert_eq!(marker, 24.0);
        },
    );
}

#[test]
fn missing_baselines_fall_back_to_top_and_the_last_row_alignment_wins() {
    lay_out_in_column_with_measurer(
        || {
            Row(
                Modifier::empty().height(40.0),
                RowSpec::default().vertical_alignment(VerticalAlignment::Bottom),
                || {
                    Box(
                        Modifier::empty()
                            .size(Size::new(10.0, 10.0))
                            .align_by_baseline(),
                        BoxSpec::default(),
                        || {},
                    );
                    Text(
                        "bottom",
                        Modifier::empty()
                            .align_by_baseline()
                            .alignInRow(VerticalAlignment::Bottom),
                        style(10.0),
                    );
                    Text(
                        "baseline",
                        Modifier::empty()
                            .alignInRow(VerticalAlignment::Bottom)
                            .align_by_baseline(),
                        style(10.0),
                    );
                },
            );
        },
        BaselineMeasurer,
        |layout| {
            let row = layout.root().children.first().expect("row in column");
            assert_eq!(row.children[0].rect.y, row.rect.y);
            assert_eq!(text_top(row, "bottom"), 30.0);
            assert_eq!(text_top(row, "baseline"), 0.0);
        },
    );
}

#[test]
fn editable_text_aligns_with_a_label_baseline() {
    lay_out_in_column_with_measurer(
        || {
            Row(Modifier::empty(), RowSpec::default(), || {
                let state = cranpose_core::remember(|| {
                    cranpose_foundation::text::TextFieldState::new("small")
                })
                .with(|state| *state);
                cranpose_ui::BasicTextFieldWithOptions(
                    state,
                    Modifier::empty().align_by_baseline(),
                    cranpose_ui::BasicTextFieldOptions {
                        text_style: style(10.0),
                        ..Default::default()
                    },
                );
                Text("large", Modifier::empty().align_by_baseline(), style(20.0));
            });
        },
        BaselineMeasurer,
        |layout| {
            let row = layout.root().children.first().expect("row in column");
            assert_eq!(text_top(row, "small") + 8.0, text_top(row, "large") + 16.0);
        },
    );
}

#[test]
fn row_updates_baselines_after_font_changes_and_retains_them_across_parent_resizes() {
    let font_handle = Rc::new(Cell::new(None));
    let output = Rc::clone(&font_handle);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let font = cranpose_core::remember(|| cranpose_core::mutableStateOf(10.0_f32))
            .with(|state| *state);
        output.set(Some(font));
        Row(
            Modifier::empty().width(200.0),
            RowSpec::default(),
            move || {
                Text(
                    "small",
                    Modifier::empty().align_by_baseline(),
                    style(font.value()),
                );
                Text("large", Modifier::empty().align_by_baseline(), style(20.0));
            },
        );
    });
    cranpose_ui::set_text_measurer(BaselineMeasurer);
    let font = font_handle.get().expect("font state");
    for (size, viewport) in [(10.0, 300.0), (10.0, 400.0), (30.0, 400.0), (10.0, 300.0)] {
        font.set_value(size);
        while composition.process_invalid_scopes().expect("font update") {}
        let layout = layout_composition(&mut composition, Size::new(viewport, 300.0));
        let row = layout.root();
        let small_baseline = size * 0.8;
        assert_eq!(
            text_top(row, "small") + small_baseline,
            text_top(row, "large") + 16.0
        );
        assert_eq!(
            text_top(row, "small"),
            16.0_f32.max(small_baseline) - small_baseline
        );
    }
}

#[test]
fn custom_layouts_can_report_baselines_through_padding_and_parent_wrappers() {
    for (subcompose, wrapped) in [(false, false), (false, true), (true, false), (true, true)] {
        lay_out_in_column_with_measurer(
            move || {
                Row(Modifier::empty(), RowSpec::default(), move || {
                    let modifier = Modifier::empty()
                        .padding_each(0.0, 4.0, 0.0, 0.0)
                        .offset(0.0, 5.0)
                        .align_by_baseline();
                    if wrapped {
                        Box(modifier, BoxSpec::default(), move || {
                            custom_baseline_layout(Modifier::empty(), subcompose)
                        });
                    } else {
                        custom_baseline_layout(modifier, subcompose);
                    }
                    Text("large", Modifier::empty().align_by_baseline(), style(20.0));
                });
            },
            BaselineMeasurer,
            |layout| {
                let row = layout.root().children.first().expect("row in column");
                assert_eq!(text_top(row, "large"), 11.0);
            },
        );
    }
}

#[test]
fn reordered_custom_placements_inherit_the_baselines_of_the_drawn_children() {
    for policy in [
        CustomBaselinePolicy::Reordered,
        CustomBaselinePolicy::DirectPlacement,
    ] {
        lay_out_in_column_with_measurer(
            move || {
                Row(Modifier::empty(), RowSpec::default(), move || {
                    cranpose_ui::Layout(Modifier::empty().align_by_baseline(), policy, || {
                        Text("small", Modifier::empty(), style(10.0));
                        Text("large", Modifier::empty(), style(20.0));
                    });
                    Text(
                        "reference",
                        Modifier::empty().align_by_baseline(),
                        style(30.0),
                    );
                });
            },
            BaselineMeasurer,
            |layout| {
                let row = layout.root().children.first().expect("row in column");
                assert_eq!(text_top(row, "small"), 38.0);
                assert_eq!(text_top(row, "large"), 8.0);
                assert_eq!(text_top(row, "reference"), 0.0);
            },
        );
    }
}
