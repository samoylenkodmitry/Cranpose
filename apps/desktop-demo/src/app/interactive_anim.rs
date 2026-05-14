#![allow(non_snake_case)]

use cranpose_animation::{animateFloatAsState, spring, Spring};
use cranpose_ui::{
    composable, rememberMutableInteractionSource,
    text::{FontWeight, SpanStyle, TextUnit},
    Brush, Button, ButtonSpec, Color, Column, ColumnSpec, CornerRadii, GraphicsLayer, LayerShape,
    LinearArrangement, Modifier, RoundedCornerShape, Row, RowSpec, Size, Spacer, Text, TextStyle,
    VerticalAlignment,
};

const BUTTON_RADIUS: f32 = 20.0;
const BUTTON_RELEASE_STIFFNESS: f32 = Spring::StiffnessVeryLow / 4.0;

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
    let is_pressed = pressed.value();
    let press_animation = if is_pressed {
        spring(Spring::DampingRatioMediumBouncy, Spring::StiffnessMedium)
    } else {
        spring(Spring::DampingRatioNoBouncy, BUTTON_RELEASE_STIFFNESS)
    };
    let target_scale = if is_pressed { 0.94 } else { 1.0 };
    let scale = animateFloatAsState(
        target_scale,
        press_animation,
        "interactive_button_press_scale",
    );
    let target_shadow = if is_pressed { 4.0 } else { 10.0 };
    let shadow = animateFloatAsState(
        target_shadow,
        press_animation,
        "interactive_button_press_shadow",
    );
    let fill = if is_pressed {
        Color(0.03, 0.33, 0.64, 1.0)
    } else {
        Color(0.05, 0.42, 0.82, 1.0)
    };

    Button(
        modifier
            .graphics_layer_block(move |layer: &mut GraphicsLayer| {
                let scale = scale.value();
                layer.scale_x = scale;
                layer.scale_y = scale;
                layer.shadow_elevation = shadow.value();
                layer.shape = LayerShape::Rounded(RoundedCornerShape::uniform(BUTTON_RADIUS));
            })
            .rounded_corners(BUTTON_RADIUS)
            .draw_behind(move |scope| {
                scope.draw_round_rect(Brush::solid(fill), CornerRadii::uniform(BUTTON_RADIUS));
            })
            .padding(18.0),
        ButtonSpec::new().interaction_source(interaction_source),
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
