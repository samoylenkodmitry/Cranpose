use std::sync::{Arc, Mutex};

use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    input::{
        ButtonState,
        mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel},
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_asset::RenderAssets,
        render_resource::{AsBindGroup, BlendState, RenderPipelineDescriptor},
        renderer::{RenderAdapter, RenderDevice, RenderInstance, RenderQueue},
        texture::GpuImage,
    },
    shader::ShaderRef,
    ui_render::{
        UiMaterialPlugin,
        ui_material::{UiMaterial, UiMaterialKey},
    },
    window::{CursorMoved, PrimaryWindow},
};
use cranpose::{AppLauncher, GpuContext, GuestView};

const HUD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const CRANPOSE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const WHEEL_LINE: f32 = 40.0;

type ContentFactory = dyn Fn(&mut World) -> Box<dyn FnMut()> + Send + Sync;

pub struct CranposeHudPlugin {
    content: Arc<ContentFactory>,
}

impl CranposeHudPlugin {
    pub fn new(content: impl Fn(&mut World) -> Box<dyn FnMut()> + Send + Sync + 'static) -> Self {
        Self {
            content: Arc::new(content),
        }
    }
}

#[derive(Resource, Default, Clone, Copy)]
pub struct HudPointer {
    pub captured: bool,
}

#[derive(Resource)]
struct HudContent(Arc<ContentFactory>);

#[derive(Resource)]
struct HudGpu(GpuContext);

#[derive(Resource, Clone, Default)]
struct HudShared(Arc<Mutex<SharedTarget>>);

#[derive(Default)]
struct SharedTarget {
    image: Option<AssetId<Image>>,
    texture: Option<HudTexture>,
}

#[derive(Clone)]
struct HudTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

#[derive(Resource)]
struct HudImage(Handle<Image>);

struct HudView {
    view: GuestView,
    drawn_into: Option<wgpu::Texture>,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct HudMaterial {
    #[texture(0)]
    #[sampler(1)]
    texture: Handle<Image>,
}

impl UiMaterial for HudMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://cranpose_bevy/hud.wgsl".into()
    }

    fn specialize(descriptor: &mut RenderPipelineDescriptor, _key: UiMaterialKey<Self>) {
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING);
            }
        }
    }
}

impl Plugin for CranposeHudPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "hud.wgsl");
        app.add_plugins(UiMaterialPlugin::<HudMaterial>::default())
            .insert_resource(HudContent(Arc::clone(&self.content)))
            .init_resource::<HudPointer>()
            .init_resource::<HudShared>()
            .add_systems(Startup, spawn_hud)
            .add_systems(PreUpdate, route_input)
            .add_systems(Last, draw_hud);
    }

    fn finish(&self, app: &mut App) {
        let Some(instance) = app
            .get_sub_app(RenderApp)
            .and_then(|render| render.world().get_resource::<RenderInstance>())
            .map(|instance| (**instance).clone())
        else {
            return;
        };
        let world = app.world();
        let (Some(device), Some(queue), Some(adapter)) = (
            world.get_resource::<RenderDevice>(),
            world.get_resource::<RenderQueue>(),
            world.get_resource::<RenderAdapter>(),
        ) else {
            return;
        };
        let gpu = GpuContext {
            instance,
            adapter: (**adapter).clone(),
            device: Arc::new(device.wgpu_device().clone()),
            queue: Arc::new((**queue).clone()),
        };
        app.insert_resource(HudGpu(gpu));
        let shared = app.world().resource::<HudShared>().clone();
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .insert_resource(shared)
                .add_systems(Render, share_target.in_set(RenderSystems::PrepareResources));
        }
    }
}

fn spawn_hud(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<HudMaterial>>,
    shared: Res<HudShared>,
) {
    let mut image = Image::new_target_texture(1, 1, CRANPOSE_FORMAT, Some(HUD_FORMAT));
    image.data = None;
    image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    image.copy_on_resize = false;
    let image = images.add(image);
    if let Ok(mut shared) = shared.0.lock() {
        shared.image = Some(image.id());
    }
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        MaterialNode(materials.add(HudMaterial {
            texture: image.clone(),
        })),
        GlobalZIndex(i32::MAX),
    ));
    commands.insert_resource(HudImage(image));
}

fn share_target(images: Res<RenderAssets<GpuImage>>, shared: Res<HudShared>) {
    let Ok(mut shared) = shared.0.lock() else {
        return;
    };
    let Some(gpu_image) = shared.image.and_then(|id| images.get(id)) else {
        return;
    };
    let texture: &wgpu::Texture = &gpu_image.texture;
    if shared
        .texture
        .as_ref()
        .is_some_and(|current| current.texture == *texture)
    {
        return;
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        format: Some(CRANPOSE_FORMAT),
        ..default()
    });
    shared.texture = Some(HudTexture {
        texture: texture.clone(),
        view,
    });
}

fn route_input(
    view: Option<NonSendMut<HudView>>,
    mut cursor: MessageReader<CursorMoved>,
    mut buttons: MessageReader<MouseButtonInput>,
    mut wheel: MessageReader<MouseWheel>,
    mut pointer: ResMut<HudPointer>,
) {
    let Some(mut view) = view else {
        return;
    };
    let shell = view.view.shell();
    for moved in cursor.read() {
        shell.set_cursor(moved.position.x, moved.position.y);
    }
    for input in buttons.read() {
        if input.button != MouseButton::Left {
            continue;
        }
        match input.state {
            ButtonState::Pressed => pointer.captured = shell.pointer_pressed(),
            ButtonState::Released => {
                shell.pointer_released();
                pointer.captured = false;
            }
        }
    }
    for scrolled in wheel.read() {
        let scale = match scrolled.unit {
            MouseScrollUnit::Line => WHEEL_LINE,
            MouseScrollUnit::Pixel => 1.0,
        };
        shell.pointer_scrolled(-scrolled.x * scale, -scrolled.y * scale);
    }
}

fn draw_hud(world: &mut World) {
    let Ok(window) = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
    else {
        return;
    };
    let width = window.physical_width();
    let height = window.physical_height();
    let density = window.scale_factor();
    if width == 0 || height == 0 {
        return;
    }
    let image = world.resource::<HudImage>().0.id();
    let resize = world
        .resource::<Assets<Image>>()
        .get(image)
        .is_some_and(|image| image.size() != UVec2::new(width, height));
    if resize && let Some(mut image) = world.resource_mut::<Assets<Image>>().get_mut(image) {
        image.resize(wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        });
    }
    if world.get_non_send::<HudView>().is_none() {
        let Some(gpu) = world.get_resource::<HudGpu>().map(|gpu| gpu.0.clone()) else {
            return;
        };
        let factory = Arc::clone(&world.resource::<HudContent>().0);
        let content = factory(world);
        match AppLauncher::new().create_guest_view(
            &gpu,
            CRANPOSE_FORMAT,
            width,
            height,
            density,
            content,
        ) {
            Ok(mut view) => {
                view.shell().renderer().set_transparent_background(true);
                world.insert_non_send(HudView {
                    view,
                    drawn_into: None,
                });
            }
            Err(error) => {
                error!("Cranpose HUD: {error}");
                return;
            }
        }
    }
    let target = world
        .resource::<HudShared>()
        .0
        .lock()
        .ok()
        .and_then(|shared| shared.texture.clone());
    let Some(mut hud) = world.get_non_send_mut::<HudView>() else {
        return;
    };
    if let Err(error) = hud.view.resize(width, height, density) {
        error!("Cranpose HUD resize: {error}");
        return;
    }
    let Some(target) = target else {
        return;
    };
    if (target.texture.width(), target.texture.height()) != (width, height) {
        return;
    }
    if hud.drawn_into.as_ref() != Some(&target.texture) {
        hud.view.shell().mark_dirty();
        hud.drawn_into = Some(target.texture.clone());
    }
    if let Err(error) = hud.view.draw(&target.texture, &target.view) {
        error!("Cranpose HUD draw: {error}");
    }
}
