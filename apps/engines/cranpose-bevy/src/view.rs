use std::{
    sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel},
    thread::JoinHandle,
};

use bevy::{
    app::PluginsState,
    camera::{ManualTextureViewHandle, RenderTarget},
    math::UVec2,
    prelude::*,
    render::{
        Render, RenderApp, RenderPlugin, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        texture::{ManualTextureView, ManualTextureViews},
    },
    tasks::tick_global_task_pools_on_main_thread,
    window::ExitCondition,
};
use cranpose::{ExternalFrames, FrameWriter, GpuContext, prelude::ImageBitmap};

const TARGET: ManualTextureViewHandle = ManualTextureViewHandle(0);
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

type Command = Box<dyn FnOnce(&mut World) + Send>;

/// A Bevy app that runs on its own thread and renders with Cranpose's device,
/// for a composition to show.
///
/// The composition calls [`Self::show`] once per frame. Each call lets Bevy
/// run one update and returns the newest frame Bevy finished, so the
/// composition never waits for Bevy, and Bevy never waits for the composition.
///
/// Bevy must be built without its window plugins (the `bevy_winit` feature),
/// since windows belong to Cranpose and Bevy's thread is not the main thread.
pub struct BevyView {
    frames: ExternalFrames,
    ticks: Option<SyncSender<()>>,
    commands: Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Resource, Clone, Copy, ExtractResource)]
#[extract_app(RenderApp)]
struct FrameNumber(u64);

#[derive(Resource)]
struct Publisher(FrameWriter);

impl BevyView {
    /// Starts Bevy on its own thread. `setup` adds the scene and gets the
    /// render target its camera draws into.
    pub fn new(
        gpu: &GpuContext,
        setup: impl FnOnce(&mut App, RenderTarget) + Send + 'static,
    ) -> std::io::Result<Self> {
        let frames = ExternalFrames::new(gpu, FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT, true);
        let writer = frames.writer();
        let (ticks, tick_receiver) = sync_channel(1);
        let (commands, command_receiver) = channel();
        let gpu = gpu.clone();
        let thread = std::thread::Builder::new()
            .name("bevy".into())
            .spawn(move || {
                let app = start(&gpu, writer.clone(), setup);
                run(app, &writer, &tick_receiver, &command_receiver);
            })?;
        Ok(Self {
            frames,
            ticks: Some(ticks),
            commands,
            thread: Some(thread),
        })
    }

    /// Asks Bevy for frames of `width` by `height` pixels, lets it run one
    /// more update, and returns the newest frame it finished since the last
    /// call.
    pub fn show(&self, width: u32, height: u32) -> Option<ImageBitmap> {
        self.frames.set_size(width, height);
        if let Some(ticks) = self.ticks.as_ref() {
            // A full channel means Bevy has not started the last update yet.
            let _ = ticks.try_send(());
        }
        self.frames.next_frame()
    }

    /// Runs `command` on Bevy's world before its next update.
    pub fn send(&self, command: impl FnOnce(&mut World) + Send + 'static) {
        let _ = self.commands.send(Box::new(command));
    }
}

impl Drop for BevyView {
    fn drop(&mut self) {
        self.ticks = None;
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            error!("Bevy's thread panicked");
        }
    }
}

fn start(gpu: &GpuContext, writer: FrameWriter, setup: impl FnOnce(&mut App, RenderTarget)) -> App {
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
    )
    .add_plugins(ExtractResourcePlugin::<FrameNumber>::default());
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render
            .insert_resource(Publisher(writer))
            .add_systems(Render, publish.in_set(RenderSystems::Cleanup));
    }
    setup(&mut app, RenderTarget::TextureView(TARGET));
    while app.plugins_state() == PluginsState::Adding {
        tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();
    app
}

fn run(mut app: App, writer: &FrameWriter, ticks: &Receiver<()>, commands: &Receiver<Command>) {
    while ticks.recv().is_ok() {
        for command in commands.try_iter() {
            command(app.world_mut());
        }
        // Every texture is in use while the composition has not drawn the
        // frames published so far, so Bevy waits for the next tick.
        let Some(target) = writer.begin() else {
            continue;
        };
        let texture = target.texture();
        let world = app.world_mut();
        world.resource_mut::<ManualTextureViews>().insert(
            TARGET,
            ManualTextureView {
                texture_view: target.view().clone().into(),
                size: UVec2::new(texture.width(), texture.height()),
                view_format: FORMAT,
            },
        );
        world.insert_resource(FrameNumber(target.number()));
        app.update();
    }
}

// Runs in the render world after it submitted the frame's work.
fn publish(frame: Option<Res<FrameNumber>>, publisher: Res<Publisher>) {
    if let Some(frame) = frame {
        publisher.0.publish(frame.0);
    }
}
