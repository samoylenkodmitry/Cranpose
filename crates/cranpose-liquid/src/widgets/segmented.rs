//! Segmented control with a liquid selection blob: the glass pill behind the
//! selected segment runs its leading and trailing edges on different springs,
//! so it stretches like a droplet while traveling and settles round. Touching
//! it lifts the indicator into a magnifying glass lens that follows the finger
//! across the segments (the reference control swipes, it doesn't just tap).

use crate::material::{Glass, GlassDynamics, GlassMorph, LiquidModifierExt, LiquidShape};
use crate::motion::LiquidMotion;
use crate::theme::{liquid_colors, liquid_typography};
use cranpose_animation::{animateFloatAsState, spring};
use cranpose_core::{mutableStateOf, remember};
use cranpose_foundation::{PointerEventKind, PointerId};
use cranpose_macros::composable;
use cranpose_services::{default_haptics, HapticFeedback};
use cranpose_ui::text::{FontWeight, SpanStyle, TextStyle};
use cranpose_ui::widgets::{
    Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Row, RowSpec, Text,
};
use cranpose_ui::{Modifier, PointerInputScope, Size};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, GraphicsLayer};
use cranpose_ui_layout::Alignment;
use std::rc::Rc;

const SEGMENT_HEIGHT: f32 = 36.0;
const TRACK_PADDING: f32 = 2.0;
/// How far the interaction lens pokes past the track vertically.
const LENS_OVERFLOW: f32 = 8.0;
/// Touch raises the whole optical body before directional deformation. This
/// preserves the control's volume without letting maximum horizontal strain
/// squash the lens below the track height.
const LENS_LIFT_SCALE: f32 = 1.22;
/// Glass node span beyond the lens shape (rim glow + bulge live here).
const LENS_PAD: f32 = 10.0;
/// A segmented selection stays recognizably one cell wide while its surface
/// carries the shared incompressible fluid strain.
const SEGMENTED_STRAIN_RESPONSE: f32 = 0.36;
/// Pointer travel below this is a tap, not a swipe.
const TAP_SLOP: f32 = 4.0;

fn plain_indicator_alpha(lens_progress: f32) -> f32 {
    ((0.18 - lens_progress) / 0.16).clamp(0.0, 1.0)
}

fn segment_lens_left(pointer_x: f32, segment_width: f32, count: usize) -> f32 {
    (pointer_x - segment_width * 0.5).clamp(0.0, segment_width * (count.saturating_sub(1)) as f32)
}

/// A segmented control. `labels` are equal-width segments; `selected` is the
/// active index; `on_select` receives the committed index. Segments tap AND
/// swipe: dragging slides the indicator with the finger as a glass lens.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidSegmentedControl(
    modifier: Modifier,
    labels: Vec<String>,
    selected: usize,
    on_select: impl Fn(usize) + 'static,
) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    let count = labels.len().max(1);
    let selected = selected.min(count - 1);
    let on_select: Rc<dyn Fn(usize)> = Rc::new(on_select);
    let labels = Rc::new(labels);

    let pressed = remember(|| mutableStateOf(false)).with(|s| *s);

    let track_fill = colors.fill;
    let track = Modifier::empty()
        .height(SEGMENT_HEIGHT + TRACK_PADDING * 2.0)
        .draw_behind(move |scope| {
            scope.draw_round_rect(
                Brush::solid(track_fill),
                CornerRadii::uniform((SEGMENT_HEIGHT + TRACK_PADDING * 2.0) * 0.5),
            );
        });

    Box(track.then(modifier), BoxSpec::default(), move || {
        let labels = Rc::clone(&labels);
        let typography = typography.clone();
        let on_select = Rc::clone(&on_select);
        BoxWithConstraints(Modifier::empty().padding(TRACK_PADDING), move |scope| {
            let labels = Rc::clone(&labels);
            let typography = typography.clone();
            let on_select = Rc::clone(&on_select);
            let total_width = scope.constraints().max_width.max(1.0);
            let segment_width = total_width / count as f32;
            let selected_x = segment_width * selected as f32;
            let lens_axis = crate::motion::remember_liquid_drag_axis(selected_x);
            lens_axis.settle_to(selected_x, LiquidMotion::glide());
            let lens_x = lens_axis.value();
            let visual_index = crate::motion::liquid_visual_index(
                selected,
                lens_x,
                segment_width,
                count,
                crate::motion::liquid_axis_owns_visual_selection(
                    pressed.get(),
                    lens_x,
                    selected_x,
                    segment_width,
                ),
            );

            // The resting indicator belongs to controlled state. The
            // interaction lens has a separate direct-drag axis: it reads the
            // raw pointer while held and only springs after release.
            let leading = animateFloatAsState(
                selected_x,
                LiquidMotion::blob_leading(),
                "segmented-leading",
            );
            let trailing = animateFloatAsState(
                selected_x + segment_width,
                LiquidMotion::blob_trailing(),
                "segmented-trailing",
            );

            // Lens presence: up while touched, lingering decay on release
            // (the indicator stays liquid through the settle flight).
            let lens_settling = !lens_axis.is_dragging() && (lens_x - selected_x).abs() > 1.0;
            let lens_target = if pressed.get() || lens_settling {
                1.0
            } else {
                0.0
            };
            let lens_progress = animateFloatAsState(
                lens_target,
                if pressed.get() {
                    spring(0.9, 1400.0)
                } else {
                    spring(1.0, 170.0)
                },
                "segmented-lens",
            );

            let indicator_color = if colors.is_dark {
                Color::from_rgba_u8(90, 90, 96, 240)
            } else {
                Color::WHITE
            };
            let lens_for_indicator = lens_progress;
            let indicator = Modifier::empty()
                .size(Size::new(segment_width, SEGMENT_HEIGHT))
                .graphics_layer(move || {
                    let lead = leading.get();
                    let trail = trailing.get().max(lead + 1.0);
                    GraphicsLayer {
                        translation_x: lead,
                        scale_x: ((trail - lead) / segment_width.max(1.0)).max(0.01),
                        // The plain fill hides while the lens is up.
                        alpha: plain_indicator_alpha(lens_for_indicator.get()),
                        // Scale from the leading edge so translation stays exact.
                        transform_origin: cranpose_ui_graphics::TransformOrigin {
                            pivot_fraction_x: 0.0,
                            pivot_fraction_y: 0.5,
                        },
                        ..Default::default()
                    }
                })
                .draw_behind(move |scope| {
                    scope.draw_round_rect(
                        Brush::solid(indicator_color),
                        CornerRadii::uniform(SEGMENT_HEIGHT * 0.5),
                    );
                });
            Box(indicator, BoxSpec::default(), || {});

            // Labels row on top of the indicator. The cells keep button
            // semantics (robot/a11y); pointer handling lives on the swipe
            // surface below.
            Row(Modifier::empty(), RowSpec::default(), move || {
                for (index, label) in labels.iter().enumerate() {
                    let is_selected = index == visual_index;
                    let style = TextStyle {
                        span_style: SpanStyle {
                            color: Some(if is_selected {
                                colors.label
                            } else {
                                colors.secondary_label
                            }),
                            font_weight: Some(if is_selected {
                                FontWeight::SEMI_BOLD
                            } else {
                                FontWeight::MEDIUM
                            }),
                            ..typography.subheadline.span_style.clone()
                        },
                        ..typography.subheadline.clone()
                    };
                    let label_for_semantics = label.clone();
                    let cell = Modifier::empty()
                        .size(Size::new(segment_width, SEGMENT_HEIGHT))
                        .semantics(move |config| {
                            config.is_button = true;
                            config.is_clickable = true;
                            config.content_description = Some(label_for_semantics.clone());
                        });
                    let label = label.clone();
                    Box(
                        cell,
                        BoxSpec::default().content_alignment(Alignment::CENTER),
                        move || {
                            Text(label.clone(), Modifier::empty(), style.clone());
                        },
                    );
                }
            });

            // Swipe/tap surface across the whole control.
            let gesture = Modifier::empty()
                .size(Size::new(total_width, SEGMENT_HEIGHT))
                .pointer_input(selected, {
                    let on_select = Rc::clone(&on_select);
                    let lens_axis = Rc::clone(&lens_axis);
                    move |scope: PointerInputScope| {
                        let on_select = Rc::clone(&on_select);
                        let lens_axis = Rc::clone(&lens_axis);
                        async move {
                            scope
                                .await_pointer_event_scope(|await_scope| async move {
                                    let mut down_x = 0.0f32;
                                    let mut active_pointer = Option::<PointerId>::None;
                                    loop {
                                        let event = await_scope.await_pointer_event().await;
                                        match event.kind {
                                            PointerEventKind::Down if active_pointer.is_none() => {
                                                active_pointer = Some(event.id);
                                                down_x = event.position.x;
                                                pressed.set(true);
                                                lens_axis.begin(
                                                    segment_lens_left(
                                                        event.position.x,
                                                        segment_width,
                                                        count,
                                                    ),
                                                    event.time_ms,
                                                );
                                                default_haptics()
                                                    .perform(HapticFeedback::Selection);
                                                event.consume();
                                            }
                                            PointerEventKind::Move
                                                if active_pointer == Some(event.id) =>
                                            {
                                                lens_axis.move_to(
                                                    segment_lens_left(
                                                        event.position.x,
                                                        segment_width,
                                                        count,
                                                    ),
                                                    event.time_ms,
                                                );
                                                event.consume();
                                            }
                                            PointerEventKind::Up
                                                if active_pointer == Some(event.id) =>
                                            {
                                                active_pointer = None;
                                                pressed.set(false);
                                                let travelled =
                                                    (event.position.x - down_x).abs() > TAP_SLOP;
                                                let position = if travelled {
                                                    event.position.x
                                                } else {
                                                    down_x
                                                };
                                                let index =
                                                    ((position / segment_width).floor().max(0.0)
                                                        as usize)
                                                        .min(count - 1);
                                                lens_axis.release_to(
                                                    segment_width * index as f32,
                                                    event.time_ms,
                                                    LiquidMotion::glide(),
                                                );
                                                default_haptics()
                                                    .perform(HapticFeedback::ImpactLight);
                                                on_select(index);
                                                event.consume();
                                            }
                                            PointerEventKind::Cancel
                                                if active_pointer == Some(event.id) =>
                                            {
                                                active_pointer = None;
                                                pressed.set(false);
                                                lens_axis.release_to(
                                                    selected_x,
                                                    event.time_ms,
                                                    LiquidMotion::glide(),
                                                );
                                                event.consume();
                                            }
                                            _ => {}
                                        }
                                    }
                                })
                                .await;
                        }
                    }
                });
            Box(gesture, BoxSpec::default(), || {});

            // The interaction lens riding the indicator: a glass capsule that
            // magnifies the label under it and bulges along the travel.
            let lens_h = SEGMENT_HEIGHT + LENS_OVERFLOW;
            let response_stretch = |stretch: f32| 1.0 + (stretch - 1.0) * SEGMENTED_STRAIN_RESPONSE;
            let deformation_headroom = response_stretch(crate::dynamics::STRETCH_MAX)
                .max(1.0 / response_stretch(crate::dynamics::STRETCH_MIN));
            let node_w = (segment_width + 4.0) * LENS_LIFT_SCALE * deformation_headroom
                + crate::dynamics::BULGE_MAX
                + LENS_PAD * 2.0;
            let node_h = lens_h * LENS_LIFT_SCALE * deformation_headroom
                + crate::dynamics::BULGE_MAX
                + LENS_PAD * 2.0;
            let lens_for_layer = lens_progress;
            let physics_axis = Rc::clone(&lens_axis);
            let lens = Modifier::empty()
                // required_size: taller than the track; the fixed-height
                // host keeps the control's layout put.
                .required_size(Size::new(node_w, node_h))
                .offset(
                    (segment_width - node_w) * 0.5,
                    (SEGMENT_HEIGHT - node_h) * 0.5,
                )
                .graphics_layer(move || GraphicsLayer {
                    translation_x: lens_x,
                    alpha: (lens_for_layer.get() * 2.5).clamp(0.0, 1.0),
                    ..Default::default()
                })
                .glass_effect_with(
                    Glass::lens()
                        .shape(LiquidShape::Capsule)
                        .tint(Color::rgba(1.0, 1.0, 1.0, 0.05))
                        .highlight(0.52)
                        .no_clip(),
                    move || {
                        let grow = lens_for_layer.get().clamp(0.0, 1.2);
                        let lift = 1.0 + (LENS_LIFT_SCALE - 1.0) * grow;
                        let base_w = (segment_width + 4.0 * grow) * lift;
                        let base_h = (SEGMENT_HEIGHT + LENS_OVERFLOW * grow) * lift;
                        // Droplet law over the indicator ride
                        // (crate::dynamics): speed stretches the capsule
                        // along the travel, braking swells its front.
                        let pose = physics_axis.liquid_pose();
                        GlassDynamics {
                            activity: Some(grow.clamp(0.0, 1.0)),
                            morph: Some(GlassMorph {
                                node_size: (node_w, node_h),
                                primary: (node_w * 0.5, node_h * 0.5, base_w, base_h, -1.0),
                                shapes: Vec::new(),
                                glue: 0.0,
                                wobble_amplitude: 0.0,
                                wobble_phase: 0.0,
                                bulge_amplitude: pose.bulge_amplitude.min(4.0),
                                bulge_direction: pose.bulge_direction,
                                ellipse_blend: 0.0,
                                deformation: Some(
                                    crate::material::GlassDeformation::incompressible(
                                        pose.axis,
                                        response_stretch(pose.stretch),
                                    ),
                                ),
                            }),
                            ..Default::default()
                        }
                    },
                );
            Box(lens, BoxSpec::default(), || {});
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_position_is_the_clamped_lens_center() {
        let width = 100.0;
        assert_eq!(segment_lens_left(50.0, width, 3), 0.0);
        assert_eq!(segment_lens_left(150.0, width, 3), 100.0);
        assert_eq!(segment_lens_left(250.0, width, 3), 200.0);
        assert_eq!(segment_lens_left(-50.0, width, 3), 0.0);
        assert_eq!(segment_lens_left(400.0, width, 3), 200.0);
    }

    #[test]
    fn plain_indicator_stays_hidden_until_the_lens_is_almost_gone() {
        assert_eq!(plain_indicator_alpha(1.0), 0.0);
        assert_eq!(plain_indicator_alpha(0.5), 0.0);
        assert_eq!(plain_indicator_alpha(0.2), 0.0);
        assert!(plain_indicator_alpha(0.05) > 0.8);
        assert_eq!(plain_indicator_alpha(0.0), 1.0);
    }
}
