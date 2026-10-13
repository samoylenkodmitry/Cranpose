use std::{cell::OnceCell, rc::Rc};

use cranpose::{
    AppLauncher, ExternalFrames, FrameWriter, GpuContext, embedded_view::EmbeddedView, gpu_context,
    prelude::*,
};

use crate::gpu_support::{fill, pixel};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;
const FRAME: u32 = 8;
const COLORS: [(wgpu::Color, [u8; 4]); 3] = [
    (wgpu::Color::RED, [255, 0, 0, 255]),
    (wgpu::Color::GREEN, [0, 255, 0, 255]),
    (wgpu::Color::BLUE, [0, 0, 255, 255]),
];

struct Scene {
    gpu: GpuContext,
    frames: ExternalFrames,
    shown: MutableState<Option<ImageBitmap>>,
}

struct Harness {
    view: EmbeddedView,
    scene: Rc<OnceCell<Scene>>,
    pixels: Vec<u8>,
}

impl Harness {
    fn new() -> Self {
        let scene: Rc<OnceCell<Scene>> = Rc::new(OnceCell::new());
        let composed = Rc::clone(&scene);
        let mut view = AppLauncher::new()
            .create_embedded_view(WIDTH, HEIGHT, 1.0, move || {
                let Some(gpu) = gpu_context() else {
                    return;
                };
                let shown = rememberMutableStateOf(|| None::<ImageBitmap>);
                composed.get_or_init(|| {
                    let frames = ExternalFrames::new(
                        &gpu,
                        wgpu::TextureFormat::Rgba8Unorm,
                        wgpu::TextureUsages::RENDER_ATTACHMENT,
                        true,
                    );
                    frames.set_size(FRAME, FRAME);
                    Scene { gpu, frames, shown }
                });
                if let Some(frame) = shown.value() {
                    Image(
                        frame,
                        None,
                        Modifier::empty().fill_max_size(),
                        Alignment::CENTER,
                        ContentScale::FillBounds,
                        1.0,
                        None,
                    );
                }
            })
            .expect("GPU embedded component");
        let mut pixels = Vec::new();
        view.draw(&mut pixels).expect("first frame");
        Self {
            view,
            scene,
            pixels,
        }
    }

    fn scene(&self) -> &Scene {
        self.scene.get().expect("composed")
    }

    fn writer(&self) -> FrameWriter {
        self.scene().frames.writer()
    }

    /// Renders one frame on another thread and returns the texture it used.
    fn render_elsewhere(&self, color: wgpu::Color) -> wgpu::Texture {
        let writer = self.writer();
        let gpu = self.scene().gpu.clone();
        std::thread::spawn(move || {
            let target = writer.begin().expect("a free texture");
            fill(&gpu, target.texture(), color);
            writer.publish(target.number());
            target.texture().clone()
        })
        .join()
        .expect("writer thread")
    }

    /// Shows the newest published frame and returns the pixel in the middle.
    fn draw_next(&mut self) -> [u8; 4] {
        let scene = self.scene();
        if let Some(frame) = scene.frames.next_frame() {
            scene.shown.set(Some(frame));
        }
        self.view.draw(&mut self.pixels).expect("frame");
        let middle = pixel(&self.pixels, WIDTH, WIDTH / 2, HEIGHT / 2);
        [middle[0], middle[1], middle[2], middle[3]]
    }
}

#[test]
fn a_composition_shows_the_newest_frame_another_thread_published() {
    let mut harness = Harness::new();
    for (color, _) in COLORS {
        harness.render_elsewhere(color);
    }
    assert_eq!(harness.draw_next(), COLORS[2].1);
    assert!(harness.scene().frames.next_frame().is_none());
}

#[test]
fn a_writer_never_gets_a_texture_that_a_bitmap_still_draws() {
    let mut harness = Harness::new();
    let kept_texture = harness.render_elsewhere(COLORS[0].0);
    harness.draw_next();
    let kept = harness.scene().shown.value();
    for round in 1..12 {
        let (color, expected) = COLORS[round % COLORS.len()];
        let texture = harness.render_elsewhere(color);
        assert_ne!(texture, kept_texture, "round {round}");
        assert_eq!(harness.draw_next(), expected, "round {round}");
    }
    drop(kept);
    let reused = (0..4).any(|round| {
        let texture = harness.render_elsewhere(COLORS[round % COLORS.len()].0);
        harness.draw_next();
        texture == kept_texture
    });
    assert!(reused, "the texture comes back once its bitmap is gone");
}

#[test]
fn a_new_size_shows_once_a_frame_of_that_size_is_published() {
    let mut harness = Harness::new();
    assert!(
        harness.scene().frames.writer().begin().is_some(),
        "a size was asked for"
    );
    harness.render_elsewhere(COLORS[0].0);
    assert_eq!(harness.draw_next(), COLORS[0].1);

    harness.scene().frames.set_size(FRAME / 2, FRAME / 2);
    assert_eq!(harness.draw_next(), COLORS[0].1, "the old frame stays");
    let texture = harness.render_elsewhere(COLORS[1].0);
    assert_eq!((texture.width(), texture.height()), (FRAME / 2, FRAME / 2));
    assert_eq!(harness.draw_next(), COLORS[1].1);
}

#[test]
fn a_target_that_is_never_published_goes_back_to_the_writer() {
    let mut harness = Harness::new();
    for round in 0..12 {
        let writer = harness.writer();
        let skipped = writer.begin().expect("a free texture");
        let (color, expected) = COLORS[round % COLORS.len()];
        let texture = harness.render_elsewhere(color);
        assert_ne!(&texture, skipped.texture());
        assert_eq!(harness.draw_next(), expected, "round {round}");
    }
}
