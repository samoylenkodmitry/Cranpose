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
use cranpose_macros::composable;
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
/// Finger-halo circle diameter over the pill height while touched
/// (segmented-drag f_025: the circle spans ~1.4x the track height).
const SEGMENT_HALO_FACTOR: f32 = 1.4;
/// How far the interaction lens pokes past the track vertically.
const LENS_OVERFLOW: f32 = 8.0;
/// Touch raises the whole optical body before directional deformation. This
/// preserves the control's volume without letting maximum horizontal strain
/// squash the lens below the track height.
const LENS_WIDTH_LIFT_SCALE: f32 = 1.06;
const LENS_HEIGHT_LIFT_SCALE: f32 = 1.22;
/// Glass node span beyond the lens shape (rim glow + bulge live here).
const LENS_PAD: f32 = 10.0;
/// A segmented selection stays recognizably one cell wide while its surface
/// carries the shared incompressible fluid strain.
const SEGMENTED_STRAIN_RESPONSE: f32 = 0.18;
/// Pointer travel below this is a tap, not a swipe.
const TAP_SLOP: f32 = 4.0;

/// The white pill dissolves with TRAVEL, never with the press itself: the
/// reference keeps the pill (and the label through it) visible for the whole
/// pressed dwell (tap-flight f_0000..f_0383), dissolves it at the origin as
/// the lens departs, and re-materializes it under the destination as the
/// lens arrives. Keying it on lens rise painted a white body over the label
/// at the press instant.
fn plain_indicator_alpha(travel_fraction: f32) -> f32 {
    ((0.45 - travel_fraction.abs()) / 0.30).clamp(0.0, 1.0)
}

fn segment_lens_left(pointer_x: f32, segment_width: f32, count: usize) -> f32 {
    (pointer_x - segment_width * 0.5).clamp(0.0, segment_width * (count.saturating_sub(1)) as f32)
}

fn segmented_lens_base_size(segment_width: f32, progress: f32) -> Size {
    let progress = progress.clamp(0.0, 1.2);
    let width_lift = 1.0 + (LENS_WIDTH_LIFT_SCALE - 1.0) * progress;
    let height_lift = 1.0 + (LENS_HEIGHT_LIFT_SCALE - 1.0) * progress;
    Size::new(
        (segment_width + 4.0 * progress) * width_lift,
        (SEGMENT_HEIGHT + LENS_OVERFLOW * progress) * height_lift,
    )
}

fn segmented_strain(stretch: f32) -> f32 {
    1.0 + (stretch - 1.0) * SEGMENTED_STRAIN_RESPONSE
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
                if pressed.get() || lens_settling {
                    // A tap must FLY the raised lens (reference tap-flight:
                    // ~220ms crossing with the glyphs warping through it) —
                    // the rise has to win the race against the flight.
                    spring(0.9, 1400.0)
                } else {
                    spring(1.0, 170.0)
                },
                "segmented-lens",
            );
            // The finger halo: while touched, a circle rides the pill center
            // and the union silhouette bulges above/below the track
            // (segmented-drag f_025/f_055 — visible through transit and a
            // parked hold, gone at rest).
            let halo_presence = animateFloatAsState(
                if pressed.get() { 1.0 } else { 0.0 },
                spring(1.0, 900.0),
                "segmented-halo",
            );

            let indicator_color = if colors.is_dark {
                Color::from_rgba_u8(90, 90, 96, 240)
            } else {
                Color::WHITE
            };
            let indicator_axis = Rc::clone(&lens_axis);
            // Direct manipulation: while the lens gesture owns the control
            // (pressed, or still settling after release) the white pill IS
            // the dragged body — it snaps to the lens's nearest cell and
            // dissolves/materializes with the lens's distance to that cell
            // (reference: pill visible through the pressed dwell AND parked
            // mid-drag holds, gone mid-gap, re-forming under the landing
            // cell). Controlled-state changes keep the blob springs.
            let lens_engaged = pressed.get() || lens_settling;
            let visual_cell_x = segment_width * visual_index as f32;
            let indicator = Modifier::empty()
                .size(Size::new(segment_width, SEGMENT_HEIGHT))
                .graphics_layer(move || {
                    let lead = leading.get();
                    let trail = trailing.get().max(lead + 1.0);
                    if lens_engaged {
                        let travel =
                            (indicator_axis.value() - visual_cell_x) / segment_width.max(1.0);
                        GraphicsLayer {
                            translation_x: visual_cell_x,
                            alpha: plain_indicator_alpha(travel),
                            ..Default::default()
                        }
                    } else {
                        GraphicsLayer {
                            translation_x: lead,
                            scale_x: ((trail - lead) / segment_width.max(1.0)).max(0.01),
                            alpha: 1.0,
                            // Scale from the leading edge so translation
                            // stays exact.
                            transform_origin: cranpose_ui_graphics::TransformOrigin {
                                pivot_fraction_x: 0.0,
                                pivot_fraction_y: 0.5,
                            },
                            ..Default::default()
                        }
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
                            // Every reference label reads near-black
                            // (tap-flight/drag strips) — selection is told
                            // by weight and the pill, never by dimming the
                            // unselected cells.
                            color: Some(colors.label),
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

            // Swipe/tap surface across the whole control: the shared lens
            // gesture (crate::motion) with the segment clamp rules.
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

            // The interaction lens riding the indicator: a glass capsule that
            // magnifies the label under it and bulges along the travel.
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
                    // The reference lens body is nearly invisible on the
                    // white bar — no readable outline, no tint; it shows
                    // itself only through strong glyph refraction and
                    // saturated RGB fringes at the strokes (segmented-drag
                    // sheet, T 500/2000ms).
                    Glass::lens()
                        .shape(LiquidShape::Capsule)
                        .tint(Color::rgba(1.0, 1.0, 1.0, 0.02))
                        // The reference body is invisible inside the track:
                        // no drop shadow under the riding lens.
                        .shadow(false)
                        .rim_reflection(0.12)
                        // The full continuous wcKSRD dome (example/
                        // shaders.txt): glyph warps and rim replay come from
                        // ONE mapping; soft interior per the original's blur.
                        .blur_radius(0.5)
                        .refraction_depth(1.0)
                        .refraction_curve(0.25)
                        // The reference fold is a DEEP band: glyphs crossing
                        // the rim collapse into a dense spectral blob
                        // (segmented-drag f_011), not a thin outline.
                        .fold_depth(5.0)
                        .dispersion(0.85)
                        .highlight(0.04)
                        .lift(0.0)
                        .no_clip(),
                    move || {
                        let grow = lens_for_layer.get().clamp(0.0, 1.2);
                        let base_size = segmented_lens_base_size(segment_width, grow);
                        // Droplet law over the indicator ride
                        // (crate::dynamics): speed stretches the capsule
                        // along the travel, braking swells its front.
                        let pose = physics_axis.liquid_pose();
                        // The halo circle grows concentric with the pill;
                        // below the track height it hides inside the capsule,
                        // so presence needs no alpha of its own.
                        let halo = halo_presence.get().clamp(0.0, 1.0);
                        let halo_diameter = base_size.height * SEGMENT_HALO_FACTOR * halo;
                        let shapes = if halo_diameter > base_size.height {
                            vec![(
                                node_w * 0.5,
                                node_h * 0.5,
                                halo_diameter,
                                halo_diameter,
                                -1.0,
                            )]
                        } else {
                            Vec::new()
                        };
                        GlassDynamics {
                            activity: Some(grow.clamp(0.0, 1.0)),
                            // The lens paints NO body of its own: the white
                            // pill lives BELOW the labels (plain indicator)
                            // and stays visible through the pressed dwell —
                            // a white resting_tint here sat ABOVE the labels
                            // and flashed an opaque capsule over "Errored"
                            // at the press instant.
                            morph: Some(GlassMorph {
                                node_size: (node_w, node_h),
                                primary: (
                                    node_w * 0.5,
                                    node_h * 0.5,
                                    base_size.width,
                                    base_size.height,
                                    -1.0,
                                ),
                                shapes,
                                glue: 0.0,
                                wobble_amplitude: 0.0,
                                wobble_phase: 0.0,
                                bulge_amplitude: pose.bulge_amplitude.min(4.0),
                                bulge_direction: pose.bulge_direction,
                                ellipse_blend: 0.0,
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
    fn plain_indicator_dissolves_with_travel_not_with_the_press() {
        // Pressed dwell (no travel): the pill and its label stay visible.
        assert_eq!(plain_indicator_alpha(0.0), 1.0);
        assert_eq!(plain_indicator_alpha(0.10), 1.0);
        // Departing the origin: dissolved by mid-cell.
        assert_eq!(plain_indicator_alpha(0.5), 0.0);
        assert_eq!(plain_indicator_alpha(1.0), 0.0);
        // Arriving works from either side.
        assert_eq!(plain_indicator_alpha(-0.10), 1.0);
        assert_eq!(plain_indicator_alpha(-0.5), 0.0);
        // The fade band between dwell and gone is continuous.
        assert!(plain_indicator_alpha(0.3) > 0.4);
    }

    #[test]
    fn raised_lens_lifts_in_depth_without_becoming_a_wide_worm() {
        let resting = segmented_lens_base_size(120.0, 0.0);
        let raised = segmented_lens_base_size(120.0, 1.0);
        assert_eq!(resting, Size::new(120.0, SEGMENT_HEIGHT));
        assert!(raised.width < resting.width * 1.10);
        assert!(raised.height > resting.height * 1.45);
        assert!(segmented_strain(crate::dynamics::STRETCH_MAX) < 1.10);
    }
}
