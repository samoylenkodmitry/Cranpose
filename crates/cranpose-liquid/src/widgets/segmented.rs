use std::rc::Rc;

use cranpose_animation::{animateFloatAsState, spring};
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
    material::{Glass, GlassDynamics, GlassMorph, GlassShadow, LiquidModifierExt, LiquidShape},
    motion::LiquidMotion,
    theme::{liquid_colors, liquid_typography},
    widgets::content_scope::ScopeContent,
};

const SEGMENT_HEIGHT: f32 = 41.0;
const TRACK_PADDING: f32 = 2.0;
const MARKER_WIDTH_FACTOR: f32 = 1.16;
const MARKER_REST_ACTIVITY: f32 = 0.55;
const MARKER_POKE_TOP: f32 = 1.5;
const MARKER_POKE_BOTTOM: f32 = 0.5;
const LENS_OVERFLOW: f32 = 8.0;
const LENS_ELLIPSE_BLEND: f32 = 0.55;
const LENS_WIDTH_LIFT_SCALE: f32 = 1.06;
const LENS_HEIGHT_LIFT_SCALE: f32 = 1.22;
const LENS_PAD: f32 = 10.0;
const SEGMENTED_STRAIN_RESPONSE: f32 = 0.30;
const TAP_SLOP: f32 = 4.0;

fn segment_lens_left(pointer_x: f32, segment_width: f32, count: usize) -> f32 {
    (pointer_x - segment_width * 0.5).clamp(0.0, segment_width * (count.saturating_sub(1)) as f32)
}

fn segmented_lens_base_size(segment_width: f32, progress: f32) -> Size {
    let progress = progress.clamp(0.0, 1.2);
    let rest_h = SEGMENT_HEIGHT + TRACK_PADDING * 2.0 + MARKER_POKE_TOP + MARKER_POKE_BOTTOM;
    let width_lift = 1.0 + (LENS_WIDTH_LIFT_SCALE - 1.0) * progress;
    let height_lift = 1.0 + (LENS_HEIGHT_LIFT_SCALE - 1.0) * progress;
    Size::new(
        (segment_width * MARKER_WIDTH_FACTOR + 2.0 * progress) * width_lift,
        (rest_h + LENS_OVERFLOW * progress) * height_lift,
    )
}

fn segmented_strain(stretch: f32) -> f32 {
    1.0 + (stretch - 1.0) * SEGMENTED_STRAIN_RESPONSE
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
#[allow(non_snake_case)]
fn SegmentLabel(label: String, selected: bool) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    let style = TextStyle {
        span_style: SpanStyle {
            color: Some(colors.label),
            font_size: cranpose_ui::text::TextUnit::Sp(17.0),
            font_weight: Some(if selected {
                FontWeight::MEDIUM
            } else {
                FontWeight::NORMAL
            }),
            ..typography.subheadline.span_style.clone()
        },
        ..typography.subheadline.clone()
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
#[allow(non_snake_case)]
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

    let track_height = SEGMENT_HEIGHT + TRACK_PADDING * 2.0;
    let track_fill = if colors.is_dark {
        colors.fill
    } else {
        Color::WHITE
    };
    let track = if colors.is_dark {
        Modifier::empty()
    } else {
        Modifier::empty().drop_shadow(
            cranpose_ui_graphics::LayerShape::Rounded(
                cranpose_ui_graphics::RoundedCornerShape::uniform(track_height * 0.5),
            ),
            |scope| {
                scope.radius = 6.0;
                scope.offset.y = 4.5;
                scope.color = Color::BLACK.with_alpha(0.10);
            },
        )
    };
    let track = track.height(track_height).draw_behind(move |scope| {
        scope.draw_round_rect(
            Brush::solid(track_fill),
            CornerRadii::uniform(track_height * 0.5),
        );
    });

    Box(track.then(modifier), BoxSpec::default(), move || {
        let segments = Rc::clone(&segments);
        let on_select = Rc::clone(&on_select);
        BoxWithConstraints(Modifier::empty().padding(TRACK_PADDING), move |scope| {
            let segments = Rc::clone(&segments);
            let on_select = Rc::clone(&on_select);
            let total_width = scope.constraints().max_width.max(1.0);
            let segment_width = total_width / count as f32;
            let selected_x = segment_width * selected as f32;
            let lens_axis = crate::motion::remember_liquid_drag_axis(selected_x);
            if !pressed.get() {
                lens_axis.settle_to(selected_x, LiquidMotion::glide());
            }
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

            let lens_settling = !lens_axis.is_dragging() && (lens_x - selected_x).abs() > 1.0;
            let lens_target = if pressed.get() || lens_settling {
                1.0
            } else {
                0.0
            };
            let lens_progress = animateFloatAsState(
                lens_target,
                if pressed.get() || lens_settling {
                    spring(0.9, 1400.0)
                } else {
                    spring(1.0, 170.0)
                },
                "segmented-lens",
            );

            Row(Modifier::empty(), RowSpec::default(), move || {
                for (index, segment) in segments.iter().enumerate() {
                    let is_selected = index == visual_index;
                    let description = segment.description.clone();
                    let cell = Modifier::empty()
                        .size(Size::new(segment_width, SEGMENT_HEIGHT))
                        .semantics(move |config| {
                            config.role = Some(SemanticsWidgetRole::Button);
                            config.is_clickable = true;
                            config.content_description = Some(description.clone());
                        });
                    let content = Rc::clone(&segment.content);
                    Box(
                        cell,
                        BoxSpec::default().content_alignment(Alignment::CENTER),
                        move || content(is_selected),
                    );
                }
            });

            let gesture = Modifier::empty()
                .size(Size::new(total_width, SEGMENT_HEIGHT))
                .pointer_input(selected, {
                    let on_select = Rc::clone(&on_select);
                    let lens_axis = Rc::clone(&lens_axis);
                    move |scope: PointerInputScope| {
                        let on_select = Rc::clone(&on_select);
                        let lens_axis = Rc::clone(&lens_axis);
                        crate::motion::liquid_lens_gesture(
                            scope,
                            crate::motion::LiquidLensGesture {
                                axis: lens_axis,
                                cell_width: segment_width,
                                count,
                                tap_slop: TAP_SLOP,
                                drag_left: Rc::new(move |x| {
                                    segment_lens_left(x, segment_width, count)
                                }),
                                rest_left: Rc::new(move |index| segment_width * index as f32),
                                selected,
                                on_pressed: Rc::new(move |down| pressed.set(down)),
                                on_touch: Rc::new(|_, _| {}),
                                on_select,
                            },
                        )
                    }
                });
            Box(gesture, BoxSpec::default(), || {});

            let raised_size = segmented_lens_base_size(segment_width, 1.2);
            let deformation_headroom = segmented_strain(crate::dynamics::STRETCH_MAX)
                .max(1.0 / segmented_strain(crate::dynamics::STRETCH_MIN));
            let node_w = raised_size.width * deformation_headroom
                + crate::dynamics::BULGE_MAX
                + LENS_PAD * 2.0;
            let node_h = raised_size.height * deformation_headroom
                + crate::dynamics::BULGE_MAX
                + LENS_PAD * 2.0;
            let lens_for_layer = lens_progress;
            let physics_axis = Rc::clone(&lens_axis);
            let lens = Modifier::empty()
                .required_size(Size::new(node_w, node_h))
                .offset(
                    (segment_width - node_w) * 0.5,
                    (SEGMENT_HEIGHT - node_h) * 0.5,
                )
                .graphics_layer(move || GraphicsLayer {
                    translation_x: lens_x,
                    alpha: 1.0,
                    ..Default::default()
                })
                .glass_effect_with(
                    Glass::lens()
                        .shape(LiquidShape::Capsule)
                        .tint(Color::rgba(0.0, 0.0, 0.0, 0.08))
                        .shadow_style(GlassShadow::new(
                            Color::BLACK.with_alpha(0.14),
                            12.0,
                            4.0,
                            -2.0,
                        ))
                        .rim_reflection(0.04)
                        .blur_radius(0.5)
                        .refraction_depth(1.0)
                        .refraction_curve(0.25)
                        .fold_depth(5.0)
                        .dispersion(0.85)
                        .highlight(0.04)
                        .lift(0.0)
                        .no_clip(),
                    move || {
                        let grow = lens_for_layer.get().clamp(0.0, 1.2);
                        let base_size = segmented_lens_base_size(segment_width, grow);
                        let pose = physics_axis.liquid_pose();
                        GlassDynamics {
                            activity: Some(
                                MARKER_REST_ACTIVITY
                                    + (1.0 - MARKER_REST_ACTIVITY) * grow.clamp(0.0, 1.0),
                            ),
                            press_depth: Some(
                                (0.12 + 0.30 * grow.clamp(0.0, 1.0) + 0.58 * pose.energy())
                                    .clamp(0.0, 1.0),
                            ),
                            morph: Some(GlassMorph {
                                node_size: (node_w, node_h),
                                primary: (
                                    node_w * 0.5,
                                    node_h * 0.5 - (MARKER_POKE_TOP - MARKER_POKE_BOTTOM) * 0.5,
                                    base_size.width,
                                    base_size.height,
                                    -1.0,
                                ),
                                shapes: Vec::new(),
                                glue: 0.0,
                                wobble_amplitude: 0.0,
                                wobble_phase: 0.0,
                                bulge_amplitude: pose.bulge_amplitude.min(4.0),
                                bulge_direction: pose.bulge_direction,
                                ellipse_blend: LENS_ELLIPSE_BLEND,
                                deformation: Some(
                                    crate::material::GlassDeformation::incompressible(
                                        pose.axis,
                                        segmented_strain(pose.stretch),
                                    ),
                                ),
                                zoom_anchor: (0.0, 0.0),
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
    fn raised_lens_lifts_in_depth_without_becoming_a_wide_worm() {
        let resting = segmented_lens_base_size(120.0, 0.0);
        let raised = segmented_lens_base_size(120.0, 1.0);
        assert_eq!(resting.width, 120.0 * MARKER_WIDTH_FACTOR);
        assert!(resting.height > SEGMENT_HEIGHT + TRACK_PADDING * 2.0);
        assert!(raised.width < resting.width * 1.10);
        assert!(raised.height > resting.height * 1.20);
        assert!(segmented_strain(crate::dynamics::STRETCH_MAX) < 1.20);
    }

    #[test]
    fn a_scope_records_what_each_segment_announces() {
        let drawn = Rc::new(std::cell::Cell::new(0u32));
        let counted = Rc::clone(&drawn);
        let segments = collect_segments(|scope| {
            scope.segment("Sending");
            scope.segment_content("Received", move |selected| {
                counted.set(counted.get() + u32::from(selected));
            });
        });

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].description, "Sending");
        assert_eq!(segments[1].description, "Received");
        assert_eq!(drawn.get(), 0);
        (segments[1].content)(true);
        assert_eq!(drawn.get(), 1);
    }

    #[test]
    fn a_control_with_no_segments_declares_none() {
        assert!(collect_segments(|_| {}).is_empty());
    }
}
