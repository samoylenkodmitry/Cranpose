#![allow(non_snake_case)]

use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, Easing, RepeatMode, StartOffset,
};
use cranpose_foundation::lazy::{remember_lazy_list_state, LazyListScope};
use cranpose_ui::{
    composable,
    text::{FontWeight, SpanStyle},
    Color, Column, ColumnSpec, GraphicsLayer, LinearArrangement, Modifier, Row, RowSpec, Size,
    Spacer, Text, TextStyle, VerticalAlignment,
};
use cranpose_ui::{LazyColumn, LazyColumnSpec};

#[derive(Clone)]
struct PulseSample {
    value: cranpose_core::State<f32>,
}

#[composable]
fn PulseModel(period_ms: u64, label: &'static str) -> PulseSample {
    let transition = rememberInfiniteTransition(label);
    let pulse = transition.animateFloat(
        0.0,
        1.0,
        infiniteRepeatable(
            AnimationSpec::tween(period_ms, Easing::EaseInOut),
            RepeatMode::Reverse,
            StartOffset::default(),
        ),
        label,
    );

    PulseSample { value: pulse }
}

fn card_modifier() -> Modifier {
    Modifier::empty()
        .fill_max_width()
        .background(Color(0.98, 0.98, 0.98, 1.0))
        .rounded_corners(8.0)
        .padding(12.0)
}

#[composable]
pub(crate) fn AnimationsTab() {
    let pulse = PulseModel(900, "pulse_alpha");
    let slide = PulseModel(1200, "slide_offset");
    let scale = PulseModel(700, "scale_bounce");

    let header_style = TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.05, 0.05, 0.05, 1.0)),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    };

    let section_style = TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.2, 0.2, 0.2, 1.0)),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    };

    Column(
        Modifier::empty().fill_max_width().padding(16.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(16.0)),
        move || {
            let pulse_value = pulse.value;
            let slide_value = slide.value;
            let scale_value = scale.value;
            Text(
                "Animations Showcase",
                Modifier::empty().padding(4.0),
                header_style.clone(),
            );

            Column(
                card_modifier(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                {
                    let section_style = section_style.clone();
                    move || {
                        Text("Pulse Alpha", Modifier::empty(), section_style.clone());
                        let alpha = 0.2 + 0.8 * pulse_value.value();
                        Row(
                            Modifier::empty()
                                .fill_max_width()
                                .background(Color(0.2, 0.25, 0.3, alpha))
                                .rounded_corners(6.0)
                                .padding(8.0),
                            RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
                            move || {
                                Text(
                                    format!("Alpha {:.2}", alpha),
                                    Modifier::empty(),
                                    TextStyle {
                                        span_style: SpanStyle {
                                            color: Some(Color(1.0, 1.0, 1.0, 1.0)),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    },
                                );
                            },
                        );
                    }
                },
            );

            Column(
                card_modifier(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                {
                    let section_style = section_style.clone();
                    move || {
                        Text("Slide Offset", Modifier::empty(), section_style.clone());
                        let offset_x = 8.0 + 140.0 * slide_value.value();
                        Row(
                            Modifier::empty()
                                .fill_max_width()
                                .height(44.0)
                                .background(Color(0.92, 0.94, 0.97, 1.0))
                                .rounded_corners(6.0),
                            RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
                            move || {
                                Row(
                                    Modifier::empty()
                                        .offset(offset_x, 0.0)
                                        .width(32.0)
                                        .height(24.0)
                                        .background(Color(0.2, 0.5, 0.85, 1.0))
                                        .rounded_corners(4.0),
                                    RowSpec::default(),
                                    || {},
                                );
                                Spacer(Size {
                                    width: 8.0,
                                    height: 0.0,
                                });
                                Text(
                                    format!("Offset {:.1}", offset_x),
                                    Modifier::empty(),
                                    TextStyle {
                                        span_style: SpanStyle {
                                            color: Some(Color(0.2, 0.2, 0.2, 1.0)),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    },
                                );
                            },
                        );
                    }
                },
            );

            Column(
                card_modifier(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                {
                    let section_style = section_style.clone();
                    move || {
                        Text(
                            "Scale + Fade (layer lambda)",
                            Modifier::empty(),
                            section_style.clone(),
                        );
                        Row(
                            Modifier::empty()
                                .fill_max_width()
                                .height(48.0)
                                .background(Color(0.96, 0.93, 0.9, 1.0))
                                .rounded_corners(6.0)
                                .padding(6.0),
                            RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
                            move || {
                                Row(
                                    Modifier::empty()
                                        .graphics_layer(move || {
                                            let progress = scale_value.value();
                                            GraphicsLayer {
                                                alpha: 0.4 + 0.6 * progress,
                                                scale: 0.85 + 0.3 * progress,
                                                ..Default::default()
                                            }
                                        })
                                        .width(36.0)
                                        .height(36.0)
                                        .background(Color(0.85, 0.35, 0.2, 1.0))
                                        .rounded_corners(6.0),
                                    RowSpec::default(),
                                    || {},
                                );
                                Spacer(Size {
                                    width: 10.0,
                                    height: 0.0,
                                });
                                Text(
                                    "Animation sampled inside graphics_layer",
                                    Modifier::empty(),
                                    TextStyle {
                                        span_style: SpanStyle {
                                            color: Some(Color(0.2, 0.2, 0.2, 1.0)),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    },
                                );
                            },
                        );
                    }
                },
            );

            Column(
                card_modifier(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                {
                    let section_style = section_style.clone();
                    move || {
                        Text(
                            "Lazy List Animation",
                            Modifier::empty(),
                            section_style.clone(),
                        );

                        let list_state = remember_lazy_list_state();
                        LazyColumn(
                            Modifier::empty()
                                .fill_max_width()
                                .height(140.0)
                                .background(Color(1.0, 1.0, 1.0, 1.0))
                                .rounded_corners(6.0)
                                .padding(8.0),
                            list_state,
                            LazyColumnSpec::new()
                                .vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                            |scope| {
                                scope.item(Some(0), None, || {
                                    Text(
                                        "Static item: ready",
                                        Modifier::empty(),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(0.35, 0.35, 0.35, 1.0)),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                    );
                                });

                                scope.item(Some(1), None, || {
                                    let lazy_pulse = PulseModel(800, "lazy_pulse");
                                    let display = (lazy_pulse.value.value() * 100.0).round() as i32;
                                    Text(
                                        format!("Lazy Pulse: {}", display),
                                        Modifier::empty(),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(0.15, 0.2, 0.3, 1.0)),
                                                font_weight: Some(FontWeight::BOLD),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                    );
                                });

                                scope.item(Some(2), None, || {
                                    Text(
                                        "Static item: complete",
                                        Modifier::empty(),
                                        TextStyle {
                                            span_style: SpanStyle {
                                                color: Some(Color(0.35, 0.35, 0.35, 1.0)),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                    );
                                });
                            },
                        );
                    }
                },
            );
        },
    );
}
