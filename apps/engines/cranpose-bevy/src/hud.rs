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
    wanted: Vec<AssetId<Image>>,
    textures: Vec<(AssetId<Image>, HudTexture)>,
}

#[derive(Clone)]
struct HudTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct HudImage {
    handle: Handle<Image>,
    size: UVec2,
}

#[derive(Resource)]
struct HudImages {
    material: Handle<HudMaterial>,
    shown: HudImage,
    pending: Option<HudImage>,
}

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

fn hud_image(images: &mut Assets<Image>, size: UVec2) -> HudImage {
    let mut image = Image::new_target_texture(size.x, size.y, CRANPOSE_FORMAT, Some(HUD_FORMAT));
    image.data = None;
    image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    HudImage {
        handle: images.add(image),
        size,
    }
}

fn spawn_hud(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<HudMaterial>>,
) {
    let shown = hud_image(&mut images, UVec2::ONE);
    let material = materials.add(HudMaterial {
        texture: shown.handle.clone(),
    });
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        MaterialNode(material.clone()),
        GlobalZIndex(i32::MAX),
    ));
    commands.insert_resource(HudImages {
        material,
        shown,
        pending: None,
    });
}

fn share_target(images: Res<RenderAssets<GpuImage>>, shared: Res<HudShared>) {
    let Ok(mut shared) = shared.0.lock() else {
        return;
    };
    let SharedTarget { wanted, textures } = &mut *shared;
    textures.retain(|(id, _)| wanted.contains(id));
    for id in wanted.iter() {
        let Some(gpu_image) = images.get(*id) else {
            continue;
        };
        let texture: &wgpu::Texture = &gpu_image.texture;
        if textures
            .iter()
            .any(|(shared_id, shared)| shared_id == id && shared.texture == *texture)
        {
            continue;
        }
        textures.retain(|(shared_id, _)| shared_id != id);
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(CRANPOSE_FORMAT),
            ..default()
        });
        textures.push((
            *id,
            HudTexture {
                texture: texture.clone(),
                view,
            },
        ));
    }
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
    let size = UVec2::new(window.physical_width(), window.physical_height());
    let density = window.scale_factor();
    if size.x == 0 || size.y == 0 || !ensure_view(world, density) {
        return;
    }
    let shared = world.resource::<HudShared>().clone();
    let texture_of = |image: &HudImage| {
        shared.0.lock().ok().and_then(|shared| {
            shared
                .textures
                .iter()
                .find(|(id, _)| *id == image.handle.id())
                .map(|(_, texture)| texture.clone())
        })
    };
    let images = world.resource::<HudImages>();
    let ready = images
        .pending
        .as_ref()
        .and_then(|pending| texture_of(pending).map(|texture| (pending.size, texture, true)))
        .or_else(|| texture_of(&images.shown).map(|texture| (images.shown.size, texture, false)));
    if let Some((target_size, target, promote)) = ready {
        draw_into(world, target_size, density, &target);
        if promote {
            promote_pending(world);
        }
    }
    world.resource_scope(|world, mut images: Mut<HudImages>| {
        let wanted_size = images
            .pending
            .as_ref()
            .map_or(images.shown.size, |pending| pending.size);
        if wanted_size != size {
            images.pending = Some(hud_image(&mut world.resource_mut::<Assets<Image>>(), size));
        }
        if let Ok(mut shared) = shared.0.lock() {
            shared.wanted.clear();
            shared.wanted.push(images.shown.handle.id());
            shared
                .wanted
                .extend(images.pending.as_ref().map(|pending| pending.handle.id()));
        }
    });
}

fn ensure_view(world: &mut World, density: f32) -> bool {
    if world.get_non_send::<HudView>().is_some() {
        return true;
    }
    let Some(gpu) = world.get_resource::<HudGpu>().map(|gpu| gpu.0.clone()) else {
        return false;
    };
    let factory = Arc::clone(&world.resource::<HudContent>().0);
    let content = factory(world);
    match AppLauncher::new().create_guest_view(&gpu, CRANPOSE_FORMAT, 1, 1, density, content) {
        Ok(mut view) => {
            view.shell().renderer().set_transparent_background(true);
            world.insert_non_send(HudView {
                view,
                drawn_into: None,
            });
            true
        }
        Err(error) => {
            error!("Cranpose HUD: {error}");
            false
        }
    }
}

fn draw_into(world: &mut World, size: UVec2, density: f32, target: &HudTexture) {
    let Some(mut hud) = world.get_non_send_mut::<HudView>() else {
        return;
    };
    if let Err(error) = hud.view.resize(size.x, size.y, density) {
        error!("Cranpose HUD resize: {error}");
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

fn promote_pending(world: &mut World) {
    world.resource_scope(|world, mut images: Mut<HudImages>| {
        let Some(pending) = images.pending.take() else {
            return;
        };
        if let Some(mut material) = world
            .resource_mut::<Assets<HudMaterial>>()
            .get_mut(&images.material)
        {
            material.texture = pending.handle.clone();
        }
        images.shown = pending;
    });
}
