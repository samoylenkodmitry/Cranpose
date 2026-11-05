// What the changed-areas tests share: the shells they step and the check
// that a frame a host keeps, redrawn only in the areas the renderer
// reports, stays equal to a full redraw.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_render_common::{RenderScene, Renderer};
use cranpose_render_pixels::{PixelRect, PixelsRenderer};
use cranpose_ui::{MemoryHeadroom, set_memory_headroom};
use cranpose_ui_graphics::Color;

pub(crate) const WIDTH: u32 = 240;
pub(crate) const HEIGHT: u32 = 180;
pub(crate) const FRAMES: u32 = 24;

pub(crate) type Step = Rc<RefCell<Option<MutableState<u32>>>>;

pub(crate) fn color(step: u32, salt: u32) -> Color {
    let value = (step * 37 + salt * 91) % 255;
    Color::from_rgb_u8(value as u8, (255 - value) as u8, ((value * 3) % 255) as u8)
}

pub(crate) fn shell(scale: f32, scenes: fn(Step)) -> (AppShell<PixelsRenderer>, Step) {
    let captured: Step = Rc::default();
    let content = Rc::clone(&captured);
    let shell = AppShell::new_with_size_and_density(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || scenes(Rc::clone(&content)),
        (WIDTH, HEIGHT),
        (WIDTH as f32 / scale, HEIGHT as f32 / scale),
        scale,
    );
    (shell, captured)
}

fn apply(kept: &mut [u8], area: PixelRect, pixels: &[u8]) {
    let row_bytes = area.width as usize * 4;
    for (row, source) in pixels.chunks_exact(row_bytes).enumerate() {
        let start = ((area.y as usize + row) * WIDTH as usize + area.x as usize) * 4;
        kept[start..start + row_bytes].copy_from_slice(source);
    }
}

fn first_difference(kept: &[u8], full: &[u8]) -> Option<(usize, usize)> {
    let pixel = kept
        .as_chunks::<4>()
        .0
        .iter()
        .zip(full.as_chunks::<4>().0)
        .position(|(a, b)| a != b)?;
    Some((pixel % WIDTH as usize, pixel / WIDTH as usize))
}

// A shell updated in place draws only the areas it reports into a kept
// frame; a second shell of the same scenes, whose scene goes before each
// update that renders its root again, builds it anew and draws it whole.
pub(crate) fn kept_frames_match_full_redraws(scale: f32, scenes: fn(Step)) {
    kept_frames_match_full_redraws_with(scale, MemoryHeadroom::Ample, scenes);
}

// As `kept_frames_match_full_redraws`, with `headroom` the memory both
// shells' host reports left.
pub(crate) fn kept_frames_match_full_redraws_with(
    scale: f32,
    headroom: MemoryHeadroom,
    scenes: fn(Step),
) {
    let (mut shell, captured) = shell(scale, scenes);
    let (mut fresh, fresh_captured) = self::shell(scale, scenes);
    for host in [&shell, &fresh] {
        host.app_context().enter(|| set_memory_headroom(headroom));
    }
    let mut areas = Vec::new();
    let mut kept = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let mut full = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let mut area_pixels = Vec::new();
    let set_step = |captured: &Step, frame: u32| {
        if frame > 0 {
            let step = (*captured.borrow()).expect("the composition remembered the step");
            step.set(frame);
        }
    };
    for frame in 0..FRAMES {
        set_step(&captured, frame);
        shell.update();
        shell.primary().with_renderer(|renderer| {
            renderer.update_picture(WIDTH, HEIGHT, &mut areas);
            for area in &areas {
                area_pixels.resize(area.width as usize * area.height as usize * 4, 0);
                renderer.draw_picture_area(*area, &mut area_pixels);
                apply(&mut kept, *area, &area_pixels);
            }
        });
        set_step(&fresh_captured, frame);
        fresh
            .primary()
            .with_renderer(|renderer| renderer.scene_mut().clear());
        fresh.request_root_render();
        fresh.update();
        fresh
            .primary()
            .with_renderer(|renderer| renderer.draw(&mut full, WIDTH, HEIGHT));
        assert_eq!(
            first_difference(&kept, &full),
            None,
            "frame {frame} at scale {scale}: the kept frame differs from a full redraw at (x, y); areas {areas:?}"
        );
    }
}
