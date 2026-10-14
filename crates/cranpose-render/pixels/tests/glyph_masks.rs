//! A renderer keeps the masks of the glyphs it drew: a text that changes
//! draws the same pixels as a renderer that never drew text before.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_macros::composable;
use cranpose_render_pixels::PixelsRenderer;
use cranpose_ui::{Column, ColumnSpec, Modifier, SpanStyle, Text, TextStyle, text::TextUnit};

const WIDTH: u32 = 200;
const HEIGHT: u32 = 90;

type Count = Rc<RefCell<Option<MutableState<u32>>>>;

fn style(size: f32) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(size),
            ..SpanStyle::default()
        },
        ..TextStyle::default()
    }
}

#[composable]
fn Counter(start: u32, captured: Count) {
    let count = rememberMutableStateOf(move || start);
    *captured.borrow_mut() = Some(count);
    let n = count.value();
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(format!("Running for {n} s"), Modifier::empty(), style(14.0));
        Text(format!("{n} of 90"), Modifier::empty(), style(22.0));
    });
}

fn shell(start: u32) -> (AppShell<PixelsRenderer>, Count) {
    let captured: Count = Rc::default();
    let content = Rc::clone(&captured);
    let shell = AppShell::new_with_size(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || Counter(start, Rc::clone(&content)),
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
    );
    (shell, captured)
}

fn frame(shell: &mut AppShell<PixelsRenderer>) -> Vec<u8> {
    shell.update();
    let mut pixels = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    shell
        .primary()
        .with_renderer(|renderer| renderer.draw(&mut pixels, WIDTH, HEIGHT));
    pixels
}

#[test]
fn changed_text_draws_as_a_fresh_renderer_draws_it() {
    let (mut kept, captured) = shell(0);
    frame(&mut kept);
    for n in [1, 10, 19, 88, 9] {
        let count = (*captured.borrow()).expect("the composition remembered the count");
        count.set(n);
        let drawn = frame(&mut kept);
        let pixels = drawn.as_chunks::<4>().0;
        assert!(
            pixels.iter().any(|pixel| *pixel != pixels[0]),
            "{n}: the frame shows no text"
        );
        let (mut fresh, _) = shell(n);
        assert!(
            drawn == frame(&mut fresh),
            "{n}: the renderer that drew earlier texts drew different pixels"
        );
    }
}
