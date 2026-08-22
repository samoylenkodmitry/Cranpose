//! Segmented control with a liquid selection blob: the glass pill behind the
//! selected segment runs its leading and trailing edges on different springs,
//! so it stretches like a droplet while traveling and settles round. Touching
//! it lifts the indicator into a magnifying glass lens that follows the finger
//! across the segments (the reference control swipes, it doesn't just tap).

use crate::material::{
    Glass, GlassDynamics, GlassMorph, GlassShadow, LiquidModifierExt, LiquidShape,
};
use crate::motion::LiquidMotion;
use crate::theme::{liquid_colors, liquid_typography};
use crate::widgets::content_scope::ScopeContent;
use cranpose_animation::{animateFloatAsState, spring};
use cranpose_core::{mutableStateOf, remember};
use cranpose_macros::composable;
use cranpose_ui::text::{FontWeight, SpanStyle, TextStyle};
use cranpose_ui::widgets::{
    Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Row, RowSpec, Text,
};
use cranpose_ui::{Modifier, PointerInputScope, SemanticsWidgetRole, Size};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, GraphicsLayer};
use cranpose_ui_layout::Alignment;
use std::rc::Rc;

/// Reference control height: the marker capsule crop is 130px at the
/// recording's 2.89 px/dp = 45dp total, 41dp inside the track padding.
const SEGMENT_HEIGHT: f32 = 41.0;
const TRACK_PADDING: f32 = 2.0;
/// The resting marker IS the glass lens at a shallow rest state — a
/// clear puck sitting on the white body with its own bevel, drop shadow
/// and rim iridescence from the one glass light path (user-arbitrated:
/// no flat draw reproduces those). It spans 120dp over the 103dp cell
/// (tap-flight f_045: 347px against the 299px cell at 2.89 px/dp).
const MARKER_WIDTH_FACTOR: f32 = 1.16;
/// How much of the glass activity survives at rest: enough for the
/// shallow bevel, shadow and the iridescent whisper on the rim; the
/// touch raise runs it to 1.
const MARKER_REST_ACTIVITY: f32 = 0.55;
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
/// Reference mid-drag oval elongates to ~1.15x its rest width (f_025:
/// 138dp over the 120dp rest against 103dp cells).
const SEGMENTED_STRAIN_RESPONSE: f32 = 0.30;
/// Pointer travel below this is a tap, not a swipe.
const TAP_SLOP: f32 = 4.0;

fn segment_lens_left(pointer_x: f32, segment_width: f32, count: usize) -> f32 {
    (pointer_x - segment_width * 0.5).clamp(0.0, segment_width * (count.saturating_sub(1)) as f32)
}

fn segmented_lens_base_size(segment_width: f32, progress: f32) -> Size {
    let progress = progress.clamp(0.0, 1.2);
    // Rest: the shallow marker puck cresting past the body. Raised: the
    // finger oval at ~1.4x the track height (segmented-drag f_025).
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

/// One segment: what accessibility announces, and what it draws.
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

/// Runs `content` and returns the segments it declared.
fn collect_segments(content: impl FnOnce(&LiquidSegmentedControlScope)) -> Vec<LiquidSegment> {
    ScopeContent::collect(|segments| LiquidSegmentedControlScope { segments }, content)
}

/// The label a plain segment draws.
#[composable]
#[allow(non_snake_case)]
fn SegmentLabel(label: String, selected: bool) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    let style = TextStyle {
        span_style: SpanStyle {
            // Every reference label reads near-black (tap-flight/drag
            // strips) — selection is told by weight and the pill, never by
            // dimming the unselected cells.
            color: Some(colors.label),
            // Reference "Sending" spans 63dp of the 103dp cell; the
            // subheadline's 15dp rendered 55dp.
            font_size: cranpose_ui::text::TextUnit::Sp(17.0),
            // The reference weight step is a whisper (regular -> medium);
            // semibold-vs-medium read as a black blob against the airy target.
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
                // Bottom-biased: the reference page is clean above the
                // body's top edge (f_045 body-only column: 254 flat); the
                // shadow pools only under the caps (219-225 on f_130).
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

            // Raise state: up while touched, lingering decay on release
            // (the glass stays deep through the settle flight, then
            // relaxes back into the shallow resting puck).
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

            // Labels row under the glass. The cells keep button
            // semantics (robot/a11y); pointer handling lives on the swipe
            // surface below.
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
                    // The glass IS the resting marker — always present;
                    // rest vs raised is a depth change, never an alpha one.
                    alpha: 1.0,
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
                        .tint(Color::rgba(0.0, 0.0, 0.0, 0.08))
                        // The tab bubble's soft contact shadow (user: the
                        // segmented bubble lacked a shadow; make it like the
                        // bottom bar's "perfect shadow"). The old -5 spread
                        // against a 6 radius cancelled to an invisible sliver.
                        .shadow_style(GlassShadow::new(
                            Color::BLACK.with_alpha(0.14),
                            12.0,
                            4.0,
                            -2.0,
                        ))
                        .rim_reflection(0.04)
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
                            // Rest keeps a shallow floor of glass presence
                            // (bevel + shadow + rim whisper); touch raises
                            // depth to full.
                            activity: Some(
                                MARKER_REST_ACTIVITY
                                    + (1.0 - MARKER_REST_ACTIVITY) * grow.clamp(0.0, 1.0),
                            ),
                            // Depth follows MOTION, not the raise: the
                            // reference distorts the glyphs only while the
                            // marker MOVES (segmented-drag spectral fringing),
                            // and keeps the text crisp under a stationary
                            // held press (tap-flight T83..T300 "Errored" stays
                            // sharp). Keying the deep dome to grow alone ran a
                            // motionless hold at full depth and washed the
                            // label to gray.
                            press_depth: Some(
                                (0.12 + 0.30 * grow.clamp(0.0, 1.0) + 0.58 * pose.energy())
                                    .clamp(0.0, 1.0),
                            ),
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
        // Rest is the marker puck: 1.16x the cell, cresting past the body.
        assert_eq!(resting.width, 120.0 * MARKER_WIDTH_FACTOR);
        assert!(resting.height > SEGMENT_HEIGHT + TRACK_PADDING * 2.0);
        // The raise deepens, it does not widen into a worm.
        assert!(raised.width < resting.width * 1.10);
        assert!(raised.height > resting.height * 1.20);
        // Max fluid stretch elongates ~1.15x like the reference mid-drag
        // oval — never a two-cell worm.
        assert!(segmented_strain(crate::dynamics::STRETCH_MAX) < 1.20);
    }

    /// A plain segment announces its own label; a drawn one announces the
    /// description the caller supplies, because its content cannot.
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
        // The declaration stores content; it does not draw it.
        assert_eq!(drawn.get(), 0);
        (segments[1].content)(true);
        assert_eq!(drawn.get(), 1);
    }

    #[test]
    fn a_control_with_no_segments_declares_none() {
        assert!(collect_segments(|_| {}).is_empty());
    }
}
