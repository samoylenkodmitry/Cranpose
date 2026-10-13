//! Fonts installed while an app runs lay out the text they cover from the
//! next frame on, as in an app started with them. Text the app's own fonts
//! cover keeps its layout, though the installed fonts have Latin letters too.

use std::{cell::RefCell, path::Path, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_render_common::installed_fonts::{install_font_bytes, install_font_file};
use cranpose_render_pixels::PixelsRenderer;
use cranpose_ui::{
    Column, ColumnSpec, Modifier, Text, TextStyle,
    text::{FontFamily, FontWeight},
};
use cranpose_ui_graphics::Size;

/// Hindi, Arabic and English: the embedded default font has glyphs only for
/// the English line.
const LINES: [&str; 3] = ["नमस्ते दुनिया", "مرحبا بالعالم", "Hello world"];

type Captured = Rc<RefCell<Vec<MutableState<Size>>>>;

#[composable]
fn Greetings(captured: Captured) {
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        captured.borrow_mut().clear();
        for line in LINES {
            let size = rememberMutableStateOf(|| Size::ZERO);
            captured.borrow_mut().push(size);
            Text(
                line,
                Modifier::empty().report_size_state(size),
                TextStyle::default(),
            );
        }
    });
}

/// A shell showing the greetings, and the sizes their lines took in the
/// last frame.
fn greetings_shell() -> (AppShell<PixelsRenderer>, impl Fn() -> Vec<Size>) {
    let captured: Captured = Rc::default();
    let content = Rc::clone(&captured);
    let shell = AppShell::new_with_size(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || Greetings(Rc::clone(&content)),
        (320, 240),
        (320.0, 240.0),
    );
    let sizes = move || captured.borrow().iter().map(MutableState::value).collect();
    (shell, sizes)
}

#[test]
fn installed_fonts_lay_out_the_text_they_cover_from_the_next_frame() {
    let (mut running, running_sizes) = greetings_shell();
    running.update();
    let before = running_sizes();

    let fonts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cranpose-fonts/assets");
    install_font_file(
        &FontFamily::named("Noto Sans Devanagari"),
        &fonts.join("NotoSansDevanagari-VF.ttf"),
        &[FontWeight::NORMAL, FontWeight::BOLD],
    )
    .expect("the Devanagari font installs from its file");
    install_font_bytes(
        &FontFamily::named("Noto Sans Arabic"),
        include_bytes!("../../../cranpose-fonts/assets/NotoSansArabic-VF.ttf"),
        &[FontWeight::NORMAL],
    )
    .expect("the Arabic font installs from its bytes");
    running.update();
    running.update();
    let after = running_sizes();

    let (mut started, started_sizes) = greetings_shell();
    started.update();
    assert_ne!(
        before[0], after[0],
        "the Devanagari font must lay the Hindi line out"
    );
    assert_ne!(
        before[1], after[1],
        "the Arabic font must lay the Arabic line out"
    );
    assert_eq!(
        before[2], after[2],
        "the English line must keep the app's own face, though both installed fonts have Latin letters"
    );
    assert_eq!(
        after,
        started_sizes(),
        "the running app must lay the lines out as an app started after the installs"
    );
}
