use std::sync::Arc;

use cranpose_animation::{AnimationType, animateFloatAsState, spring};
use cranpose_core::{mutableStateOf, remember, rememberKeyed};
use cranpose_macros::composable;
use cranpose_services::{HapticFeedback, default_haptics};
use cranpose_ui::{
    Modifier, PointerEventKind, PointerInputScope, Size,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, GraphicsLayer, Point, TileMode};

use crate::{
    material::{GlassDynamics, GlassMorph, LiquidModifierExt},
    motion::{LiquidMotion, Press},
    theme::liquid_colors,
};

pub(crate) const TRACK_WIDTH: f32 = 63.0;
pub(crate) const TRACK_HEIGHT: f32 = 28.0;
const THUMB_WIDTH: f32 = 37.0;
const THUMB_HEIGHT: f32 = 24.0;
const THUMB_MARGIN: f32 = 2.0;
const LENS_WIDTH: f32 = 58.0;
const LENS_HEIGHT: f32 = 115.0 / 3.0;
const LENS_VERTICAL_OFFSET: f32 = 0.0;
const LENS_PAD: f32 = 18.0;
const TAP_SLOP: f32 = 4.0;

// The native track recolors on commit by sliding a strip ten tracks wide
// beneath it: one track of the on color, eight of a linear blend, one of the
// off color. The strip's travel is a spring fitted to the iOS 27 tap and drag
// layer traces.
const FILL_STRIP_TRACKS: f32 = 10.0;

fn toggle_fill_motion() -> AnimationType {
    spring(1.08, 108.0)
}

// The on and off track colors, shared by every frame's strip.
#[composable]
fn remember_track_colors() -> Arc<[Color]> {
    let colors = liquid_colors();
    rememberKeyed((colors.toggle_on, colors.toggle_off), |&(on, off)| {
        Arc::<[Color]>::from([on, off])
    })
}

fn track_brush(fill: f32, colors: &Arc<[Color]>) -> Brush {
    let blend = FILL_STRIP_TRACKS - 2.0;
    if fill <= 0.0 || fill >= 1.0 {
        return Brush::solid(colors[usize::from(fill <= 0.0)]);
    }
    let strip_left = -(FILL_STRIP_TRACKS - 1.0) * TRACK_WIDTH * (1.0 - fill);
    Brush::LinearGradient {
        colors: Arc::clone(colors),
        stops: None,
        start: Point::new(strip_left + TRACK_WIDTH, 0.0),
        end: Point::new(strip_left + TRACK_WIDTH * (1.0 + blend), 0.0),
        tile_mode: TileMode::Clamp,
    }
}

fn lens_translation_x(thumb_x: f32, node_width: f32) -> f32 {
    thumb_x + (THUMB_WIDTH - node_width) * 0.5
}

fn lens_ride_x(drag_progress: Option<f32>, thumb_x: f32) -> f32 {
    drag_progress.map_or(thumb_x, |progress| {
        let min_x = THUMB_MARGIN;
        let max_x = TRACK_WIDTH - THUMB_MARGIN - THUMB_WIDTH;
        min_x + (max_x - min_x) * progress.clamp(0.0, 1.0)
    })
}

/// An on/off switch. `checked` is owned by the caller; `on_change` receives
/// the requested new value. The thumb both taps and swipes.
#[composable]
pub fn LiquidToggle(modifier: Modifier, checked: bool, on_change: impl Fn(bool) + 'static) {
    let drag_progress = remember(|| mutableStateOf(Option::<f32>::None)).with(|s| *s);
    let track_colors = remember_track_colors();
    let fill = animateFloatAsState(f32::from(checked), toggle_fill_motion(), "toggle-fill");

    let min_x = THUMB_MARGIN;
    let max_x = TRACK_WIDTH - THUMB_MARGIN - THUMB_WIDTH;

    let target_x = if checked { max_x } else { min_x };
    let lens_axis = crate::motion::remember_liquid_follow_axis(target_x, spring(1.0, 300.0));
    let shape = super::lens_motion::remember_lens_shape(super::lens_motion::LensShapeKind::Thumb);
    lens_axis.settle_to(target_x, LiquidMotion::snappy());
    let contact = super::control_motion::remember_control_contact(
        super::control_motion::ControlContactKind::Thumb,
    );
    let (lens_progress, material_progress) = contact.states();

    let on_change = std::rc::Rc::new(on_change);
    let track = Modifier::empty()
        .size(Size::new(TRACK_WIDTH, TRACK_HEIGHT))
        .stable_semantics(switch_semantics(checked))
        .focusable()
        .pointer_input(checked, {
            let on_change = std::rc::Rc::clone(&on_change);
            let lens_axis = std::rc::Rc::clone(&lens_axis);
            let contact = std::rc::Rc::clone(&contact);
            move |scope: PointerInputScope| {
                let on_change = std::rc::Rc::clone(&on_change);
                let lens_axis = std::rc::Rc::clone(&lens_axis);
                let contact = std::rc::Rc::clone(&contact);
                async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            let mut down_x = 0.0f32;
                            let mut grab_offset = 0.0f32;
                            let mut dragging = false;
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                match event.kind {
                                    PointerEventKind::Down => {
                                        dragging = true;
                                        down_x = event.position.x;
                                        grab_offset = event.position.x
                                            - (lens_axis.value() + THUMB_WIDTH * 0.5);
                                        lens_axis.begin(lens_axis.value(), event.time_ms);
                                        contact.pressed(Press::Down, event.animation_time_nanos);
                                        default_haptics().perform(HapticFeedback::Selection);
                                        event.consume();
                                    }
                                    PointerEventKind::Move if dragging => {
                                        let progress = ((event.position.x
                                            - grab_offset
                                            - THUMB_MARGIN
                                            - THUMB_WIDTH * 0.5)
                                            / (TRACK_WIDTH - 2.0 * THUMB_MARGIN - THUMB_WIDTH))
                                            .clamp(0.0, 1.0);
                                        drag_progress.set(Some(progress));
                                        lens_axis.move_to(
                                            lens_ride_x(Some(progress), lens_axis.value()),
                                            event.time_ms,
                                        );
                                        event.consume();
                                    }
                                    PointerEventKind::Up if dragging => {
                                        dragging = false;
                                        contact.pressed(Press::Up, event.animation_time_nanos);
                                        let travelled =
                                            (event.position.x - down_x).abs() > TAP_SLOP;
                                        let next = if travelled {
                                            drag_progress.get().map_or(!checked, |p| p >= 0.5)
                                        } else {
                                            !checked
                                        };
                                        lens_axis.release_to(
                                            if next { max_x } else { min_x },
                                            event.time_ms,
                                            LiquidMotion::glide(),
                                        );
                                        drag_progress.set(None);
                                        if next != checked {
                                            default_haptics().perform(HapticFeedback::ImpactLight);
                                            on_change(next);
                                        }
                                        event.consume();
                                    }
                                    PointerEventKind::Cancel if dragging => {
                                        dragging = false;
                                        contact.pressed(Press::Up, event.animation_time_nanos);
                                        lens_axis.release_to(
                                            if checked { max_x } else { min_x },
                                            event.time_ms,
                                            LiquidMotion::snappy(),
                                        );
                                        drag_progress.set(None);
                                        event.consume();
                                    }
                                    _ => {}
                                }
                            }
                        })
                        .await;
                }
            }
        })
        .draw_behind(move |scope| {
            scope.draw_round_rect(
                track_brush(fill.get(), &track_colors),
                CornerRadii::uniform(TRACK_HEIGHT * 0.5),
            );
        });

    Box(modifier.then(track), BoxSpec::default(), move || {
        let Size {
            width: node_w,
            height: node_h,
        } = super::control_lens::node_size(
            Size::new(THUMB_WIDTH, THUMB_HEIGHT),
            Size::new(LENS_WIDTH, LENS_HEIGHT),
            LENS_PAD,
        );
        let shape = std::rc::Rc::clone(&shape);

        let lens_for_layer = lens_progress;
        let physics_axis = std::rc::Rc::clone(&lens_axis);
        let layer_axis = std::rc::Rc::clone(&lens_axis);
        let lens = Modifier::empty()
            .required_size(Size::new(node_w, node_h))
            .offset(0.0, (TRACK_HEIGHT - node_h) * 0.5 + LENS_VERTICAL_OFFSET)
            .graphics_layer(move || GraphicsLayer {
                translation_x: lens_translation_x(layer_axis.value(), node_w),
                ..Default::default()
            })
            .glass_effect_with(
                super::control_material::material(super::control_motion::ControlContactKind::Thumb),
                move || {
                    let grow = lens_for_layer.get().clamp(-0.1, 1.2);
                    let base_w = THUMB_WIDTH + (LENS_WIDTH - THUMB_WIDTH) * grow;
                    let base_h = THUMB_HEIGHT + (LENS_HEIGHT - THUMB_HEIGHT) * grow;
                    let projection = shape.projection(physics_axis.value());
                    let material = material_progress.get().max(0.0);
                    GlassDynamics {
                        optical_projection: Some(projection),
                        resting_tint: Some(Color::WHITE),
                        morph: Some(GlassMorph {
                            node_size: (node_w, node_h),
                            primary: (node_w * 0.5, node_h * 0.5, base_w, base_h, -1.0),
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
                            super::control_motion::ControlContactKind::Thumb,
                            grow,
                            material,
                        )
                    }
                },
            );
        Box(lens, BoxSpec::default(), || {});
    });
}

fn switch_semantics(checked: bool) -> impl Fn(&mut cranpose_ui::SemanticsConfiguration) {
    move |config| {
        config.role = Some(cranpose_ui::SemanticsWidgetRole::Switch);
        config.is_clickable = true;
        config.toggled = Some(checked.into());
    }
}

#[cfg(test)]
#[path = "tests/toggle_tests.rs"]
mod tests;
