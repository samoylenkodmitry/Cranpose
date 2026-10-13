//! A font file installed while an app runs lays out the text it covers from
//! the next frame on, as in an app started with the font.

use std::{cell::RefCell, path::Path, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_render_common::installed_fonts::install_font_file;
use cranpose_render_pixels::PixelsRenderer;
use cranpose_ui::{
    Modifier, Text, TextStyle,
    text::{FontFamily, FontWeight},
};
use cranpose_ui_graphics::Size;

/// Hindi, which the embedded default font has no glyphs for.
const GREETING: &str = "नमस्ते दुनिया";

type Captured = Rc<RefCell<Option<MutableState<Size>>>>;

#[composable]
fn Greeting(captured: Captured) {
    let size = rememberMutableStateOf(|| Size::ZERO);
    *captured.borrow_mut() = Some(size);
    Text(
        GREETING,
        Modifier::empty().report_size_state(size),
        TextStyle::default(),
    );
}

/// A shell showing the greeting, and the size its text took in the last frame.
fn greeting_shell() -> (AppShell<PixelsRenderer>, impl Fn() -> Size) {
    let captured: Captured = Rc::default();
    let content = Rc::clone(&captured);
    let shell = AppShell::new_with_size(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || Greeting(Rc::clone(&content)),
        (320, 240),
        (320.0, 240.0),
    );
    let size = move || {
        captured
            .borrow()
            .as_ref()
            .map_or(Size::ZERO, MutableState::value)
    };
    (shell, size)
}

#[test]
fn an_installed_font_lays_out_the_text_it_covers_from_the_next_frame() {
    let (mut running, running_size) = greeting_shell();
    running.update();
    let before = running_size();

    let font = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../cranpose-fonts/assets/NotoSansDevanagari-VF.ttf");
    install_font_file(
        &FontFamily::named("Noto Sans Devanagari"),
        &font,
        &[FontWeight::NORMAL, FontWeight::BOLD],
    )
    .expect("the Devanagari font installs");
    running.update();
    running.update();
    let after = running_size();

    let (mut started, started_size) = greeting_shell();
    started.update();
    assert_ne!(
        before, after,
        "the installed font must change how the running app lays the greeting out"
    );
    assert_eq!(
        after,
        started_size(),
        "the running app must lay the greeting out as an app started after the install"
    );
}
