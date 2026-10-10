use std::rc::Rc;

use cranpose_animation::{Lerp, spring};
use cranpose_core::{State, mutableStateOf, remember};
use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, PointerInputScope, SemanticsWidgetRole, SharedText, Size,
    text::{FontWeight, TextStyle},
    widgets::{Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Row, RowSpec, Text},
};
use cranpose_ui_graphics::{Brush, Color, ColorFilter, CornerRadii, GraphicsLayer};
use cranpose_ui_layout::Alignment;

use crate::{
    material::{GlassDynamics, GlassMorph, LiquidModifierExt},
    motion::LiquidMotion,
    theme::{LiquidSegmentedStyle, liquid_segmented_style},
    widgets::content_scope::ScopeContent,
};

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

fn segmented_lens_base_size(segment_width: f32, height: f32, progress: f32) -> Size {
    let progress = progress.clamp(-0.1, 1.2);
    let rest_h = height - TRACK_PADDING * 2.0;
    Size::new(
        segment_width - TRACK_PADDING * 2.0 + 24.0 * progress,
        rest_h + 16.0 * progress,
    )
}

enum LiquidSegment {
    /// A label the control draws in its style.
    Label(SharedText),
    /// Content the caller draws, and what accessibility announces for it.
    Content {
        description: String,
        content: Rc<dyn Fn(bool)>,
    },
}

impl LiquidSegment {
    fn description(&self) -> String {
        match self {
            Self::Label(label) => label.to_string(),
            Self::Content { description, .. } => description.clone(),
        }
    }
}

/// The scope a segmented control's content is declared in.
///
/// Each call adds one equal-width segment, in the order it is made, which is
/// also the order the indices passed to `on_select` count in.
pub struct LiquidSegmentedControlScope {
    segments: ScopeContent<LiquidSegment>,
}

impl LiquidSegmentedControlScope {
    /// A segment showing `label` in the control's
    /// [`LiquidSegmentedStyle`]: the selected one is told apart by weight
    /// and by the selected label color, never by dimming the rest.
    pub fn segment(&self, label: impl Into<String>) {
        self.segments
            .push(LiquidSegment::Label(SharedText::from(label.into())));
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
        self.segments.push(LiquidSegment::Content {
            description: description.into(),
            content: Rc::new(content),
        });
    }
}

fn collect_segments(content: impl FnOnce(&LiquidSegmentedControlScope)) -> Vec<LiquidSegment> {
    ScopeContent::collect(|segments| LiquidSegmentedControlScope { segments }, content)
}

/// The labels' text, worked out once for every segment of a control.
struct SegmentLabels {
    text: TextStyle,
    selected_text: TextStyle,
    /// The chosen label's color at rest and while the lens is held, when
    /// they differ.
    held: Option<(Color, Color)>,
}

impl SegmentLabels {
    fn new(style: &LiquidSegmentedStyle) -> Self {
        let mut text = style.text.clone();
        text.span_style.color = Some(style.label);
        let mut selected_text = style.text.clone();
        let weight = selected_text
            .span_style
            .font_weight
            .unwrap_or(FontWeight::NORMAL);
        selected_text.span_style.font_weight = Some(FontWeight((weight.0 + 100).min(900)));
        selected_text.span_style.color = Some(style.selected_label);
        Self {
            text,
            selected_text,
            held: (style.selected_label != style.label)
                .then_some((style.selected_label, style.label)),
        }
    }

    /// Draws `label`. The chosen label follows the indicator: as a held lens
    /// clears to glass, the label moves to the color of the others, which
    /// reads on the track.
    fn show(&self, label: &SharedText, selected: bool, lift: State<f32>) {
        let modifier = match self.held.filter(|_| selected) {
            Some((rest, held)) => Modifier::empty().graphics_layer(move || {
                let lift = lift.get().clamp(0.0, 1.0);
                GraphicsLayer {
                    color_filter: (lift > 0.0).then(|| ColorFilter::tint(rest.lerp(&held, lift))),
                    ..Default::default()
                }
            }),
            None => Modifier::empty(),
        };
        let text = if selected {
            &self.selected_text
        } else {
            &self.text
        };
        Text(label.clone(), modifier, text.clone());
    }
}

/// A segmented control. `content` declares equal-width segments; `selected` is
/// the active index; `on_select` receives the committed index. Segments tap AND
/// swipe: dragging slides the indicator with the finger as a glass lens. The
/// look comes from [`liquid_segmented_style`].
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
    let style = liquid_segmented_style();
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

    let height = style.height;
    let track_fill = style.track;
    let marker_fill = style.indicator;
    let labels = Rc::new(SegmentLabels::new(&style));
    let track = Modifier::empty().height(height).draw_behind(move |scope| {
        scope.draw_round_rect(Brush::solid(track_fill), CornerRadii::uniform(height * 0.5));
    });

    Box(modifier.then(track), BoxSpec::default(), move || {
        let segments = Rc::clone(&segments);
        let labels = Rc::clone(&labels);
        let on_select = Rc::clone(&on_select);
        let contact = Rc::clone(&contact);
        BoxWithConstraints(Modifier::empty(), move |scope| {
            let segments = Rc::clone(&segments);
            let labels = Rc::clone(&labels);
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
                .size(Size::new(total_width, height))
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
                segmented_lens_base_size(segment_width, height, 0.0),
                segmented_lens_base_size(segment_width, height, 1.0),
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
                .offset((segment_width - node_w) * 0.5, (height - node_h) * 0.5)
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
                        let base_size = segmented_lens_base_size(segment_width, height, grow);
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
                                Size::new(total_width, height),
                                (position + segment_width * 0.5, height * 0.5),
                                segmented_lens_base_size(
                                    segment_width,
                                    height,
                                    lens_progress.get(),
                                ),
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
                        let description = segment.description();
                        let on_select = Rc::clone(&semantic_selection);
                        let cell = Modifier::empty()
                            .size(Size::new(segment_width, height))
                            .stable_semantics(super::selection::selection_semantics(
                                description,
                                SemanticsWidgetRole::RadioButton,
                                index,
                                selected,
                                on_select,
                            ))
                            .focusable();
                        let (drawn, labels) = (Rc::clone(&segments), Rc::clone(&labels));
                        Box(
                            cell,
                            BoxSpec::default().content_alignment(Alignment::CENTER),
                            move || match &drawn[index] {
                                LiquidSegment::Label(label) => {
                                    labels.show(label, is_selected, material_progress);
                                }
                                LiquidSegment::Content { content, .. } => content(is_selected),
                            },
                        );
                    }
                },
            );
            Box(
                Modifier::empty()
                    .required_size(Size::new(node_w, node_h))
                    .offset((segment_width - node_w) * 0.5, (height - node_h) * 0.5)
                    .graphics_layer(move || {
                        let position = foreground_axis.value();
                        GraphicsLayer {
                            translation_x: position,
                            backdrop_effect: super::control_material::foreground_effect(
                                Size::new(node_w, node_h),
                                segmented_lens_base_size(
                                    segment_width,
                                    height,
                                    lens_progress.get(),
                                ),
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
