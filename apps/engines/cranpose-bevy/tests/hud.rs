use std::sync::{Arc, Mutex};

use bevy::{
    app::PluginsState,
    camera::RenderTarget,
    core_pipeline::tonemapping::Tonemapping,
    prelude::{
        App, Assets, Camera3d, ClearColor, Color as BevyColor, Commands, DefaultPlugins, Image, On,
        PluginGroup, ResMut, Startup, WindowPlugin, default,
    },
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{TextureFormat, TextureUsages},
    },
    tasks::tick_global_task_pools_on_main_thread,
    window::ExitCondition,
};
use cranpose::prelude::*;
use cranpose_bevy::hud::{CranposeHudPlugin, HudCamera};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let start = ((y * WIDTH + x) * 4) as usize;
    [
        pixels[start],
        pixels[start + 1],
        pixels[start + 2],
        pixels[start + 3],
    ]
}

#[test]
fn the_hud_draws_over_the_image_of_its_camera() {
    let read: Arc<Mutex<Option<Vec<u8>>>> = Arc::default();
    let sink = Arc::clone(&read);
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: None,
        exit_condition: ExitCondition::DontExit,
        close_when_requested: false,
        ..default()
    }))
    .add_plugins(CranposeHudPlugin::new(|_| {
        Box::new(|| {
            Box(
                Modifier::empty()
                    .size(Size::new(WIDTH as f32 / 2.0, HEIGHT as f32 / 2.0))
                    .background(Color::from_rgb_u8(255, 0, 0)),
                BoxSpec::default(),
                || {},
            );
        })
    }))
    .insert_resource(ClearColor(BevyColor::srgb(0.0, 0.0, 1.0)))
    .add_systems(
        Startup,
        move |mut commands: Commands, mut images: ResMut<Assets<Image>>| {
            let mut image =
                Image::new_target_texture(WIDTH, HEIGHT, TextureFormat::Rgba8UnormSrgb, None);
            image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
            let image = images.add(image);
            commands.spawn((
                Camera3d::default(),
                Tonemapping::None,
                RenderTarget::Image(image.clone().into()),
                HudCamera,
            ));
            let sink = Arc::clone(&sink);
            commands
                .spawn(Readback::texture(image))
                .observe(move |event: On<ReadbackComplete>| {
                    if let Ok(mut read) = sink.lock() {
                        *read = Some(event.data.clone());
                    }
                });
        },
    );
    while app.plugins_state() == PluginsState::Adding {
        tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();

    let mut last = None;
    for _ in 0..600 {
        app.update();
        let pixels = read.lock().expect("readback").take();
        if let Some(pixels) = pixels {
            let seen = (pixel(&pixels, 8, 8), pixel(&pixels, WIDTH - 8, HEIGHT - 8));
            if seen == (RED, BLUE) {
                return;
            }
            last = Some(seen);
        }
    }
    panic!("the HUD never showed over the camera's image; last read {last:?}");
}
