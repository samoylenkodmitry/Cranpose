mod support;

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::lazy::LazyItems;
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui::{
    Color, Modifier, Text,
    round_scaling_list::CentreAnchor,
    widgets::wear::{
        ListHeader, ListHeaderSpec, WearColors, WearScalingLazyColumn, WearScalingLazyColumnSpec,
        WearTextStyle, rememberWearScalingListState,
    },
};

const SIZE: u32 = 454;

const HEADERS: [&str; 3] = ["ALPHA", "BRAVO", "CHARLIE"];
const BODY: &str =
    "Every graphic and sound in this build was made for it, on a watch that fits in a pocket.";

fn colors() -> WearColors {
    WearColors {
        content: Color::WHITE,
        on_surface: Color::WHITE,
        background: Color::BLACK,
        ..WearColors::default()
    }
}

fn lit_pixel_count(frame: &CapturedFrame) -> usize {
    frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] > 16 || pixel[1] > 16 || pixel[2] > 16)
        .count()
}

#[test]
fn a_wear_scaling_list_puts_its_rows_on_the_glass() {
    let (_lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!(
                "skipping the Wear scaling list pixel count because headless WGPU init failed: \
                 {err}"
            );
            return;
        }
    };

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(renderer, root_key, || {
        let state = rememberWearScalingListState(CentreAnchor::default());
        WearScalingLazyColumn(
            Modifier::empty()
                .fill_max_size()
                .background(colors().background),
            state,
            WearScalingLazyColumnSpec::default().content_padding(18.0, 34.0),
            |scope| {
                scope.items(
                    LazyItems::new(HEADERS.len() + 1)
                        .key(|index: usize| index as u64)
                        .content_type(|index: usize| u64::from(index >= HEADERS.len())),
                    |index| {
                        if index < HEADERS.len() {
                            ListHeader(
                                Modifier::empty(),
                                ListHeaderSpec::default().colors(colors()),
                                HEADERS[index].to_string(),
                            );
                        } else {
                            Text(
                                BODY.to_string(),
                                Modifier::empty().fill_max_width(),
                                WearTextStyle::BODY_LARGE.resolve(colors().content),
                            );
                        }
                    },
                );
            },
        );
    });
    shell.set_viewport(SIZE as f32, SIZE as f32);
    shell.set_buffer_size(SIZE, SIZE);
    shell.update();

    let frame = shell
        .renderer()
        .capture_frame(SIZE, SIZE)
        .expect("frame capture should succeed");
    let lit = lit_pixel_count(&frame);

    assert!(
        lit > 1_000,
        "a four-row Wear list drew {lit} lit pixels into a {SIZE}x{SIZE} frame; the rows never \
         reached the framebuffer"
    );
}
