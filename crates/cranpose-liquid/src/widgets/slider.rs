use std::{cell::Cell, rc::Rc};

use cranpose_core::remember;
use cranpose_foundation::{PointerEventKind, PointerId};
use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, Size,
    widgets::{Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, GraphicsLayer};

use crate::{
    material::{Glass, GlassDynamics, GlassMorph, LiquidModifierExt, LiquidShape},
    theme::liquid_colors,
};

const TRACK_HEIGHT: f32 = 6.0;
const THUMB_WIDTH: f32 = 37.0;
const THUMB_HEIGHT: f32 = 24.0;
const SLIDER_HEIGHT: f32 = 32.0;
const LENS_WIDTH: f32 = THUMB_WIDTH * 1.55;
const LENS_HEIGHT: f32 = THUMB_HEIGHT * 1.55;
const LENS_PAD: f32 = 10.0;

/// A 0..=1 slider. The caller owns `value`; `on_change` streams new values
/// while dragging or on tap.
#[composable]
pub fn LiquidSlider(modifier: Modifier, value: f32, on_change: impl Fn(f32) + 'static) {
    let colors = liquid_colors();
    let value = value.clamp(0.0, 1.0);
    let on_change: Rc<dyn Fn(f32)> = Rc::new(on_change);
    let active_pointer = remember(|| Rc::new(Cell::new(Option::<PointerId>::None))).with(Rc::clone);

    let contact = super::control_motion::remember_control_contact(
        super::control_motion::ControlContactKind::Thumb,
    );
    let (lens_progress, material_progress) = contact.states();

    Box(
        modifier
            .height(SLIDER_HEIGHT)
            .stable_semantics(slider_semantics(value, Rc::clone(&on_change))),
        BoxSpec::default(),
        move || {
            let on_change = Rc::clone(&on_change);
            let active_pointer = Rc::clone(&active_pointer);
            let contact = Rc::clone(&contact);
            BoxWithConstraints(Modifier::empty(), move |scope| {
                let width = scope.constraints().max_width.max(THUMB_WIDTH);
                let usable = (width - THUMB_WIDTH).max(1.0);
                let controlled_x = usable * value;
                let lens_axis = crate::motion::remember_liquid_drag_axis(controlled_x);
                let shape = super::lens_motion::remember_lens_shape(
                    super::lens_motion::LensShapeKind::Thumb,
                );
                lens_axis.settle_to(controlled_x, crate::motion::LiquidMotion::snappy());
                let thumb_x = lens_axis.value();

                let on_drag = Rc::clone(&on_change);
                let active_pointer = Rc::clone(&active_pointer);
                let gesture_axis = Rc::clone(&lens_axis);
                let contact = Rc::clone(&contact);
                let surface = Modifier::empty()
                    .size(Size::new(width, SLIDER_HEIGHT))
                    .pointer_input((width.to_bits(), value.to_bits()), {
                        move |pointer_scope| {
                            let on_drag = Rc::clone(&on_drag);
                            let active_pointer = Rc::clone(&active_pointer);
                            let lens_axis = Rc::clone(&gesture_axis);
                            let contact = Rc::clone(&contact);
                            async move {
                                pointer_scope
                                    .await_pointer_event_scope(|await_scope| async move {
                                        loop {
                                            let event = await_scope.await_pointer_event().await;
                                            let fraction = ((event.position.x - THUMB_WIDTH * 0.5)
                                                / usable)
                                                .clamp(0.0, 1.0);
                                            match event.kind {
                                                PointerEventKind::Down
                                                    if active_pointer.get().is_none() =>
                                                {
                                                    active_pointer.set(Some(event.id));
                                                    lens_axis
                                                        .begin(usable * fraction, event.time_ms);
                                                    contact
                                                        .pressed(true, event.animation_time_nanos);
                                                    event.consume();
                                                    on_drag(fraction);
                                                }
                                                PointerEventKind::Move
                                                    if active_pointer.get() == Some(event.id) =>
                                                {
                                                    lens_axis
                                                        .move_to(usable * fraction, event.time_ms);
                                                    event.consume();
                                                    on_drag(fraction);
                                                }
                                                PointerEventKind::Up
                                                    if active_pointer.get() == Some(event.id) =>
                                                {
                                                    lens_axis.finish_at(
                                                        usable * fraction,
                                                        event.time_ms,
                                                    );
                                                    on_drag(fraction);
                                                    event.consume();
                                                    active_pointer.set(None);
                                                    contact
                                                        .pressed(false, event.animation_time_nanos);
                                                }
                                                PointerEventKind::Cancel
                                                    if active_pointer.get() == Some(event.id) =>
                                                {
                                                    lens_axis.release_to(
                                                        controlled_x,
                                                        event.time_ms,
                                                        crate::motion::LiquidMotion::snappy(),
                                                    );
                                                    event.consume();
                                                    active_pointer.set(None);
                                                    contact
                                                        .pressed(false, event.animation_time_nanos);
                                                }
                                                _ => {}
                                            }
                                        }
                                    })
                                    .await;
                            }
                        }
                    });

                Box(surface, BoxSpec::default(), move || {
                    let Size {
                        width: node_w,
                        height: node_h,
                    } = super::control_lens::node_size(
                        Size::new(THUMB_WIDTH, THUMB_HEIGHT),
                        Size::new(LENS_WIDTH, LENS_HEIGHT),
                        LENS_PAD,
                    );
                    let shape = Rc::clone(&shape);
                    let track_fill = colors.label.with_alpha(25.0 / 255.0);
                    let track = Modifier::empty()
                        .size(Size::new(width, TRACK_HEIGHT))
                        .offset(0.0, (SLIDER_HEIGHT - TRACK_HEIGHT) * 0.5)
                        .draw_behind(move |scope| {
                            scope.draw_round_rect(
                                Brush::solid(track_fill),
                                CornerRadii::uniform(TRACK_HEIGHT * 0.5),
                            );
                        });
                    Box(track, BoxSpec::default(), || {});

                    let accent = colors.accent;
                    let filled_width = (thumb_x + THUMB_WIDTH * 0.5).max(TRACK_HEIGHT);
                    let filled = Modifier::empty()
                        .size(Size::new(filled_width, TRACK_HEIGHT))
                        .offset(0.0, (SLIDER_HEIGHT - TRACK_HEIGHT) * 0.5)
                        .draw_behind(move |scope| {
                            scope.draw_round_rect(
                                Brush::solid(accent),
                                CornerRadii::uniform(TRACK_HEIGHT * 0.5),
                            );
                        });
                    Box(filled, BoxSpec::default(), || {});

                    let lens_for_thumb = material_progress;
                    let thumb_shape = Rc::clone(&shape);
                    let thumb_axis = Rc::clone(&lens_axis);
                    let thumb = Modifier::empty()
                        .size(Size::new(THUMB_WIDTH, THUMB_HEIGHT))
                        .offset(0.0, (SLIDER_HEIGHT - THUMB_HEIGHT) * 0.5)
                        .graphics_layer(move || {
                            let lens = lens_for_thumb.get();
                            let grow = lens_progress.get().clamp(-0.1, 1.2);
                            let projection = thumb_shape.projection(thumb_axis.value());
                            GraphicsLayer {
                                translation_x: thumb_x.round(),
                                scale_x: (THUMB_WIDTH + (LENS_WIDTH - THUMB_WIDTH) * grow)
                                    / THUMB_WIDTH
                                    * projection.0,
                                scale_y: (THUMB_HEIGHT + (LENS_HEIGHT - THUMB_HEIGHT) * grow)
                                    / THUMB_HEIGHT
                                    * projection.1,
                                alpha: (1.0 - lens).clamp(0.0, 1.0),
                                ..Default::default()
                            }
                        })
                        .drop_shadow(
                            cranpose_ui_graphics::LayerShape::Rounded(
                                cranpose_ui_graphics::RoundedCornerShape::uniform(
                                    THUMB_HEIGHT * 0.5,
                                ),
                            ),
                            |scope| {
                                scope.radius = 6.0;
                                scope.offset.y = 2.0;
                                scope.color = Color::BLACK.with_alpha(0.16);
                            },
                        );
                    super::control_lens::WhiteControlThumb(thumb, THUMB_HEIGHT);

                    let lens_for_layer = lens_progress;
                    let physics_axis = Rc::clone(&lens_axis);
                    let lens = Modifier::empty()
                        .required_size(Size::new(node_w, node_h))
                        .offset(0.0, (SLIDER_HEIGHT - node_h) * 0.5)
                        .graphics_layer(move || GraphicsLayer {
                            translation_x: thumb_x + (THUMB_WIDTH - node_w) * 0.5,
                            alpha: material_progress.get().clamp(0.0, 1.0),
                            ..Default::default()
                        })
                        .glass_effect_with(
                            Glass::lens()
                                .shape(LiquidShape::Capsule)
                                .tint(Color::WHITE.with_alpha(0.07))
                                .face_lighting(false)
                                .highlight(0.15)
                                .no_clip(),
                            move || {
                                let grow = lens_for_layer.get().clamp(-0.1, 1.2);
                                let w = THUMB_WIDTH + (LENS_WIDTH - THUMB_WIDTH) * grow;
                                let h = THUMB_HEIGHT + (LENS_HEIGHT - THUMB_HEIGHT) * grow;
                                let projection = shape.projection(physics_axis.value());
                                GlassDynamics {
                                    optical_projection: Some(projection),
                                    activity: Some(material_progress.get().clamp(0.0, 1.0)),
                                    morph: Some(GlassMorph {
                                        node_size: (node_w, node_h),
                                        primary: (node_w * 0.5, node_h * 0.5, w, h, -1.0),
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
                                    ..Default::default()
                                }
                            },
                        );
                    Box(lens, BoxSpec::default(), || {});
                });
            });
        },
    );
}

fn slider_semantics(
    value: f32,
    on_change: Rc<dyn Fn(f32)>,
) -> impl Fn(&mut cranpose_ui::SemanticsConfiguration) {
    move |config| {
        config.state_description = Some(format!("{}%", (value * 100.0).round() as u32));
        config.progress = Some(cranpose_ui::ProgressBarRangeInfo::new(value, 0.0, 1.0, 0));
        let on_change = Rc::clone(&on_change);
        config.set_progress = Some(cranpose_ui::SemanticsSetProgress::new(move |next: f32| {
            on_change(next.clamp(0.0, 1.0));
            true
        }));
    }
}
#[cfg(test)]
#[path = "tests/slider_tests.rs"]
mod tests;
