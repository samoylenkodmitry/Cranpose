use demo_ui::{HudBridge, HudCommand, Stats};
use godot::{
    classes::{CanvasLayer, Engine, INode, InputEvent, InputEventMouseMotion, Node},
    global::MouseButtonMask,
    prelude::*,
};

use crate::{surface::Surface, world::DemoWorld};

#[derive(GodotClass)]
#[class(base=Node, init)]
pub struct CranposeHudDemo {
    base: Base<Node>,
    world: Option<Gd<DemoWorld>>,
    surface: Option<Surface>,
    bridge: Option<HudBridge>,
    since_report: f64,
    frames: u32,
}

#[godot_api]
impl INode for CranposeHudDemo {
    fn ready(&mut self) {
        let world = DemoWorld::new_alloc();
        self.base_mut().add_child(&world);
        self.world = Some(world);
        let mut layer = CanvasLayer::new_alloc();
        self.base_mut().add_child(&layer);
        let bridge = HudBridge::new("Godot");
        let content = bridge.clone();
        match Surface::new(&mut layer, true, move || demo_ui::Hud(content.clone())) {
            Ok(surface) => self.surface = Some(surface),
            Err(error) => {
                godot_error!("Cranpose HUD: {error}");
            }
        }
        self.bridge = Some(bridge);
    }

    fn process(&mut self, delta: f64) {
        let size = self
            .base()
            .get_viewport()
            .map(|viewport| viewport.get_visible_rect().size);
        let (Some(bridge), Some(world)) = (self.bridge.clone(), self.world.as_mut()) else {
            return;
        };
        if let Some(states) = bridge.states.get().copied() {
            world.bind_mut().set_controls(states.controls.value());
            for command in bridge.commands.borrow_mut().drain(..) {
                match command {
                    HudCommand::DropShape => world.bind_mut().drop_shape(),
                }
            }
            self.since_report += delta;
            self.frames += 1;
            if self.since_report >= 0.5 {
                states.stats.set(Stats {
                    fps: Engine::singleton().get_frames_per_second().round() as u32,
                    shapes: world.bind().shape_count(),
                    energy: 0.5
                        + 0.45 * (Engine::singleton().get_process_frames() as f32 / 90.0).sin(),
                    size: size.map_or((0, 0), |size| (size.x as u32, size.y as u32)),
                });
                self.since_report = 0.0;
                self.frames = 0;
            }
        }
        if let (Some(surface), Some(size)) = (self.surface.as_mut(), size)
            && let Err(error) = surface.draw(Vector2i::new(size.x as i32, size.y as i32))
        {
            godot_error!("Cranpose HUD draw: {error}");
        }
    }

    fn input(&mut self, event: Gd<InputEvent>) {
        let took = self
            .surface
            .as_mut()
            .is_some_and(|surface| surface.input(&event));
        if took {
            if let Some(mut viewport) = self.base().get_viewport() {
                viewport.set_input_as_handled();
            }
            return;
        }
        let Ok(motion) = event.try_cast::<InputEventMouseMotion>() else {
            return;
        };
        if !motion.get_button_mask().is_set(MouseButtonMask::LEFT) {
            return;
        }
        let Some(states) = self
            .bridge
            .as_ref()
            .and_then(|bridge| bridge.states.get().copied())
        else {
            return;
        };
        let delta = motion.get_relative();
        states.controls.update(|controls| {
            controls.yaw += delta.x * 0.004;
            controls.pitch = (controls.pitch + delta.y * 0.003).clamp(0.05, 1.5);
        });
    }
}
