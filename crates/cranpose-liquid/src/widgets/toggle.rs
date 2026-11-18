use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_animation::{Animatable, AnimationType, spring};
use cranpose_core::{MutableState, mutableStateOf, remember, rememberKeyed, with_current_composer};
use cranpose_macros::composable;
use cranpose_services::{HapticFeedback, default_haptics};
use cranpose_ui::{
    Modifier, PointerEvent, PointerEventKind, PointerInputScope, Size,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, GraphicsLayer, Point, TileMode};

use crate::{
    glass_morph::GlassMorph,
    material::{GlassDynamics, LiquidModifierExt},
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
const THUMB_MAX_X: f32 = TRACK_WIDTH - THUMB_MARGIN - THUMB_WIDTH;

// The native track recolors on commit by sliding a strip ten tracks wide
// beneath it: one track of the on color, eight of a linear blend, one of the
// off color. The strip's travel is a spring fitted to the iOS 27 tap and drag
// layer traces.
const FILL_STRIP_TRACKS: f32 = 10.0;

fn toggle_fill_motion() -> AnimationType {
    spring(1.08, 108.0)
}

// The track fill. A tap starts it at the release's time, as iOS does; a
// `checked` changed by the caller starts it from the next frame.
#[composable]
fn remember_track_fill(checked: bool) -> Rc<RefCell<Animatable<f32>>> {
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember(move || Rc::new(RefCell::new(Animatable::new(f32::from(checked), runtime))))
            .with(Rc::clone)
    })
}

// The on and off track colors, shared by every frame's strip.
#[composable]
fn remember_track_colors() -> Arc<[Color]> {
    let colors = liquid_colors();
    // Colors on eight bits pass the renderer's snap without a copy.
    rememberKeyed((colors.toggle_on, colors.toggle_off), |&(on, off)| {
        Arc::<[Color]>::from([on.srgb_8bit(), off.srgb_8bit()])
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

// What a toggle's touches drive.
struct ToggleGesture {
    on_change: Rc<dyn Fn(bool)>,
    lens_axis: Rc<crate::motion::LiquidDragAxis>,
    contact: Rc<super::control_motion::ControlContactMotion>,
    fill: Rc<RefCell<Animatable<f32>>>,
    drag_progress: MutableState<Option<f32>>,
    checked: bool,
}

// A press on the thumb: where it went down, and how far from the thumb's
// center.
struct ThumbPress {
    down_x: f32,
    grab_offset: f32,
}

fn thumb_x(checked: bool) -> f32 {
    if checked { THUMB_MAX_X } else { THUMB_MARGIN }
}

impl ToggleGesture {
    fn modifier(self) -> Modifier {
        let gesture = Rc::new(self);
        Modifier::empty().pointer_input(gesture.checked, move |scope: PointerInputScope| {
            let gesture = Rc::clone(&gesture);
            async move {
                scope
                    .await_pointer_event_scope(|await_scope| async move {
                        let mut press = None;
                        loop {
                            let event = await_scope.await_pointer_event().await;
                            gesture.on_event(&mut press, &event);
                        }
                    })
                    .await;
            }
        })
    }

    fn on_event(&self, press: &mut Option<ThumbPress>, event: &PointerEvent) {
        match (event.kind, press.take()) {
            (PointerEventKind::Down, _) => *press = Some(self.on_down(event)),
            (PointerEventKind::Move, Some(thumb)) => {
                self.on_move(&thumb, event);
                *press = Some(thumb);
            }
            (PointerEventKind::Up, Some(thumb)) => self.on_up(&thumb, event),
            (PointerEventKind::Cancel, Some(_)) => self.on_cancel(event),
            (_, thumb) => {
                *press = thumb;
                return;
            }
        }
        event.consume();
    }

    fn on_down(&self, event: &PointerEvent) -> ThumbPress {
        let value = self.lens_axis.value();
        self.lens_axis.begin(value, event.time_ms);
        self.contact
            .pressed(Press::Down, event.animation_time_nanos);
        default_haptics().perform(HapticFeedback::Selection);
        ThumbPress {
            down_x: event.position.x,
            grab_offset: event.position.x - (value + THUMB_WIDTH * 0.5),
        }
    }

    fn on_move(&self, thumb: &ThumbPress, event: &PointerEvent) {
        let progress = ((event.position.x - thumb.grab_offset - THUMB_MARGIN - THUMB_WIDTH * 0.5)
            / (TRACK_WIDTH - 2.0 * THUMB_MARGIN - THUMB_WIDTH))
            .clamp(0.0, 1.0);
        self.drag_progress.set(Some(progress));
        self.lens_axis.move_to(
            lens_ride_x(Some(progress), self.lens_axis.value()),
            event.time_ms,
        );
    }

    fn on_up(&self, thumb: &ThumbPress, event: &PointerEvent) {
        self.contact.pressed(Press::Up, event.animation_time_nanos);
        let travelled = (event.position.x - thumb.down_x).abs() > TAP_SLOP;
        let next = if travelled {
            self.drag_progress
                .get()
                .map_or(!self.checked, |progress| progress >= 0.5)
        } else {
            !self.checked
        };
        self.lens_axis
            .release_to(thumb_x(next), event.time_ms, LiquidMotion::glide());
        self.drag_progress.set(None);
        if next != self.checked {
            crate::motion::retarget(
                &mut self.fill.borrow_mut(),
                f32::from(next),
                toggle_fill_motion(),
                event.animation_time_nanos,
            );
            default_haptics().perform(HapticFeedback::ImpactLight);
            (self.on_change)(next);
        }
    }

    fn on_cancel(&self, event: &PointerEvent) {
        self.contact.pressed(Press::Up, event.animation_time_nanos);
        self.lens_axis
            .release_to(thumb_x(self.checked), event.time_ms, LiquidMotion::snappy());
        self.drag_progress.set(None);
    }
}

/// An on/off switch. `checked` is owned by the caller; `on_change` receives
/// the requested new value. The thumb both taps and swipes.
#[composable]
pub fn LiquidToggle(modifier: Modifier, checked: bool, on_change: impl Fn(bool) + 'static) {
    let drag_progress = remember(|| mutableStateOf(Option::<f32>::None)).with(|s| *s);
    let track_colors = remember_track_colors();
    let fill = remember_track_fill(checked);
    crate::motion::retarget(
        &mut fill.borrow_mut(),
        f32::from(checked),
        toggle_fill_motion(),
        None,
    );
    let fill_state = fill.borrow().state();

    let target_x = thumb_x(checked);
    let lens_axis = crate::motion::remember_liquid_follow_axis(target_x, spring(1.0, 300.0));
    let shape = super::lens_motion::remember_lens_shape(super::lens_motion::LensShapeKind::Thumb);
    lens_axis.settle_to(target_x, LiquidMotion::snappy());
    let contact = super::control_motion::remember_control_contact(
        super::control_motion::ControlContactKind::Thumb,
    );
    let (lens_progress, material_progress) = contact.states();

    let track = Modifier::empty()
        .size(Size::new(TRACK_WIDTH, TRACK_HEIGHT))
        .stable_semantics(switch_semantics(checked))
        .focusable()
        .then(
            ToggleGesture {
                on_change: Rc::new(on_change),
                lens_axis: Rc::clone(&lens_axis),
                contact: Rc::clone(&contact),
                fill,
                drag_progress,
                checked,
            }
            .modifier(),
        )
        .draw_behind(move |scope| {
            scope.draw_round_rect(
                track_brush(fill_state.get(), &track_colors),
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
        let shape = Rc::clone(&shape);

        let lens_for_layer = lens_progress;
        let physics_axis = Rc::clone(&lens_axis);
        let layer_axis = Rc::clone(&lens_axis);
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
