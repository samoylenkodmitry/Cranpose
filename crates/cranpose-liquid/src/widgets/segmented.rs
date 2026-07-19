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

/// Reference control height: the marker capsule crop is 130px at the
/// recording's 2.89 px/dp = 45dp total, 41dp inside the track padding.
const SEGMENT_HEIGHT: f32 = 41.0;
const TRACK_PADDING: f32 = 2.0;
/// The light-scheme selection marker is a RECESSED well pressed into the
/// white body: interior 244 on the 254 body (tap-flight f_045 mean
/// 242.7), its depth told by an inner shadow arc hugging the top rim —
/// never by darkening the fill. A filled gray track does not exist in
/// the reference.
const MARKER_INNER_SHADOW_ALPHA: f32 = 0.09;
const MARKER_INNER_SHADOW_RADIUS: f32 = 1.0;
/// Small top bias; the shade hugs the WHOLE perimeter softly (the
/// reference rim is one narrow even gradient, slightly stronger on the
/// upper arc — a bright inner lip ring is an artifact it never shows).
const MARKER_INNER_SHADOW_DROP: f32 = 0.4;
/// The resting marker is a big TRUE ELLIPSE, not a capsule: 120dp over
/// the 103dp cell (tap-flight f_045, marker run 347px against the 299px
/// cell at 2.89 px/dp), centered on its cell and overhanging both
/// neighbors. A stadium's long flat top/bottom reads as a rounded
/// rectangle — the reference silhouette crests continuously.
const MARKER_WIDTH_FACTOR: f32 = 1.16;
/// The marker crests past the body top (f_045 column: the shade rides
/// over the body edge) and only whispers past the bottom — the poke is
/// asymmetric, biased up, and subtle.
const MARKER_POKE_TOP: f32 = 1.5;
const MARKER_POKE_BOTTOM: f32 = 0.5;
/// How far the interaction lens pokes past the track vertically. With the
/// lift scale this lands the raised body at ~1.4x the track height — the
/// reference finger oval (segmented-drag f_025). The oval is ONE
/// continuous silhouette; a separate glued halo circle read as two-lobed
/// "cheeks" bulging over the track.
const LENS_OVERFLOW: f32 = 8.0;
/// How far the raised lens SDF blends from the capsule toward a true
/// ellipse (activity-scaled at the material layer; the resting lens is
/// untouched). The reference riding body is a continuously curved oval,
/// never a flat-topped capsule (segmented-drag f_025..f_055).
const LENS_ELLIPSE_BLEND: f32 = 0.55;
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

    // The light-scheme control body is a WHITE capsule floating on the
    // page with a soft, below-biased drop shadow (drag f_130 corners:
    // page ~235-240 falling to ~219-225 under the caps, body 254). The
    // recessed marker and the raised lens live INSIDE this white body.
    // Dark keeps a filled track for contrast.
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
                scope.radius = 7.0;
                scope.offset.y = 1.5;
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
            let indicator_brush = if colors.is_dark {
                Brush::solid(Color::from_rgba_u8(90, 90, 96, 240))
            } else {
                // The marker is a LIGHT SOLID well, not a translucent dark
                // wash: a wash over the page turned the above-body crest
                // into a harsh dark eyebrow (219 on the 242 card), while
                // the reference crest is nearly page-light (232 on 237).
                // The recessed depth reads from the rim-following inner
                // shadow, not from the fill.
                Brush::solid(Color::from_rgb_u8(244, 244, 246))
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
            // The ellipse is a gradient CIRCLE stretched horizontally by
            // the layer transform — the only path to a true oval with a
            // gradient fill (vector paths drop gradients, round-rects cap
            // at stadium).
            let marker_h =
                SEGMENT_HEIGHT + TRACK_PADDING * 2.0 + MARKER_POKE_TOP + MARKER_POKE_BOTTOM;
            let marker_aspect = segment_width * MARKER_WIDTH_FACTOR / marker_h;
            let indicator = Modifier::empty()
                .size(Size::new(marker_h, marker_h))
                // Center the circle node on cell 0, biased up so the
                // stretched ellipse crests past the body top.
                .offset(
                    (segment_width - marker_h) * 0.5,
                    -(TRACK_PADDING + MARKER_POKE_TOP),
                )
                .graphics_layer(move || {
                    let lead = leading.get();
                    let trail = trailing.get().max(lead + 1.0);
                    let center_pivot = cranpose_ui_graphics::TransformOrigin {
                        pivot_fraction_x: 0.5,
                        pivot_fraction_y: 0.5,
                    };
                    if lens_engaged {
                        let travel =
                            (indicator_axis.value() - visual_cell_x) / segment_width.max(1.0);
                        GraphicsLayer {
                            translation_x: visual_cell_x,
                            scale_x: marker_aspect,
                            alpha: plain_indicator_alpha(travel),
                            transform_origin: center_pivot,
                            ..Default::default()
                        }
                    } else {
                        // Blob morph: the ellipse centers on the moving
                        // cell span and stretches with it.
                        GraphicsLayer {
                            translation_x: (lead + trail - segment_width) * 0.5,
                            scale_x: marker_aspect
                                * ((trail - lead) / segment_width.max(1.0)).max(0.01),
                            alpha: 1.0,
                            transform_origin: center_pivot,
                            ..Default::default()
                        }
                    }
                })
                .draw_behind(move |scope| {
                    scope.draw_round_rect(
                        indicator_brush.clone(),
                        CornerRadii::uniform(marker_h * 0.5),
                    );
                })
                // The recess depth: a dark arc hugging the top rim (drawn
                // on the circle, stretched with it into the ellipse).
                .inner_shadow(
                    cranpose_ui_graphics::LayerShape::Rounded(
                        cranpose_ui_graphics::RoundedCornerShape::uniform(marker_h * 0.5),
                    ),
                    |scope| {
                        scope.radius = MARKER_INNER_SHADOW_RADIUS;
                        scope.offset.y = MARKER_INNER_SHADOW_DROP;
                        scope.color = Color::BLACK.with_alpha(MARKER_INNER_SHADOW_ALPHA);
                    },
                );
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
                            // Reference "Sending" spans 63dp of the 103dp
                            // cell; the subheadline's 15dp rendered 55dp.
                            font_size: cranpose_ui::text::TextUnit::Sp(17.0),
                            // The reference weight step is a whisper
                            // (regular -> medium); semibold-vs-medium read
                            // as a black blob against the airy target.
                            font_weight: Some(if is_selected {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
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
                    // Hard-gate the tail of the decay: a few percent of
                    // residual lens leaves chromatic dust on the settled
                    // marker rim, where the reference is perfectly clean.
                    alpha: {
                        let lens = lens_for_layer.get();
                        if lens < 0.04 {
                            0.0
                        } else {
                            (lens * 2.5).clamp(0.0, 1.0)
                        }
                    },
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
                        // The raised oval's interior reads a few percent
                        // darker than the page (segmented-drag f_025..f_055
                        // interiors 236..244 on 254) and it casts a soft
                        // drop shadow while lifted.
                        .tint(Color::rgba(0.0, 0.0, 0.0, 0.03))
                        .shadow(true)
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
