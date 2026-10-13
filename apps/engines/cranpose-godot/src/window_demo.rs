use cranpose::prelude::ExternalTexture;
use cranpose_gpu_interop::godot::{Alpha, GodotTexture};
use demo_ui::{EngineInfo, WindowBridge};
use godot::{
    classes::{
        CanvasLayer, Engine, INode, InputEvent, Node, RenderingServer, SubViewport,
        sub_viewport::UpdateMode,
    },
    prelude::*,
};

use crate::{surface::Surface, world::DemoWorld};

const GODOT: EngineInfo = EngineInfo {
    name: "Godot",
    description: "Godot 4.7 renders this scene with its own Metal renderer. Cranpose renders on the same Metal device, shows the frame as an image, clips it and draws glass over it. Frames cross between the two queues by GPU copies; no pixels pass through the CPU.",
    features: "Procedural sky, shadows, SSAO, fog, glow",
};

struct Imported {
    rid: Rid,
    godot: GodotTexture,
    receiver: wgpu::Texture,
    texture: ExternalTexture,
    since: u64,
}

#[derive(GodotClass)]
#[class(base=Node, init)]
pub struct CranposeWindowDemo {
    base: Base<Node>,
    world: Option<Gd<DemoWorld>>,
    viewport: Option<Gd<SubViewport>>,
    surface: Option<Surface>,
    bridge: WindowBridge,
    shown: Option<Imported>,
    candidate: Option<Imported>,
    frame: u64,
    since_report: f64,
}

#[godot_api]
impl INode for CranposeWindowDemo {
    fn ready(&mut self) {
        let mut viewport = SubViewport::new_alloc();
        viewport.set_use_own_world_3d(true);
        viewport.set_update_mode(UpdateMode::ALWAYS);
        viewport.set_size(Vector2i::new(64, 64));
        let world = DemoWorld::new_alloc();
        viewport.add_child(&world);
        self.base_mut().add_child(&viewport);
        self.world = Some(world);
        self.viewport = Some(viewport);
        let mut layer = CanvasLayer::new_alloc();
        self.base_mut().add_child(&layer);
        let bridge = self.bridge.clone();
        match Surface::new(&mut layer, false, move || {
            demo_ui::HostedEngineWindow(GODOT, bridge.clone())
        }) {
            Ok(surface) => self.surface = Some(surface),
            Err(error) => {
                godot_error!("Cranpose window: {error}");
            }
        }
    }

    fn process(&mut self, delta: f64) {
        let size = self
            .base()
            .get_viewport()
            .map(|viewport| viewport.get_visible_rect().size);
        if let Some(states) = self.bridge.states.get().copied() {
            if let Some(world) = self.world.as_mut() {
                world.bind_mut().set_controls(states.controls.value());
            }
            self.since_report += delta;
            if self.since_report >= 0.5 {
                states
                    .fps
                    .set(Engine::singleton().get_frames_per_second().round() as u32);
                self.since_report = 0.0;
            }
            if let Some(texture) = self.engine_frame() {
                self.frame += 1;
                states.frame.set(Some(texture.frame(self.frame)));
            }
        }
        if let (Some(surface), Some(size)) = (self.surface.as_mut(), size)
            && let Err(error) = surface.draw(Vector2i::new(size.x as i32, size.y as i32))
        {
            godot_error!("Cranpose window draw: {error}");
        }
    }

    fn input(&mut self, event: Gd<InputEvent>) {
        if let Some(surface) = self.surface.as_mut() {
            surface.input(&event);
        }
        if let Some(mut viewport) = self.base().get_viewport() {
            viewport.set_input_as_handled();
        }
    }
}

impl CranposeWindowDemo {
    fn engine_frame(&mut self) -> Option<ExternalTexture> {
        let (width, height) = self.bridge.size.0.get();
        if width == 0 || height == 0 {
            return None;
        }
        let now = Engine::singleton().get_process_frames();
        let rendered = self.viewport.as_ref().and_then(viewport_texture);
        self.copy_rendered(rendered, now);
        let viewport = self.viewport.as_mut()?;
        let wanted = Vector2i::new(width as i32, height as i32);
        if viewport.get_size() != wanted {
            viewport.set_size(wanted);
        }
        let next = viewport_texture(viewport)?;
        let known = |imported: &Option<Imported>| {
            imported
                .as_ref()
                .is_some_and(|imported| imported.rid == next)
        };
        if !known(&self.shown) && !known(&self.candidate) {
            let surface = self.surface.as_mut()?;
            match import(surface, next, now) {
                Ok(imported) => self.candidate = Some(imported),
                Err(error) => {
                    godot_error!("Godot viewport texture: {error}");
                }
            }
        }
        self.shown.as_ref().map(|shown| shown.texture.clone())
    }

    fn copy_rendered(&mut self, current: Option<Rid>, now: u64) {
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let rendered = self
            .candidate
            .as_ref()
            .is_some_and(|candidate| candidate.since < now);
        if rendered {
            self.shown = self.candidate.take();
        } else if self
            .shown
            .as_ref()
            .is_none_or(|shown| Some(shown.rid) != current)
        {
            return;
        }
        if let Some(shown) = self.shown.as_ref() {
            surface
                .gpu()
                .receive(&shown.godot, &shown.receiver, Alpha::Opaque);
        }
    }
}

fn viewport_texture(viewport: &Gd<SubViewport>) -> Option<Rid> {
    let rid =
        RenderingServer::singleton().texture_get_rd_texture(viewport.get_texture()?.get_rid());
    rid.is_valid().then_some(rid)
}

fn import(surface: &mut Surface, rid: Rid, since: u64) -> Result<Imported, String> {
    let godot = surface.godot_texture(rid)?;
    let receiver = surface
        .gpu()
        .receiver(&godot)
        .map_err(|error| error.to_string())?;
    let view = receiver.create_view(&wgpu::TextureViewDescriptor::default());
    let texture = ExternalTexture::new(receiver.width(), receiver.height(), true, view)
        .map_err(|error| error.to_string())?;
    Ok(Imported {
        rid,
        godot,
        receiver,
        texture,
        since,
    })
}
