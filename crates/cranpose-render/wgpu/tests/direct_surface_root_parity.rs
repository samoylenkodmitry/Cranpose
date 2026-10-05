use support::{page::*, read_texture};

use crate::support;

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 240;

/// Rounded cards under a frosted pane whose backdrop reads the page, or
/// under a plain pane.
fn cards(backdrop: bool) {
    FramePage(
        FRAME_WIDTH,
        FRAME_HEIGHT,
        Color(0.05, 0.06, 0.14, 1.0),
        move || {
            for i in 0..10u32 {
                let x = (i as f32 * 67.0) % 300.0;
                let y = (i as f32 * 41.0) % 220.0;
                Box(
                    rect_modifier([x, y, 22.0, 22.0])
                        .background(Color(0.9, 0.5 + (i % 4) as f32 * 0.1, 0.3, 1.0))
                        .rounded_corners(11.0),
                    BoxSpec::new(),
                    || {},
                );
            }
            let pane = rect_modifier([24.0, 40.0, 272.0, 90.0]);
            let pane = if backdrop {
                pane.backdrop_effect(RenderEffect::blur(7.0))
            } else {
                pane
            };
            Box(
                pane.background(Color(1.0, 1.0, 1.0, 0.18))
                    .rounded_corners(16.0),
                BoxSpec::new(),
                || {
                    Text(
                        "Direct surface",
                        Modifier::empty().offset(16.0, 16.0),
                        TextStyle::default(),
                    );
                },
            );
        },
    );
}

#[composable]
fn CardsPage() {
    cards(true);
}

#[composable]
fn PlainCardsPage() {
    cards(false);
}

/// A presentable image that can be rendered into and copied: Metal's.
const UNSAMPLED: wgpu::TextureUsages = wgpu::TextureUsages::RENDER_ATTACHMENT
    .union(wgpu::TextureUsages::COPY_SRC)
    .union(wgpu::TextureUsages::COPY_DST);
/// A presentable image the renderer can also sample.
const SAMPLED: wgpu::TextureUsages = UNSAMPLED.union(wgpu::TextureUsages::TEXTURE_BINDING);

struct Frames {
    captured: Vec<u8>,
    presented: Vec<u8>,
    /// The passes of the presented frame.
    passes: u32,
}

fn render_frames(page: fn(), usage: wgpu::TextureUsages) -> Option<Frames> {
    let (_lock, mut shell) = support::app_shell_for(
        page,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Rgba8Unorm,
        |_| {},
    )?;
    shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("warm-up capture should succeed");
    let captured = shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("capture should succeed");

    let device = shell.renderer().try_device().expect("device").clone();
    let queue = shell
        .renderer()
        .try_queue_for_tests()
        .expect("queue")
        .clone();
    let (texture, view) = support::render_target(
        &device,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Rgba8Unorm,
        usage,
    );
    for _ in 0..2 {
        shell
            .renderer()
            .render(&texture, &view, FRAME_WIDTH, FRAME_HEIGHT)
            .expect("presentable render should succeed");
    }
    assert_eq!(shell.renderer().device_error_count_for_tests(), 0);
    let passes = shell
        .renderer()
        .last_frame_stats()
        .expect("frame stats")
        .pass_count;
    Some(Frames {
        captured: captured.pixels,
        presented: read_texture(&device, &queue, &texture),
        passes,
    })
}

#[test]
fn a_frame_rendered_straight_into_the_presentable_image_matches_the_converted_capture() {
    let Some(frames) = render_frames(CardsPage, SAMPLED) else {
        return;
    };
    assert!(
        frames
            .captured
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[0] > 200 && px[2] < 120),
        "the scene must show its orange dots"
    );
    support::assert_same_bytes(
        "direct vs captured",
        FRAME_WIDTH,
        &frames.presented,
        &frames.captured,
    );
}

/// A frame without backdrops never reads its page, so it draws straight into
/// an image the renderer cannot sample, without the output conversion.
#[test]
fn a_frame_without_backdrops_draws_straight_into_an_image_it_cannot_sample() {
    let (Some(unsampled), Some(sampled)) = (
        render_frames(PlainCardsPage, UNSAMPLED),
        render_frames(PlainCardsPage, SAMPLED),
    ) else {
        return;
    };
    support::assert_same_bytes(
        "direct into an unsampled image vs captured",
        FRAME_WIDTH,
        &unsampled.presented,
        &unsampled.captured,
    );
    assert_eq!(
        unsampled.passes, sampled.passes,
        "both images are the frame's root"
    );
}

/// A backdrop reads the page, so beside an image the renderer cannot sample
/// the frame composes in its own target and converts it into the image.
#[test]
fn a_frame_whose_backdrop_reads_the_page_composes_beside_an_image_it_cannot_sample() {
    let (Some(unsampled), Some(sampled)) = (
        render_frames(CardsPage, UNSAMPLED),
        render_frames(CardsPage, SAMPLED),
    ) else {
        return;
    };
    support::assert_same_bytes(
        "composed then converted vs captured",
        FRAME_WIDTH,
        &unsampled.presented,
        &unsampled.captured,
    );
    assert_eq!(
        unsampled.passes,
        sampled.passes + 1,
        "the unsampled image takes the output conversion pass"
    );
}
