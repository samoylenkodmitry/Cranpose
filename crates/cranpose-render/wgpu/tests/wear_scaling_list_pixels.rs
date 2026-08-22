//! The Wear scaling list, counted in framebuffer pixels.
//!
//! This is the assertion the widget set shipped without. Every test it had
//! stopped at the `LayoutTree`, which a measure policy fills by returning
//! `Placement`s; the app draws from the retained node state instead, and the
//! scene build drops anything that state does not mark placed. A list can
//! therefore measure every row to the pixel and put none of them on the glass.
//!
//! So this runs the whole path an app runs — compose, `AppShell::update`,
//! scene build, wgpu — and counts what lit up.

mod support;

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::lazy::LazyItems;
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui::round_scaling_list::CentreAnchor;
use cranpose_ui::widgets::wear::{
    rememberWearScalingListState, ListHeader, ListHeaderSpec, WearColors, WearScalingLazyColumn,
    WearScalingLazyColumnSpec, WearTextStyle,
};
use cranpose_ui::{Color, Modifier, Text};

/// A 454x454 watch, rendered one framebuffer pixel per layout point so the
/// count below is in the same units the screen is specified in.
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
                        // A header and a row are different shapes; keeping them
                        // in separate pools stops one being reused as the other.
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

    // Four rows of text on a black watch face. The number is a floor, not a
    // golden: what it rules out is the failure this test exists for, where the
    // list measured every row and drew none of them and the frame came back
    // black. A single row's worth of glyphs already clears it.
    assert!(
        lit > 1_000,
        "a four-row Wear list drew {lit} lit pixels into a {SIZE}x{SIZE} frame; the rows never \
         reached the framebuffer"
    );
}
