//! Segmented control with a liquid selection blob: the glass pill behind the
//! selected segment runs its leading and trailing edges on different springs,
//! so it stretches like a droplet while traveling and settles round. Touching
//! it lifts the indicator into a magnifying glass lens that chases the finger
//! across the segments (the reference control swipes, it doesn't just tap).

use crate::material::{Glass, GlassDynamics, GlassMorph, LiquidModifierExt, LiquidShape};
use crate::motion::LiquidMotion;
use crate::theme::{liquid_colors, liquid_typography};
use cranpose_animation::{animateFloatAsState, spring};
use cranpose_core::{mutableStateOf, remember};
use cranpose_foundation::PointerEventKind;
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
/// Glass node span beyond the lens shape (rim glow + bulge live here).
const LENS_PAD: f32 = 10.0;
/// Pointer travel below this is a tap, not a swipe.
const TAP_SLOP: f32 = 4.0;

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

    // Some(finger x) while dragging; the indicator target follows it.
    let drag_x = remember(|| mutableStateOf(Option::<f32>::None)).with(|s| *s);
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

            // While dragging, the indicator target is the finger-centered
            // segment position (clamped inside the track); at rest it is the
            // committed segment. Both edges keep their droplet springs, so a
            // release settles from the drag with velocity preserved.
            let indicator_target = match drag_x.get() {
                Some(x) => {
                    (x - segment_width * 0.5).clamp(0.0, segment_width * (count as f32 - 1.0))
                }
                None => segment_width * selected as f32,
            };
            let leading = animateFloatAsState(
                indicator_target,
                LiquidMotion::blob_leading(),
                "segmented-leading",
            );
            let trailing = animateFloatAsState(
                indicator_target + segment_width,
                LiquidMotion::blob_trailing(),
                "segmented-trailing",
            );

            // Lens presence: up while touched, lingering decay on release
            // (the indicator stays liquid through the settle flight).
            let lens_settling = (leading.get() - indicator_target).abs() > 1.0;
            let lens_target = if pressed.get() || (drag_x.get().is_none() && lens_settling) {
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
                        alpha: (1.0 - lens_for_indicator.get()).clamp(0.0, 1.0),
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
                    let is_selected = index == selected;
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
                .pointer_input((), {
                    let on_select = Rc::clone(&on_select);
                    move |scope: PointerInputScope| {
                        let on_select = Rc::clone(&on_select);
                        async move {
                            scope
                                .await_pointer_event_scope(|await_scope| async move {
                                    let mut down_x = 0.0f32;
                                    let mut active = false;
                                    loop {
                                        let event = await_scope.await_pointer_event().await;
                                        if event.id != 0 {
                                            continue;
                                        }
                                        match event.kind {
                                            PointerEventKind::Down => {
                                                active = true;
                                                down_x = event.position.x;
                                                pressed.set(true);
                                                drag_x.set(Some(event.position.x));
                                                default_haptics()
                                                    .perform(HapticFeedback::Selection);
                                                event.consume();
                                            }
                                            PointerEventKind::Move if active => {
                                                drag_x.set(Some(event.position.x));
                                                event.consume();
                                            }
                                            PointerEventKind::Up | PointerEventKind::Cancel
                                                if active =>
                                            {
                                                active = false;
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
                                                drag_x.set(None);
                                                default_haptics()
                                                    .perform(HapticFeedback::ImpactLight);
                                                on_select(index);
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
            if lens_progress.get() > 0.01 {
                let lens_h = SEGMENT_HEIGHT + LENS_OVERFLOW;
                let node_w = segment_width + LENS_PAD * 2.0;
                let node_h = lens_h + LENS_PAD * 2.0;
                let lens_for_layer = lens_progress;
                let dynamics = crate::dynamics::remember_liquid_dynamics();
                let lens = Modifier::empty()
                    // required_size: taller than the track; the fixed-height
                    // host keeps the control's layout put.
                    .required_size(Size::new(node_w, node_h))
                    .offset(-LENS_PAD, (SEGMENT_HEIGHT - node_h) * 0.5)
                    .graphics_layer(move || GraphicsLayer {
                        translation_x: leading.get(),
                        alpha: (lens_for_layer.get() * 2.5).clamp(0.0, 1.0),
                        ..Default::default()
                    })
                    .glass_effect_with(
                        Glass::lens().shape(LiquidShape::Capsule).no_clip(),
                        move || {
                            let grow = lens_for_layer.get().clamp(0.0, 1.2);
                            let base_w = segment_width + 4.0 * grow;
                            let base_h = SEGMENT_HEIGHT + LENS_OVERFLOW * grow;
                            // Droplet law over the indicator ride
                            // (crate::dynamics): speed stretches the capsule
                            // along the travel, braking swells its front.
                            let pose = dynamics.update((leading.get(), 0.0));
                            let (w, h) = pose.size(base_w, base_h);
                            GlassDynamics {
                                morph: Some(GlassMorph {
                                    node_size: (node_w, node_h),
                                    primary: (node_w * 0.5, node_h * 0.5, w, h, -1.0),
                                    shapes: Vec::new(),
                                    glue: 0.0,
                                    wobble_amplitude: 0.0,
                                    wobble_phase: 0.0,
                                    bulge_amplitude: pose.bulge_amplitude.min(6.0),
                                    bulge_direction: pose.bulge_direction,
                                }),
                                magnify_boost: 0.18,
                                ..Default::default()
                            }
                        },
                    );
                Box(lens, BoxSpec::default(), || {});
            }
        });
    });
}
