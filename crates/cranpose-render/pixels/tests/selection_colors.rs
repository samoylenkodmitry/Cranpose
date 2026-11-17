// Selected text shows the `TextSelectionColors` provided above it, and the
// default ones without: in a `SelectionContainer`, and in a text field, whose
// handles take the colors too while its cursor color stays the caret's.

use cranpose_app_shell::{AppShell, PointerSource};
use cranpose_core::{CompositionLocalProvider, location_key, remember};
use cranpose_macros::composable;
use cranpose_render_pixels::PixelsRenderer;
use cranpose_ui::{
    BasicTextFieldOptions, BasicTextFieldWithOptions, Color, Modifier, Text, TextFieldState,
    TextSelectionColors, TextStyle, local_text_selection_colors, text::TextUnit,
    widgets::SelectionContainer,
};

const WIDTH: u32 = 200;
const HEIGHT: u32 = 80;
const WORD: &str = "iiiiii";
// A point inside the word's line, between the strokes of its letters.
const PROBE: (usize, usize) = (6, 12);
// Where a pointer presses the word.
const PRESS: (f32, f32) = (10.0, 12.0);
const RED: TextSelectionColors = TextSelectionColors {
    handle_color: Color(1.0, 0.0, 0.0, 1.0),
    background_color: Color(1.0, 0.0, 0.0, 1.0),
};

// What shows the word.
#[derive(Clone, Copy, PartialEq)]
enum Shown {
    Container,
    Field,
}

fn style() -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.font_size = TextUnit::Sp(20.0);
    style.span_style.color = Some(Color::BLACK);
    style
}

#[composable]
fn Word(shown: Shown) {
    match shown {
        Shown::Container => SelectionContainer(Modifier::empty(), || {
            Text(WORD, Modifier::empty(), style());
        }),
        Shown::Field => {
            let state = remember(|| TextFieldState::new(WORD)).with(|state| *state);
            BasicTextFieldWithOptions(
                state,
                Modifier::empty(),
                BasicTextFieldOptions {
                    text_style: style(),
                    // A caret color unlike the selection colors, which must
                    // not take it.
                    cursor_color: Color(0.0, 1.0, 0.0, 1.0),
                    ..BasicTextFieldOptions::default()
                },
            );
        }
    }
}

#[composable]
fn Words(shown: Shown, colors: Option<TextSelectionColors>) {
    match colors {
        Some(colors) => {
            CompositionLocalProvider([local_text_selection_colors().provides(colors)], || {
                Word(shown);
            });
        }
        None => Word(shown),
    }
}

// The window after two presses of `source` select the word.
fn selected(shown: Shown, colors: Option<TextSelectionColors>, source: PointerSource) -> Vec<u8> {
    let mut shell = AppShell::new_with_size_and_density(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        move || Words(shown, colors),
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
        1.0,
    );
    shell.update();
    shell.set_pointer_source(source);
    for _ in 0..2 {
        shell.set_cursor(PRESS.0, PRESS.1);
        shell.pointer_pressed();
        shell.pointer_released();
    }
    shell.update();
    let mut image = vec![0; (WIDTH * HEIGHT * 4) as usize];
    shell
        .primary()
        .with_renderer(|renderer| renderer.draw_scaled(&mut image, WIDTH, HEIGHT, 1.0));
    image
}

fn probe(image: &[u8]) -> [u8; 3] {
    let at = (PROBE.1 * WIDTH as usize + PROBE.0) * 4;
    [image[at], image[at + 1], image[at + 2]]
}

fn red_pixels(image: &[u8]) -> usize {
    image
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] > 200 && pixel[1] < 60 && pixel[2] < 60)
        .count()
}

#[test]
fn a_selection_shows_the_provided_highlight() {
    for shown in [Shown::Container, Shown::Field] {
        let [r, g, b] = probe(&selected(shown, Some(RED), PointerSource::Mouse));
        assert!(
            r > 200 && g < 60 && b < 60,
            "the provided red shows: {r} {g} {b}"
        );
        let [r, g, b] = probe(&selected(shown, None, PointerSource::Mouse));
        assert!(
            b > r + 40 && b > g,
            "the default blue shows without one: {r} {g} {b}"
        );
    }
}

#[test]
fn a_field_selected_by_touch_shows_its_handles_in_the_provided_color() {
    let image = selected(Shown::Field, Some(RED), PointerSource::Touch);
    let line = (0..2 * PROBE.1)
        .flat_map(|y| {
            image[y * WIDTH as usize * 4..(y + 1) * WIDTH as usize * 4]
                .as_chunks::<4>()
                .0
                .iter()
        })
        .filter(|pixel| pixel[0] > 200 && pixel[1] < 60 && pixel[2] < 60)
        .count();
    let handles = red_pixels(&image) - line;
    assert!(
        handles > 20,
        "the handles below the word show in the provided red: {handles} px"
    );
}
