use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Once, OnceLock},
};

use cranpose_animation::{Animatable, AnimationType, spring};
use cranpose_core::{RuntimeHandle, State, remember, with_current_composer};
use cranpose_foundation::{SemanticsCustomAction, SemanticsWidgetRole};
use cranpose_macros::composable;
use cranpose_services::{HapticFeedback, default_haptics};
use cranpose_ui::{Modifier, PointerEvent, PointerEventKind, PointerInputScope};
use cranpose_ui_graphics::{
    GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, RenderEffect, RuntimeShader, ShaderTarget,
    ShaderWarmUp, Size,
};

use crate::material::{Glass, GlassDynamics, LiquidModifierExt};

fn lighting_shader() -> RuntimeShader {
    static SHADER: OnceLock<RuntimeShader> = OnceLock::new();
    SHADER
        .get_or_init(|| {
            let mut shader = RuntimeShader::new(&format!(
                "{RUNTIME_SHADER_PRELUDE_WGSL}\n{}\n{}",
                cranpose_ui_graphics::LIQUID_GLASS_GEOMETRY_WGSL,
                include_str!("floating_lighting.wgsl")
            ));
            shader.set_position_independent(true);
            shader
        })
        .clone()
}

fn contact_lighting(
    size: Size,
    center: (f32, f32),
    radius: f32,
    strength: f32,
) -> Option<RenderEffect> {
    if strength <= 0.0 {
        return None;
    }
    let mut shader = lighting_shader();
    shader.set_float4(0, size.width, size.height, center.0, center.1);
    shader.set_float2(4, strength.clamp(0.0, 1.0), radius);
    Some(RenderEffect::runtime_shader(shader))
}

struct Contact {
    runtime: RuntimeHandle,
    channels: RefCell<[Animatable<f32>; 4]>,
    origin: Cell<(f32, f32)>,
}

impl Contact {
    fn new(runtime: RuntimeHandle) -> Self {
        Self {
            channels: RefCell::new(std::array::from_fn(|_| {
                Animatable::new(0.0, runtime.clone())
            })),
            runtime,
            origin: Cell::new((0.0, 0.0)),
        }
    }

    fn target(&self, channel: usize, value: f32, animation: AnimationType, time: Option<u64>) {
        let mut channels = self.channels.borrow_mut();
        let channel = &mut channels[channel];
        if channel.target() == value {
            return;
        }
        if let Some(time) = time.or_else(|| self.runtime.last_frame_time_nanos()) {
            channel.animate_to_at(value, animation, time);
        } else {
            channel.animateTo(value, animation);
        }
    }

    fn pressed(&self, down: bool, time: Option<u64>) {
        let geometry = if down {
            spring(0.65, 625.0).with_delay(28)
        } else {
            spring(0.38, 250.0)
        };
        self.target(0, f32::from(down), geometry, time);
        self.target(
            1,
            f32::from(down),
            spring(1.0, if down { 4000.0 } else { 160.0 }),
            time,
        );
        if !down {
            self.moved(0.0, 0.0, time);
        }
    }

    fn moved(&self, x: f32, y: f32, time: Option<u64>) {
        self.target(2, x, spring(1.0, 400.0), time);
        self.target(3, y, spring(1.0, 400.0), time);
    }

    fn states(&self) -> [State<f32>; 4] {
        let channels = self.channels.borrow();
        std::array::from_fn(|index| channels[index].state())
    }
}

#[composable]
pub(super) fn FloatingButtonSurface(
    material: Option<Glass>,
    native_material: bool,
    hit_inset: f32,
    feedback: HapticFeedback,
    tint_progress: Option<(State<f32>, cranpose_ui_graphics::Color)>,
    on_click: impl Fn() + 'static,
) -> (Modifier, Modifier, Modifier, Modifier, State<f32>) {
    let size = remember(|| Rc::new(Cell::new(Size::ZERO))).with(Rc::clone);
    let content_size = remember(|| Rc::new(Cell::new(Size::ZERO))).with(Rc::clone);
    let runtime = with_current_composer(|c| c.runtime_handle());
    let contact = remember(move || Rc::new(Contact::new(runtime))).with(Rc::clone);
    let [progress, light, x, y] = contact.states();
    let scale_size = Rc::clone(&size);
    let native_lighting = native_material && material.is_some();
    if native_lighting {
        static WARM_UP: Once = Once::new();
        WARM_UP.call_once(|| {
            cranpose_ui_graphics::request_shader_warm_ups([ShaderWarmUp {
                shader: lighting_shader(),
                target: ShaderTarget::Layer,
            }]);
        });
    }
    let transform = Modifier::empty().graphics_layer(move || {
        let size = scale_size.get();
        let lift = if size.width > 0.0 {
            1.0 + 16.0 * progress.get() / size.width
        } else {
            1.0
        };
        let dx = x.get();
        let dy = y.get();
        let distance = dx.hypot(dy);
        let strain = 0.025 * (distance / 60.0).min(1.0).powi(2) * progress.get();
        let direction = if distance > 0.0 {
            (dx / distance, dy / distance)
        } else {
            (0.0, 0.0)
        };
        let axial = strain * (direction.0 * direction.0 - direction.1 * direction.1);
        GraphicsLayer {
            scale_x: lift * (1.0 + axial),
            scale_y: lift * (1.0 - axial),
            translation_x: size.width * lift * strain * direction.0,
            translation_y: size.height * lift * strain * direction.1,
            ..Default::default()
        }
    });
    let content = if native_lighting {
        let source_size = Rc::clone(&content_size);
        let control_size = Rc::clone(&size);
        let origin = Rc::clone(&contact);
        Modifier::empty()
            .report_size(content_size)
            .graphics_layer(move || {
                let size = source_size.get();
                let start = origin.origin.get();
                GraphicsLayer {
                    render_effect: contact_lighting(
                        size,
                        (
                            size.width * 0.5 + start.0 + x.get(),
                            size.height * 0.5 + start.1 + y.get(),
                        ),
                        control_size.get().height * 0.75,
                        light.get(),
                    ),
                    ..Default::default()
                }
            })
    } else {
        Modifier::empty()
    };
    let mut surface = Modifier::empty();
    if let Some(glass) = material {
        let glass_size = Rc::clone(&size);
        let origin = Rc::clone(&contact);
        surface = surface.glass_effect_with(glass, move || {
            let size = glass_size.get();
            let center = (size.width * 0.5, size.height * 0.5);
            if native_material {
                let start = origin.origin.get();
                let mut dynamics = super::glass_surface::floating_dynamics(
                    size,
                    progress.get(),
                    light.get(),
                    (center.0 + start.0 + x.get(), center.1 + start.1 + y.get()),
                );
                dynamics.tint_crossfade = tint_progress.map(|(state, color)| (color, state.get()));
                dynamics
            } else {
                GlassDynamics::default().touched_up(
                    light.get(),
                    Some((center.0 + x.get(), center.1 + y.get())),
                    center,
                )
            }
        });
    }
    let gesture = floating_gesture(size, contact, hit_inset, feedback, on_click);
    (transform, surface, gesture, content, light)
}

fn floating_gesture(
    size: Rc<Cell<Size>>,
    contact: Rc<Contact>,
    hit_inset: f32,
    feedback: HapticFeedback,
    on_click: impl Fn() + 'static,
) -> Modifier {
    let on_click = Rc::new(on_click);
    let activate = {
        let on_click = Rc::clone(&on_click);
        SemanticsCustomAction::new("", move || {
            default_haptics().perform(feedback);
            on_click();
        })
    };
    Modifier::empty()
        .report_size(Rc::clone(&size))
        .stable_semantics(move |config| {
            config.role = Some(SemanticsWidgetRole::Button);
            config.is_clickable = true;
            config.merge_descendants = true;
            config.on_click = Some(activate.clone());
        })
        .focusable()
        .pointer_input(hit_inset.to_bits(), move |scope: PointerInputScope| {
            let size = Rc::clone(&size);
            let contact = Rc::clone(&contact);
            let on_click = Rc::clone(&on_click);
            async move {
                scope
                    .await_pointer_event_scope(|input| async move {
                        let mut down = None;
                        let mut previous: Option<PointerEvent> = None;
                        loop {
                            let event = input.await_pointer_event().await;
                            if down.is_some_and(|(id, _)| id != event.id) {
                                continue;
                            }
                            let claimed = previous.take().is_some_and(|event| event.is_consumed());
                            if down.is_some() && (claimed || event.is_consumed()) {
                                down = None;
                                contact.pressed(false, event.animation_time_nanos);
                                continue;
                            }
                            if advance_contact(
                                &contact,
                                size.get(),
                                &mut down,
                                &event,
                                70.0 - hit_inset,
                            ) {
                                default_haptics().perform(feedback);
                                on_click();
                            }
                            previous = Some(event);
                        }
                    })
                    .await;
            }
        })
}

fn advance_contact(
    contact: &Contact,
    size: Size,
    down: &mut Option<(u64, cranpose_ui_graphics::Point)>,
    event: &PointerEvent,
    margin: f32,
) -> bool {
    match event.kind {
        PointerEventKind::Down if down.is_none() && !event.is_consumed() => {
            *down = Some((event.id, event.global_position));
            contact.origin.set((
                event.position.x - size.width * 0.5,
                event.position.y - size.height * 0.5,
            ));
            contact.pressed(true, event.animation_time_nanos);
        }
        PointerEventKind::Move => {
            if let Some((_, origin)) = *down {
                contact.moved(
                    event.global_position.x - origin.x,
                    event.global_position.y - origin.y,
                    event.animation_time_nanos,
                );
            }
        }
        PointerEventKind::Up if down.is_some() => {
            *down = None;
            contact.pressed(false, event.animation_time_nanos);
            event.consume();
            return event.position.x > -margin
                && event.position.x < size.width + margin
                && event.position.y > -margin
                && event.position.y < size.height + margin;
        }
        PointerEventKind::Cancel if down.is_some() => {
            *down = None;
            contact.pressed(false, event.animation_time_nanos);
        }
        _ => {}
    }
    false
}
