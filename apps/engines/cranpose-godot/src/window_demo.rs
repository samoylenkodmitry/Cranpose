use cranpose::prelude::ExternalTexture;
use cranpose_gpu_interop::godot::GodotTexture;
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
    features: "Forward+, shadows, omni lights, glow",
};

struct Imported {
    rid: Rid,
    godot: GodotTexture,
    twin: wgpu::Texture,
    texture: ExternalTexture,
}

#[derive(GodotClass)]
#[class(base=Node, init)]
pub struct CranposeWindowDemo {
    base: Base<Node>,
    world: Option<Gd<DemoWorld>>,
    viewport: Option<Gd<SubViewport>>,
    surface: Option<Surface>,
    bridge: WindowBridge,
    imported: Option<Imported>,
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
        let viewport = self.viewport.as_mut()?;
        if width == 0 || height == 0 {
            return None;
        }
        let wanted = Vector2i::new(width as i32, height as i32);
        if viewport.get_size() != wanted {
            viewport.set_size(wanted);
        }
        let texture_rid = viewport.get_texture()?.get_rid();
        let rid = RenderingServer::singleton().texture_get_rd_texture(texture_rid);
        if !rid.is_valid() {
            return None;
        }
        let surface = self.surface.as_mut()?;
        if self
            .imported
            .as_ref()
            .is_none_or(|imported| imported.rid != rid)
        {
            let godot = match surface.godot_texture(rid) {
                Ok(texture) => texture,
                Err(error) => {
                    godot_error!("Godot viewport texture: {error}");
                    return None;
                }
            };
            let twin = surface.gpu().twin(&godot);
            let view = twin.create_view(&wgpu::TextureViewDescriptor::default());
            let texture = ExternalTexture::new(twin.width(), twin.height(), true, view).ok()?;
            self.imported = Some(Imported {
                rid,
                godot,
                twin,
                texture,
            });
        }
        let imported = self.imported.as_ref()?;
        surface.gpu().receive(&imported.godot, &imported.twin);
        Some(imported.texture.clone())
    }
}
