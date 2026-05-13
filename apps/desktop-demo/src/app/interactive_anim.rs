#![allow(non_snake_case)]

use cranpose_animation::{animateFloatAsState, spring, Spring};
use cranpose_ui::{
    composable, rememberMutableInteractionSource,
    text::{FontWeight, SpanStyle, TextUnit},
    Brush, ButtonWithInteractionSource, Color, Column, ColumnSpec, CornerRadii, GraphicsLayer,
    LinearArrangement, Modifier, Row, RowSpec, Size, Spacer, Text, TextStyle, VerticalAlignment,
};

fn title_style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.08, 0.10, 0.14, 1.0)),
            font_size: TextUnit::Sp(22.0),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn body_style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.20, 0.23, 0.28, 1.0)),
            font_size: TextUnit::Sp(15.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn button_text_style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color(1.0, 1.0, 1.0, 1.0)),
            font_size: TextUnit::Sp(18.0),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[composable]
fn PressAnimatedButton(text: &'static str, modifier: Modifier, on_click: impl FnMut() + 'static) {
    let interaction_source = rememberMutableInteractionSource();
    let pressed = interaction_source.collectIsPressedAsState();
    let target_scale = if pressed.value() { 0.94 } else { 1.0 };
    let scale = animateFloatAsState(
        target_scale,
        spring(Spring::DampingRatioMediumBouncy, Spring::StiffnessMedium),
        "interactive_button_press_scale",
    );
    let fill = if pressed.value() {
        Color(0.03, 0.33, 0.64, 1.0)
    } else {
        Color(0.05, 0.42, 0.82, 1.0)
    };
    let shadow = if pressed.value() { 4.0 } else { 10.0 };

    ButtonWithInteractionSource(
        modifier
            .graphics_layer_block(move |layer: &mut GraphicsLayer| {
                let scale = scale.value();
                layer.scale_x = scale;
                layer.scale_y = scale;
                layer.shadow_elevation = shadow;
            })
            .rounded_corners(20.0)
            .draw_behind(move |scope| {
                scope.draw_round_rect(Brush::solid(fill), CornerRadii::uniform(20.0));
            })
            .padding(18.0),
        interaction_source,
        on_click,
        move || {
            Text(text, Modifier::empty(), button_text_style());
        },
    );
}

#[composable]
pub(crate) fn InteractiveAnimTab() {
    let clicks = cranpose_core::useState(|| 0u32);
    let click_count = clicks.get();

    Column(
        Modifier::empty()
            .fill_max_width()
            .padding(18.0)
            .background(Color(0.96, 0.97, 0.98, 1.0))
            .rounded_corners(18.0)
            .padding(22.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(18.0)),
        move || {
            Text("Interactive Anim", Modifier::empty(), title_style());

            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(18.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                {
                    move || {
                        PressAnimatedButton(
                            "Tap me",
                            Modifier::empty().width(220.0).height(86.0),
                            move || {
                                clicks.update(|count| *count += 1);
                            },
                        );

                        Column(
                            Modifier::empty()
                                .background(Color(1.0, 1.0, 1.0, 1.0))
                                .rounded_corners(14.0)
                                .padding(16.0),
                            ColumnSpec::new()
                                .vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                            move || {
                                Text("Clicks", Modifier::empty(), body_style());
                                Text(
                                    click_count.to_string(),
                                    Modifier::empty(),
                                    TextStyle {
                                        span_style: SpanStyle {
                                            color: Some(Color(0.05, 0.42, 0.82, 1.0)),
                                            font_size: TextUnit::Sp(30.0),
                                            font_weight: Some(FontWeight::BOLD),
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

            Spacer(Size {
                width: 0.0,
                height: 4.0,
            });
        },
    );
}
