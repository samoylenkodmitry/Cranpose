// A graphics layer's color filter reaches the glyphs of its text, whatever
// colors are named by the text's style and spans. A changed filter repaints
// the glyphs. Each renderer runs these checks through its own frames.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_ui::{
    Box, BoxSpec, Modifier, Text,
    text::{AnnotatedString, SpanStyle, TextDrawStyle, TextStyle},
};
use cranpose_ui_graphics::{Color, ColorFilter, GraphicsLayer};

pub(crate) const WIDTH: u32 = 240;
pub(crate) const HEIGHT: u32 = 80;

// A renderer's shell around composed content, WIDTH by HEIGHT.
pub(crate) trait FilteredFrames: Sized {
    fn compose(content: impl FnMut() + 'static) -> Self;

    // The frame's RGBA bytes once the shell has taken its latest state.
    fn rgba(&mut self) -> Vec<u8>;
}

// The layer that carries the filter: the word's own, or the layer of a box
// around it.
#[derive(Clone, Copy)]
pub(crate) enum FilteredLayer {
    Word,
    Parent,
}

// A word drawn over a background under a layer with a filter set by the test.
pub(crate) struct FilteredWord<F> {
    pub(crate) frames: F,
    pub(crate) filter: MutableState<Option<ColorFilter>>,
}

impl<F: FilteredFrames> FilteredWord<F> {
    // `text` in `style` over `background`, the filter on `filtered`.
    pub(crate) fn new(
        text: AnnotatedString,
        style: TextStyle,
        background: Color,
        filtered: FilteredLayer,
    ) -> Self {
        let captured: Rc<RefCell<Option<MutableState<Option<ColorFilter>>>>> = Rc::default();
        let content = Rc::clone(&captured);
        let frames = F::compose(move || {
            let filter = rememberMutableStateOf(|| None::<ColorFilter>);
            *content.borrow_mut() = Some(filter);
            let layer = move || GraphicsLayer {
                color_filter: filter.value(),
                ..Default::default()
            };
            let parent = Modifier::empty().offset(20.0, 20.0);
            let (parent, word) = match filtered {
                FilteredLayer::Word => (parent, Modifier::empty().graphics_layer(layer)),
                FilteredLayer::Parent => (parent.graphics_layer(layer), Modifier::empty()),
            };
            let (text, style) = (text.clone(), style.clone());
            Box(
                Modifier::empty()
                    .size_points(WIDTH as f32, HEIGHT as f32)
                    .background(background),
                BoxSpec::default(),
                move || {
                    let (text, word, style) = (text.clone(), word.clone(), style.clone());
                    Box(parent.clone(), BoxSpec::default(), move || {
                        Text(text.clone(), word.clone(), style.clone());
                    });
                },
            );
        });
        let filter = (*captured.borrow()).expect("the composition remembered the filter");
        Self { frames, filter }
    }

    // The frame's pixels with the layer under `filter`.
    pub(crate) fn pixels(&mut self, filter: Option<ColorFilter>) -> Vec<[u8; 3]> {
        self.filter.set(filter);
        self.frames
            .rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect()
    }
}

fn channel_sum(pixel: [u8; 3]) -> u32 {
    pixel.iter().map(|&channel| u32::from(channel)).sum()
}

fn darkest(pixels: &[[u8; 3]]) -> [u8; 3] {
    *pixels
        .iter()
        .min_by_key(|pixel| channel_sum(**pixel))
        .expect("pixels")
}

fn brightest(pixels: &[[u8; 3]]) -> [u8; 3] {
    *pixels
        .iter()
        .max_by_key(|pixel| channel_sum(**pixel))
        .expect("pixels")
}

// Whether each channel of `pixel` is within a few levels of `target`'s: the
// GPU effect that strokes a text covers its fullest pixel a few levels short.
fn near(pixel: [u8; 3], target: [u8; 3]) -> bool {
    pixel
        .iter()
        .zip(target)
        .all(|(&channel, target)| channel.abs_diff(target) <= 8)
}

pub(crate) fn black(draw_style: Option<TextDrawStyle>) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color::BLACK),
            draw_style,
            ..Default::default()
        },
        ..Default::default()
    }
}

// A filter that swaps red and green and keeps white as it is.
pub(crate) fn swap_red_green() -> ColorFilter {
    #[rustfmt::skip]
    let matrix = [
        0.0, 1.0, 0.0, 0.0, 0.0,
        1.0, 0.0, 0.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 0.0, 1.0, 0.0,
    ];
    ColorFilter::matrix(matrix)
}

pub(crate) fn reaches_text_that_has_its_own_color<F: FilteredFrames>() {
    let mut word = FilteredWord::<F>::new(
        "Saved".into(),
        black(None),
        Color::WHITE,
        FilteredLayer::Word,
    );
    assert_eq!(
        darkest(&word.pixels(None)),
        [0, 0, 0],
        "the word draws black"
    );

    let faded = darkest(&word.pixels(Some(ColorFilter::modulate(Color::WHITE.with_alpha(0.5)))));
    assert!(
        faded.iter().all(|&channel| channel.abs_diff(128) <= 3),
        "half-alpha modulation draws the black word half over white: {faded:?}"
    );

    let red = darkest(&word.pixels(Some(ColorFilter::tint(Color::RED))));
    assert_eq!(red, [255, 0, 0], "a red tint paints the black word red");
}

pub(crate) fn reaches_stroked_text_that_has_its_own_color<F: FilteredFrames>() {
    let stroke = Some(TextDrawStyle::Stroke { width: 3.0 });
    let mut word = FilteredWord::<F>::new(
        "Saved".into(),
        black(stroke),
        Color::WHITE,
        FilteredLayer::Word,
    );
    let outline = darkest(&word.pixels(None));
    assert!(
        near(outline, [0, 0, 0]),
        "the outline draws black: {outline:?}"
    );

    let red = darkest(&word.pixels(Some(ColorFilter::tint(Color::RED))));
    assert!(
        near(red, [255, 0, 0]),
        "a red tint paints the black outline red: {red:?}"
    );
}

pub(crate) fn repaints_the_color_of_a_span<F: FilteredFrames>() {
    let text = AnnotatedString::builder()
        .push_style(SpanStyle {
            color: Some(Color::RED),
            ..Default::default()
        })
        .append("Saved")
        .pop()
        .to_annotated_string();
    let mut word = FilteredWord::<F>::new(
        text,
        TextStyle::default(),
        Color::BLACK,
        FilteredLayer::Word,
    );
    assert_eq!(
        brightest(&word.pixels(None)),
        [255, 0, 0],
        "the span draws red"
    );

    let swapped = word.pixels(Some(swap_red_green()));
    assert_eq!(
        brightest(&swapped),
        [0, 255, 0],
        "the filter that swaps red and green paints the span green"
    );
    assert!(
        swapped.iter().all(|pixel| pixel[0] == 0),
        "no red is left of the span"
    );
}

pub(crate) fn a_parent_filter_reaches_text_that_has_its_own_color<F: FilteredFrames>() {
    let mut word = FilteredWord::<F>::new(
        "Saved".into(),
        black(None),
        Color::WHITE,
        FilteredLayer::Parent,
    );
    assert_eq!(
        darkest(&word.pixels(None)),
        [0, 0, 0],
        "the word draws black"
    );

    let red = darkest(&word.pixels(Some(ColorFilter::tint(Color::RED))));
    assert_eq!(
        red,
        [255, 0, 0],
        "a red tint on the parent paints the black word red"
    );
}
