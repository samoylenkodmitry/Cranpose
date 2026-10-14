//! A renderer given fonts lays text out and draws it in them instead of the
//! embedded default font.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_render_pixels::PixelsRenderer;
use cranpose_ui::{Modifier, Text, TextStyle};
use cranpose_ui_graphics::Size;

static BOLD: &[u8] = include_bytes!("../../common/assets/NotoSansBold.ttf");

type Captured = Rc<RefCell<Option<MutableState<Size>>>>;

#[composable]
fn Line(captured: Captured) {
    let size = rememberMutableStateOf(|| Size::ZERO);
    *captured.borrow_mut() = Some(size);
    Text(
        "Wide words in a line",
        Modifier::empty().report_size_state(size),
        TextStyle::default(),
    );
}

fn line_width(renderer: PixelsRenderer) -> f32 {
    let captured: Captured = Rc::default();
    let content = Rc::clone(&captured);
    let mut shell = AppShell::new_with_size(
        renderer,
        location_key(file!(), line!(), column!()),
        move || Line(Rc::clone(&content)),
        (320, 80),
        (320.0, 80.0),
    );
    shell.update();
    shell.update();
    let size = (*captured.borrow()).expect("the composition remembered the size");
    size.value().width
}

#[test]
fn text_lays_out_in_the_fonts_the_renderer_was_given() {
    let default_width = line_width(PixelsRenderer::new());
    let bold_width = line_width(PixelsRenderer::with_fonts(&[BOLD]));
    assert!(default_width > 0.0);
    assert!(
        bold_width > default_width,
        "the bold face is wider: {bold_width} against {default_width}"
    );
}

#[test]
fn fonts_that_do_not_parse_leave_the_default_font() {
    let default_width = line_width(PixelsRenderer::new());
    let fallback_width = line_width(PixelsRenderer::with_fonts(&[b"not a font"]));
    assert_eq!(fallback_width, default_width);
}
