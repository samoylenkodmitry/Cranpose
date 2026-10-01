use std::rc::Rc;

use cranpose_animation::spring;
use cranpose_core::{mutableStateOf, remember};
use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, PointerInputScope, SemanticsWidgetRole, Size,
    text::{FontWeight, SpanStyle, TextStyle},
    widgets::{Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Row, RowSpec, Text},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, GraphicsLayer};
use cranpose_ui_layout::Alignment;

use crate::{
    material::{GlassDynamics, GlassMorph, LiquidModifierExt},
    motion::LiquidMotion,
    theme::{liquid_colors, liquid_typography},
    widgets::content_scope::ScopeContent,
};

const SEGMENT_HEIGHT: f32 = 32.0;
const TRACK_PADDING: f32 = 2.0;
const LENS_PAD: f32 = 10.0;
const TAP_SLOP: f32 = 4.0;

fn segment_lens_left(pointer_x: f32, segment_width: f32, count: usize) -> f32 {
    (pointer_x - segment_width * 0.5).clamp(0.0, segment_width * (count.saturating_sub(1)) as f32)
}

fn segmented_lens_reading(
    lens_axis: &crate::motion::LiquidDragAxis,
    pressed: bool,
    selected: usize,
    segment_width: f32,
    count: usize,
) -> usize {
    let selected_x = segment_width * selected as f32;
    let lens_x = lens_axis.value();
    crate::motion::liquid_visual_index(
        selected,
        lens_x,
        segment_width,
        count,
        crate::motion::liquid_axis_owns_visual_selection(
            pressed,
            lens_x,
            selected_x,
            segment_width,
        ),
    )
}

fn segmented_lens_base_size(segment_width: f32, progress: f32) -> Size {
    let progress = progress.clamp(-0.1, 1.2);
    let rest_h = SEGMENT_HEIGHT - TRACK_PADDING * 2.0;
    Size::new(
        segment_width - TRACK_PADDING * 2.0 + 24.0 * progress,
        rest_h + 16.0 * progress,
    )
}

struct LiquidSegment {
    description: String,
    content: Rc<dyn Fn(bool)>,
}

/// The scope a segmented control's content is declared in.
///
/// Each call adds one equal-width segment, in the order it is made, which is
/// also the order the indices passed to `on_select` count in.
pub struct LiquidSegmentedControlScope {
    segments: ScopeContent<LiquidSegment>,
}

impl LiquidSegmentedControlScope {
    /// A segment showing `label`, styled by the control: the selected one is
    /// told apart by weight, never by dimming the rest.
    pub fn segment(&self, label: impl Into<String>) {
        let label = label.into();
        let text = label.clone();
        self.segment_content(label, move |selected| {
            SegmentLabel(text.clone(), selected);
        });
    }

    /// A segment the caller draws.
    ///
    /// `selected` says whether this is the active segment, so content can
    /// respond the way the built-in label's weight does. `description` is what
    /// accessibility announces, which content alone cannot supply.
    pub fn segment_content(
        &self,
        description: impl Into<String>,
        content: impl Fn(bool) + 'static,
    ) {
        self.segments.push(LiquidSegment {
            description: description.into(),
            content: Rc::new(content),
        });
    }
}

fn collect_segments(content: impl FnOnce(&LiquidSegmentedControlScope)) -> Vec<LiquidSegment> {
    ScopeContent::collect(|segments| LiquidSegmentedControlScope { segments }, content)
}

#[composable]
fn SegmentLabel(label: String, selected: bool) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    let style = TextStyle {
        span_style: SpanStyle {
            color: Some(colors.label),
            font_size: cranpose_ui::text::TextUnit::Sp(13.0),
            font_weight: Some(if selected {
                FontWeight::MEDIUM
            } else {
                FontWeight::NORMAL
            }),
            ..typography.subheadline.span_style.clone()
        },
        ..typography.subheadline
    };
    Text(label, Modifier::empty(), style);
}

/// A segmented control. `content` declares equal-width segments; `selected` is
/// the active index; `on_select` receives the committed index. Segments tap AND
/// swipe: dragging slides the indicator with the finger as a glass lens.
///
/// ```rust,ignore
/// LiquidSegmentedControl(Modifier::empty().width(310.0), selected.get(), move |index| {
///     selected.set(index)
/// }, |scope| {
///     scope.segment("Receiving");
///     scope.segment("Sending");
///     scope.segment("Errored");
/// });
/// ```
#[composable]
pub fn LiquidSegmentedControl(
    modifier: Modifier,
    selected: usize,
    on_select: impl Fn(usize) + 'static,
    content: impl FnOnce(&LiquidSegmentedControlScope),
) {
    let colors = liquid_colors();
    let segments = collect_segments(content);
    let count = segments.len().max(1);
    let selected = selected.min(count - 1);
    let on_select: Rc<dyn Fn(usize)> = Rc::new(on_select);
    let segments = Rc::new(segments);

    let pressed = remember(|| mutableStateOf(false)).with(|s| *s);
    let contact = super::control_motion::remember_control_contact(
        super::control_motion::ControlContactKind::Segment,
    );
    let (lens_progress, material_progress) = contact.states();

    let track_height = SEGMENT_HEIGHT;
    let track_fill = if colors.is_dark {
        Color::from_rgb_u8(28, 28, 31)
    } else {
        Color::from_rgb_u8(238, 238, 239)
    };
    let marker_fill = if colors.is_dark {
        Color::from_rgb_u8(90, 90, 95)
    } else {
        Color::WHITE
    };
    let track = Modifier::empty()
        .height(track_height)
        .draw_behind(move |scope| {
            scope.draw_round_rect(
                Brush::solid(track_fill),
                CornerRadii::uniform(track_height * 0.5),
            );
        });

    Box(modifier.then(track), BoxSpec::default(), move || {
        let segments = Rc::clone(&segments);
        let on_select = Rc::clone(&on_select);
        let contact = Rc::clone(&contact);
        BoxWithConstraints(Modifier::empty(), move |scope| {
            let segments = Rc::clone(&segments);
            let on_select = Rc::clone(&on_select);
            let total_width = scope.constraints().max_width.max(1.0);
            let segment_width = total_width / count as f32;
            let selected_x = segment_width * selected as f32;
            let lens_axis =
                crate::motion::remember_liquid_follow_axis(selected_x, spring(1.0, 750.0));
            let shape =
                super::lens_motion::remember_lens_shape(super::lens_motion::LensShapeKind::Segment);
            if !pressed.get() {
                lens_axis.settle_to(selected_x, LiquidMotion::glide());
            }
            let visual_index = {
                let lens_axis = Rc::clone(&lens_axis);
                cranpose_core::derivedStateOf(move || {
                    segmented_lens_reading(
                        &lens_axis,
                        pressed.get(),
                        selected,
                        segment_width,
                        count,
                    )
                })
                .get()
            };

            let gesture = Modifier::empty()
                .size(Size::new(total_width, SEGMENT_HEIGHT))
                .pointer_input(selected, {
                    let on_select = Rc::clone(&on_select);
                    let lens_axis = Rc::clone(&lens_axis);
                    let contact = Rc::clone(&contact);
                    move |scope: PointerInputScope| {
                        let on_select = Rc::clone(&on_select);
                        let lens_axis = Rc::clone(&lens_axis);
                        let contact = Rc::clone(&contact);
                        crate::motion::liquid_lens_gesture(
                            scope,
                            crate::motion::LiquidLensGesture {
                                axis: lens_axis,
                                cell_width: segment_width,
                                cell_offset: 0.0,
                                count,
                                tap_slop: TAP_SLOP,
                                drag_left: Rc::new(move |x| {
                                    segment_lens_left(x, segment_width, count)
                                }),
                                rest_left: Rc::new(move |index| segment_width * index as f32),
                                selected,
                                on_pressed: Rc::new(move |down, time| {
                                    pressed.set(down);
                                    contact.pressed(down, time);
                                }),
                                on_touch: Rc::new(|_, _| {}),
                                on_select,
                            },
                        )
                    }
                });

            let Size {
                width: node_w,
                height: node_h,
            } = super::control_lens::node_size(
                segmented_lens_base_size(segment_width, 0.0),
                segmented_lens_base_size(segment_width, 1.0),
                LENS_PAD,
            );
            let lens_for_layer = lens_progress;
            let layer_axis = Rc::clone(&lens_axis);
            let physics_axis = Rc::clone(&lens_axis);
            let content_axis = Rc::clone(&lens_axis);
            let content_shape = Rc::clone(&shape);
            let foreground_shape = Rc::clone(&shape);
            let foreground_axis = Rc::clone(&lens_axis);
            let lens = Modifier::empty()
                .required_size(Size::new(node_w, node_h))
                .offset(
                    (segment_width - node_w) * 0.5,
                    (SEGMENT_HEIGHT - node_h) * 0.5,
                )
                .graphics_layer(move || GraphicsLayer {
                    translation_x: layer_axis.value(),
                    alpha: 1.0,
                    ..Default::default()
                })
                .glass_effect_with(
                    super::control_material::material(
                        super::control_motion::ControlContactKind::Segment,
                    ),
                    move || {
                        let grow = lens_for_layer.get().clamp(-0.1, 1.2);
                        let base_size = segmented_lens_base_size(segment_width, grow);
                        let projection = shape.projection(physics_axis.value());
                        let material = material_progress.get().max(0.0);
                        GlassDynamics {
                            optical_projection: Some(projection),
                            resting_tint: Some(marker_fill),
                            morph: Some(GlassMorph {
                                node_size: (node_w, node_h),
                                primary: (
                                    node_w * 0.5,
                                    node_h * 0.5,
                                    base_size.width,
                                    base_size.height,
                                    -1.0,
                                ),
                                shapes: Vec::new(),
                                glue: 0.0,
                                wobble_amplitude: 0.0,
                                wobble_phase: 0.0,
                                bulge_amplitude: 0.0,
                                bulge_direction: 0.0,
                                ellipse_blend: 0.0,
                                capsule_smoothing_dp: 0.0,
                                deformation: None,
                                zoom_anchor: (0.0, 0.0),
                            }),
                            ..super::control_material::dynamics(
                                super::control_motion::ControlContactKind::Segment,
                                grow,
                                material,
                            )
                        }
                    },
                );
            Box(lens, BoxSpec::default(), || {});
            let semantic_selection = Rc::clone(&on_select);
            Row(
                Modifier::empty()
                    .selectable_group()
                    .graphics_layer(move || {
                        let position = content_axis.value();
                        GraphicsLayer {
                            render_effect: super::control_material::content_effect(
                                Size::new(total_width, SEGMENT_HEIGHT),
                                (position + segment_width * 0.5, SEGMENT_HEIGHT * 0.5),
                                segmented_lens_base_size(segment_width, lens_progress.get()),
                                content_shape.projection(position),
                                material_progress.get().max(0.0),
                            ),
                            ..Default::default()
                        }
                    }),
                RowSpec::default(),
                move || {
                    for (index, segment) in segments.iter().enumerate() {
                        let is_selected = index == visual_index;
                        let description = segment.description.clone();
                        let on_select = Rc::clone(&semantic_selection);
                        let cell = Modifier::empty()
                            .size(Size::new(segment_width, SEGMENT_HEIGHT))
                            .stable_semantics(super::selection::selection_semantics(
                                description,
                                SemanticsWidgetRole::RadioButton,
                                index,
                                selected,
                                on_select,
                            ))
                            .focusable();
                        let content = Rc::clone(&segment.content);
                        Box(
                            cell,
                            BoxSpec::default().content_alignment(Alignment::CENTER),
                            move || content(is_selected),
                        );
                    }
                },
            );
            Box(
                Modifier::empty()
                    .required_size(Size::new(node_w, node_h))
                    .offset(
                        (segment_width - node_w) * 0.5,
                        (SEGMENT_HEIGHT - node_h) * 0.5,
                    )
                    .graphics_layer(move || {
                        let position = foreground_axis.value();
                        GraphicsLayer {
                            translation_x: position,
                            backdrop_effect: super::control_material::foreground_effect(
                                Size::new(node_w, node_h),
                                segmented_lens_base_size(segment_width, lens_progress.get()),
                                foreground_shape.projection(position),
                                material_progress.get().max(0.0),
                            ),
                            ..Default::default()
                        }
                    }),
                BoxSpec::default(),
                || {},
            );
            Box(gesture, BoxSpec::default(), || {});
        });
    });
}

#[cfg(test)]
#[path = "tests/segmented_tests.rs"]
mod tests;
