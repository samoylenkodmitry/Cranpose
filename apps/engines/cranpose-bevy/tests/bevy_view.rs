use std::{
    cell::OnceCell,
    rc::Rc,
    time::{Duration, Instant},
};

use bevy::{
    camera::RenderTarget,
    core_pipeline::tonemapping::Tonemapping,
    prelude::{App, Camera3d, ClearColor, Color as BevyColor, Commands, Startup},
};
use cranpose::{AppLauncher, FrameEffect, embedded_view::EmbeddedView, gpu_context, prelude::*};
use cranpose_bevy::BevyView;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;

fn clear_to(color: BevyColor) -> impl FnOnce(&mut App, RenderTarget) + Send + 'static {
    move |app, target| {
        app.insert_resource(ClearColor(color));
        app.add_systems(Startup, move |mut commands: Commands| {
            commands.spawn((Camera3d::default(), Tonemapping::None, target.clone()));
        });
    }
}

// Draws frames until the pixel in the middle is `expected`.
fn wait_for(view: &mut EmbeddedView, expected: [u8; 4]) -> bool {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut pixels = Vec::new();
    while Instant::now() < deadline {
        view.draw(&mut pixels).expect("frame");
        let start = ((HEIGHT / 2 * WIDTH + WIDTH / 2) * 4) as usize;
        if pixels.get(start..start + 4) == Some(&expected[..]) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn a_bevy_app_on_its_own_thread_shows_in_a_composition() {
    let bevy: Rc<OnceCell<Rc<BevyView>>> = Rc::new(OnceCell::new());
    let composed = Rc::clone(&bevy);
    let mut view = AppLauncher::new()
        .with_gpu_requirements(cranpose_bevy::gpu_requirements())
        .create_embedded_view(WIDTH, HEIGHT, 1.0, move || {
            let Some(gpu) = gpu_context() else {
                return;
            };
            let running = composed
                .get_or_init(|| {
                    let red = clear_to(BevyColor::srgb(1.0, 0.0, 0.0));
                    Rc::new(BevyView::new(&gpu, red).expect("Bevy thread"))
                })
                .clone();
            let frame = rememberMutableStateOf(|| None::<ImageBitmap>);
            FrameEffect((), true, move |_| {
                if let Some(shown) = running.show(WIDTH, HEIGHT) {
                    frame.set(Some(shown));
                }
            });
            Box(
                Modifier::empty().fill_max_size().draw_behind(move |scope| {
                    if let Some(frame) = frame.value() {
                        scope.draw_image_at(Rect::from_size(scope.size()), frame, 1.0, None);
                    }
                }),
                BoxSpec::default(),
                || {},
            );
        })
        .expect("GPU embedded component");
    assert!(wait_for(&mut view, [255, 0, 0, 255]), "Bevy's red frame");

    let bevy = bevy.get().expect("composed");
    bevy.send(|world| world.insert_resource(ClearColor(BevyColor::srgb(0.0, 0.0, 1.0))));
    assert!(
        wait_for(&mut view, [0, 0, 255, 255]),
        "the command's blue frame"
    );
}
