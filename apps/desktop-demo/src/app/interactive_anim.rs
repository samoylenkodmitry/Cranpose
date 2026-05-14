#![allow(non_snake_case)]

use cranpose_animation::{animateFloatAsState, spring, tween, Easing, Spring};
use cranpose_ui::{
    composable, rememberMutableInteractionSource,
    text::{FontWeight, SpanStyle, TextUnit},
    Brush, Button, ButtonSpec, Color, Column, ColumnSpec, CornerRadii, GraphicsLayer, Interaction,
    LayerShape, LinearArrangement, Modifier, Point, PressInteraction, PressInteractionPress,
    RoundedCornerShape, Row, RowSpec, Size, Spacer, Text, TextStyle, VerticalAlignment,
};

const BUTTON_RADIUS: f32 = 20.0;
const BUTTON_RELEASE_DURATION_MS: u64 = 1_000;
const REVEAL_DURATION_MS: u64 = 850;
const REVEAL_EXTRA_RADIUS: f32 = 10.0;
const REVEAL_ALPHA: f32 = 0.28;

#[derive(Clone, Copy, Debug, PartialEq)]
struct RevealTrigger {
    generation: u64,
    origin: Point,
}

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

fn press_interaction_press(interaction: Option<Interaction>) -> Option<PressInteractionPress> {
    match interaction {
        Some(Interaction::Press(PressInteraction::Press(press))) => Some(press),
        Some(Interaction::Press(PressInteraction::Release(release))) => Some(release.press),
        Some(Interaction::Press(PressInteraction::Cancel(cancel))) => Some(cancel.press),
        None => None,
    }
}

fn reveal_alpha(progress: f32) -> f32 {
    let fade_in = (progress / 0.16).clamp(0.0, 1.0);
    let fade_out = (1.0 - progress).clamp(0.0, 1.0);
    REVEAL_ALPHA * fade_in * fade_out
}

fn reveal_end_radius(size: Size) -> f32 {
    ((size.width * size.width + size.height * size.height).sqrt() * 0.5) + REVEAL_EXTRA_RADIUS
}

#[composable]
fn PressAnimatedButton(text: &'static str, modifier: Modifier, on_click: impl FnMut() + 'static) {
    let interaction_source = rememberMutableInteractionSource();
    let pressed = interaction_source.collectIsPressedAsState();
    let is_pressed = pressed.value();
    let press_animation = if is_pressed {
        spring(Spring::DampingRatioMediumBouncy, Spring::StiffnessMedium)
    } else {
        tween(BUTTON_RELEASE_DURATION_MS, Easing::FastOutSlowInEasing)
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
fn RevealAnimatedButton(text: &'static str, modifier: Modifier, on_click: impl FnMut() + 'static) {
    let interaction_source = rememberMutableInteractionSource();
    let last_interaction = interaction_source.collectLastInteractionAsState();
    let reveal_trigger = cranpose_core::useState(|| RevealTrigger {
        generation: 0,
        origin: Point::default(),
    });
    let on_click = on_click;

    let trigger = reveal_trigger.value();
    let target_progress = if trigger.generation % 2 == 0 {
        0.0
    } else {
        1.0
    };
    let raw_progress = animateFloatAsState(
        target_progress,
        tween(REVEAL_DURATION_MS, Easing::FastOutSlowInEasing),
        "interactive_button_reveal_progress",
    );
    let progress = if trigger.generation == 0 {
        1.0
    } else if target_progress > 0.5 {
        raw_progress.value()
    } else {
        1.0 - raw_progress.value()
    };
    let origin = trigger.origin;
    Button(
        modifier
            .rounded_corners(BUTTON_RADIUS)
            .clip_to_bounds()
            .draw_behind(move |scope| {
                scope.draw_round_rect(
                    Brush::solid(Color(0.13, 0.14, 0.18, 1.0)),
                    CornerRadii::uniform(BUTTON_RADIUS),
                );

                let alpha = reveal_alpha(progress);
                if alpha > 0.001 {
                    let size = scope.size();
                    let start_radius = size.width.max(size.height) * 0.3;
                    let end_radius = reveal_end_radius(size);
                    let radius = start_radius + (end_radius - start_radius) * progress;
                    scope.draw_circle(Brush::solid(Color(0.38, 0.78, 1.0, alpha)), origin, radius);
                }
            })
            .padding(18.0),
        ButtonSpec::new().interaction_source(interaction_source),
        move || {
            let press = press_interaction_press(last_interaction.get());
            reveal_trigger.update(|trigger| {
                trigger.generation = trigger.generation.wrapping_add(1);
                if let Some(press) = press {
                    trigger.origin = press.press_position;
                }
            });
            on_click();
        },
        move || {
            Text(text, Modifier::empty(), button_text_style());
        },
    );
}

#[composable]
pub(crate) fn InteractiveAnimTab() {
    let clicks = cranpose_core::useState(|| 0u32);
    let reveal_clicks = cranpose_core::useState(|| 0u32);
    let click_count = clicks.get();
    let reveal_click_count = reveal_clicks.get();

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

                        RevealAnimatedButton(
                            "Reveal",
                            Modifier::empty().width(220.0).height(86.0),
                            move || {
                                reveal_clicks.update(|count| *count += 1);
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
                                Text("Press clicks", Modifier::empty(), body_style());
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
                                Text("Reveal clicks", Modifier::empty(), body_style());
                                Text(
                                    reveal_click_count.to_string(),
                                    Modifier::empty(),
                                    TextStyle {
                                        span_style: SpanStyle {
                                            color: Some(Color(0.13, 0.14, 0.18, 1.0)),
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
