use bevy::{
    app::PluginsState,
    camera::{ManualTextureViewHandle, RenderTarget},
    math::UVec2,
    prelude::*,
    render::{
        RenderPlugin,
        texture::{ManualTextureView, ManualTextureViews},
    },
    tasks::tick_global_task_pools_on_main_thread,
    window::ExitCondition,
};
use cranpose::{
    GpuContext,
    prelude::{ExternalTexture, ImageBitmap, ImageBitmapError},
};

const TARGET: ManualTextureViewHandle = ManualTextureViewHandle(0);
const BEVY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const CRANPOSE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub struct BevyView {
    app: App,
    gpu: GpuContext,
    shown: Option<ExternalTexture>,
    pending: Vec<(ExternalTexture, u64)>,
    frame: u64,
}

impl BevyView {
    pub fn new(
        gpu: &GpuContext,
        width: u32,
        height: u32,
        setup: impl FnOnce(&mut App, RenderTarget),
    ) -> Result<Self, ImageBitmapError> {
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(RenderPlugin {
                    render_creation: crate::render_creation(gpu),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    close_when_requested: false,
                    ..default()
                }),
        );
        setup(&mut app, RenderTarget::TextureView(TARGET));
        while app.plugins_state() == PluginsState::Adding {
            tick_global_task_pools_on_main_thread();
        }
        app.finish();
        app.cleanup();
        let texture = attach_target(&mut app, gpu, width, height)?;
        Ok(Self {
            app,
            gpu: gpu.clone(),
            shown: None,
            pending: vec![(texture, 0)],
            frame: 0,
        })
    }

    pub fn world_mut(&mut self) -> &mut World {
        self.app.world_mut()
    }

    pub fn size(&self) -> (u32, u32) {
        self.pending
            .last()
            .map(|(texture, _)| texture)
            .or(self.shown.as_ref())
            .map_or((0, 0), |texture| (texture.width(), texture.height()))
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), ImageBitmapError> {
        if self.size() != (width, height) {
            let texture = attach_target(&mut self.app, &self.gpu, width, height)?;
            self.pending.push((texture, self.frame));
        }
        Ok(())
    }

    pub fn update(&mut self) -> Option<ImageBitmap> {
        self.app.update();
        self.frame += 1;
        let frame = self.frame;
        if let Some(ready) = self
            .pending
            .iter()
            .rposition(|(_, attached)| frame >= attached + 2)
        {
            self.shown = self
                .pending
                .drain(..=ready)
                .last()
                .map(|(texture, _)| texture);
        }
        self.shown.as_ref().map(|texture| texture.frame(frame))
    }
}

fn attach_target(
    app: &mut App,
    gpu: &GpuContext,
    width: u32,
    height: u32,
) -> Result<ExternalTexture, ImageBitmapError> {
    if width == 0 || height == 0 {
        return Err(ImageBitmapError::InvalidDimensions);
    }
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Bevy view"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: BEVY_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[CRANPOSE_FORMAT],
    });
    let view_of = |format| {
        texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(format),
            ..default()
        })
    };
    app.world_mut().resource_mut::<ManualTextureViews>().insert(
        TARGET,
        ManualTextureView {
            texture_view: view_of(BEVY_FORMAT).into(),
            size: UVec2::new(width, height),
            view_format: BEVY_FORMAT,
        },
    );
    ExternalTexture::new(width, height, true, view_of(CRANPOSE_FORMAT))
}
