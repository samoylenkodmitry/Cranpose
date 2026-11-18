// The checks of a layer's color filter on text, run through the pixels
// renderer, and a kept frame that takes each new filter.

#[path = "../../common/tests/support/filtered_word.rs"]
mod filtered_word;
#[path = "support/kept_frames.rs"]
mod kept_frames;

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_render_pixels::PixelsRenderer;
use cranpose_ui_graphics::{Color, ColorFilter};
use filtered_word::{FilteredFrames, FilteredLayer, FilteredWord, HEIGHT, WIDTH};
use kept_frames::KeptFrames;

struct PixelsFrames(AppShell<PixelsRenderer>);

impl FilteredFrames for PixelsFrames {
    fn compose(content: impl FnMut() + 'static) -> Self {
        Self(AppShell::new_with_size(
            PixelsRenderer::new(),
            location_key(file!(), line!(), column!()),
            content,
            (WIDTH, HEIGHT),
            (WIDTH as f32, HEIGHT as f32),
        ))
    }

    fn rgba(&mut self) -> Vec<u8> {
        self.0.update();
        self.0.update();
        let mut frame = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
        self.0
            .primary()
            .with_renderer(|renderer| renderer.draw(&mut frame, WIDTH, HEIGHT));
        frame
    }
}

#[test]
fn a_layer_color_filter_reaches_text_that_has_its_own_color() {
    filtered_word::reaches_text_that_has_its_own_color::<PixelsFrames>();
}

#[test]
fn a_layer_color_filter_reaches_stroked_text_that_has_its_own_color() {
    filtered_word::reaches_stroked_text_that_has_its_own_color::<PixelsFrames>();
}

#[test]
fn a_changed_layer_color_filter_repaints_the_color_of_a_span() {
    filtered_word::repaints_the_color_of_a_span::<PixelsFrames>();
}

#[test]
fn a_parent_layer_color_filter_reaches_text_that_has_its_own_color() {
    filtered_word::a_parent_filter_reaches_text_that_has_its_own_color::<PixelsFrames>();
}

#[test]
fn a_kept_frame_takes_each_new_layer_color_filter_of_a_text() {
    let mut word = FilteredWord::<PixelsFrames>::new(
        "Saved".into(),
        filtered_word::black(None),
        Color::WHITE,
        FilteredLayer::Word,
    );
    let filters = [
        None,
        Some(ColorFilter::tint(Color::RED)),
        Some(filtered_word::swap_red_green()),
        Some(ColorFilter::modulate(Color::WHITE.with_alpha(0.5))),
        None,
    ];
    let filter = word.filter;
    KeptFrames::new((WIDTH, HEIGHT), 1.0).check(
        &mut word.frames.0,
        (
            u32::try_from(filters.len()).expect("a step for each filter"),
            1,
        ),
        |step| filter.set(filters[step as usize].clone()),
    );
}
