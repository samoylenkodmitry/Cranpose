//! A font file installed while an app runs draws the text it covers from the
//! next frame on, as in an app started with the font, though the running
//! renderer drew that text with missing glyphs before.

#[path = "../src/record_columns.rs"]
#[allow(dead_code)]
mod record_columns;
#[path = "../src/test_support.rs"]
#[allow(dead_code)]
mod shared_test_support;
#[allow(dead_code, unused_imports)]
mod support;

use std::path::Path;

use cranpose_macros::composable;
use cranpose_render_common::installed_fonts::install_font_file;
use cranpose_ui::{
    Modifier, Text, TextStyle,
    text::{FontFamily, FontWeight, SpanStyle, TextUnit},
};
use cranpose_ui_graphics::Color;
use support::page::FramePage;

const WIDTH: u32 = 240;
const HEIGHT: u32 = 64;

#[composable]
fn GreetingPage() {
    FramePage(WIDTH, HEIGHT, Color::WHITE, || {
        // Hindi, which the embedded default font has no glyphs for.
        let style = TextStyle::from_span_style(SpanStyle {
            color: Some(Color::BLACK),
            font_size: TextUnit::Sp(20.0),
            ..Default::default()
        });
        Text("नमस्ते दुनिया", Modifier::empty().padding(8.0), style);
    });
}

#[test]
fn an_installed_font_draws_in_a_running_app_as_in_one_started_with_it() {
    let Some((lock, mut running)) = support::app_shell_for(
        GreetingPage,
        WIDTH,
        HEIGHT,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        |_| {},
    ) else {
        return;
    };
    let (_, before) = support::update_and_capture(&mut running, WIDTH, HEIGHT);

    let font = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cranpose-fonts/assets/NotoSansDevanagari-VF.ttf");
    install_font_file(
        &FontFamily::named("Noto Sans Devanagari"),
        &font,
        &[FontWeight::NORMAL],
    )
    .expect("the Devanagari font installs");
    support::update_and_capture(&mut running, WIDTH, HEIGHT);
    let (_, after) = support::update_and_capture(&mut running, WIDTH, HEIGHT);
    drop(running);
    drop(lock);

    let (_lock, mut started) = support::app_shell_for(
        GreetingPage,
        WIDTH,
        HEIGHT,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        |_| {},
    )
    .expect("headless WGPU init failed mid-test");
    let (_, fresh) = support::update_and_capture(&mut started, WIDTH, HEIGHT);
    assert_ne!(
        before.pixels, after.pixels,
        "the installed font must change how the running app draws the greeting"
    );
    support::assert_same_bytes(
        "the running app after the install against an app started with the font",
        WIDTH,
        &after.pixels,
        &fresh.pixels,
    );
}
