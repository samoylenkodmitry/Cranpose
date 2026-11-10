//! The chat beside the patchwork (`../README.md`): an agent's answers arrive
//! from a data thread as tokens and the list shows them as they come.
//!
//! In Xilem's idiom a block is memoized on its number and on how much of it
//! shows, so a token rebuilds the last block alone and a finished block is
//! skipped. The list holds the blocks of the last two turns in a `tail`
//! column, which lays out and paints only the blocks in view and stashes the
//! others, so a token that grows the last block moves the text up in the
//! layout pass with no state to carry the height. The animated picture is
//! one drawing whose frame the chat picks and which repaints without a
//! layout. The spinner is the SVG rasterized once and turned by its holder's
//! transform. Masonry's labels have one color and a line 1.2 em tall, so all
//! the chat's text is `Rich` text: lines 1.4 em tall, the status under a
//! gradient and the code in its colors.

use std::sync::OnceLock;

use perf_data::*;
use xilem::{
    AnyWidgetView, Blob, Color, ImageBrush, ImageFormat, Vec2, WidgetView,
    core::memoize,
    masonry::{
        kurbo::Point,
        peniko::{Brush, Extend, Gradient, ImageAlphaType, ImageData},
    },
    style::{Padding, Style as _},
    view::{
        CrossAxisAlignment, FlexExt as _, FlexSpacer, button, flex_col, flex_row, image, sized_box,
    },
};

use super::{
    px, rgb,
    wall::INK,
    widgets::{
        Hold, Picture, RichText, RichView, drawing, gif_frames, hold, ignore_height, rich, tail,
    },
};

const MONO: &str = "DejaVu Sans Mono";
const PANEL: Color = rgb(CHAT_PANEL_RGB);
const BUBBLE: Color = rgb(CHAT_USER_RGB);
// The turns whose blocks the list holds: more than it shows.
const TURNS_HELD: usize = 2;

type Boxed<State> = Box<AnyWidgetView<State>>;

pub(crate) fn chat<State: 'static>(
    frame: u32,
    token: Option<ChatToken>,
) -> impl WidgetView<State> + use<State> {
    let blocks = token
        .map(|token| {
            let first =
                (token.block / CHAT_TURN.len()).saturating_sub(TURNS_HELD - 1) * CHAT_TURN.len();
            (first..=token.block)
                .map(|block| {
                    let shown = match CHAT_TURN[block % CHAT_TURN.len()] {
                        ChatBlock::Gif => chat_gif_frame(frame, gif_frames().len()),
                        _ if block == token.block => token.shown,
                        _ => usize::MAX,
                    };
                    memoize((block, shown), |&(block, shown)| {
                        written::<State>(block, shown)
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let list = tail(blocks, 8.0, 8.0);
    sized_box(
        flex_col((list.flex(1.0), status(frame, token)))
            .gap(px(0.0))
            .cross_axis_alignment(CrossAxisAlignment::Start),
    )
    .width(px(CHAT_WIDTH))
    .expand_height()
    .background_color(rgb(CHAT_BACKGROUND_RGB))
}

// The status line under the list: a spinner that turns each frame and what
// the agent does, under a gradient that moves each frame.
fn status<State: 'static>(
    frame: u32,
    token: Option<ChatToken>,
) -> impl WidgetView<State> + use<State> {
    let spinner = hold(
        sized_box(image(spinner().clone()))
            .width(px(CHAT_SPINNER_SIZE))
            .height(px(CHAT_SPINNER_SIZE)),
        Hold::Turn {
            offset: Vec2::ZERO,
            radians: f64::from(frame as f32 * CHAT_SPIN_DEGREES).to_radians(),
        },
        ignore_height,
    );
    let [dim, bright] = CHAT_SHIMMER_RGB.map(rgb);
    let start = f64::from((frame as f32 * CHAT_SHIMMER_STEP) % CHAT_SHIMMER_SPAN);
    let shimmer = Gradient::new_linear(
        Point::new(start, 0.0),
        Point::new(start + f64::from(CHAT_SHIMMER_SPAN), 0.0),
    )
    .with_extend(Extend::Repeat)
    .with_stops([dim, bright, dim]);
    sized_box(
        flex_row((
            spinner,
            rich(RichText {
                text: token.map(chat_status).unwrap_or_default(),
                spans: Vec::new(),
                brushes: vec![Brush::Gradient(shimmer)],
                size: CHAT_TEXT,
                family: "Roboto",
                bold: false,
                wrap: false,
            }),
        ))
        .gap(px(6.0))
        .cross_axis_alignment(CrossAxisAlignment::Center),
    )
    .expand_width()
    .height(px(CHAT_STATUS_HEIGHT))
    .padding(Padding::horizontal(8.0))
    .background_color(PANEL)
}

// Block `block` of the chat as far as `shown` tokens show it; for the
// animated picture, `shown` is the bitmap to show.
fn written<State: 'static>(block: usize, shown: usize) -> Boxed<State> {
    let turn = ChatToken::turn(block);
    match CHAT_TURN[block % CHAT_TURN.len()] {
        ChatBlock::User(said) => Box::new(flex_row((
            FlexSpacer::Flex(1.0),
            sized_box(plain(said.to_owned(), CHAT_TEXT, false, false))
                .padding(Padding {
                    top: 6.0,
                    bottom: 6.0,
                    left: 10.0,
                    right: 10.0,
                })
                .corner_radius(10.0)
                .background_color(BUBBLE),
        ))),
        ChatBlock::Heading(said) => Box::new(typed(turn, said, shown, CHAT_HEADING_TEXT, true)),
        ChatBlock::Paragraph(said) => Box::new(typed(turn, said, shown, CHAT_TEXT, false)),
        ChatBlock::Code(code) => Box::new(
            sized_box(rich(highlighted(turn, code, shown)))
                .expand_width()
                .padding(8.0)
                .corner_radius(6.0)
                .background_color(PANEL),
        ),
        ChatBlock::Table(rows) => Box::new(
            flex_col(
                rows.iter()
                    .take(shown)
                    .enumerate()
                    .map(|(index, cells)| {
                        let shaded = index == 0 || index % 2 == 1;
                        sized_box(
                            flex_row(
                                cells
                                    .iter()
                                    .map(|cell| {
                                        sized_box(plain(
                                            (*cell).to_owned(),
                                            CHAT_CODE_TEXT,
                                            index == 0,
                                            false,
                                        ))
                                        .expand_width()
                                        .flex(1.0)
                                    })
                                    .collect::<Vec<_>>(),
                            )
                            .gap(px(0.0)),
                        )
                        .expand_width()
                        .padding(Padding {
                            top: 3.0,
                            bottom: 3.0,
                            left: 6.0,
                            right: 6.0,
                        })
                        .background_color(if shaded {
                            PANEL
                        } else {
                            Color::TRANSPARENT
                        })
                    })
                    .collect::<Vec<_>>(),
            )
            .gap(px(0.0)),
        ),
        ChatBlock::Gif => Box::new(drawing(Picture::Gif {
            frame: shown,
            side: f64::from(CHAT_GIF_SIZE),
        })),
        ChatBlock::Buttons(labels) => Box::new(
            flex_row(
                labels
                    .iter()
                    .map(|label| {
                        button(
                            plain((*label).to_owned(), CHAT_TEXT, true, false),
                            |_: &mut State| {},
                        )
                        .background_color(BUBBLE)
                        .corner_radius(6.0)
                        .padding(Padding {
                            top: 6.0,
                            bottom: 6.0,
                            left: 10.0,
                            right: 10.0,
                        })
                    })
                    .collect::<Vec<_>>(),
            )
            .gap(px(8.0)),
        ),
    }
}

// A paragraph, or a heading in the bold face: the turn's number, then as
// much of `said` as arrived.
fn typed(turn: usize, said: &'static str, shown: usize, size: f32, heading: bool) -> RichView {
    let chars = chat_shown_chars(said, shown.min(said.len()));
    plain(
        format!("{} {}", chat_turn_label(turn), &said[..chars]),
        size,
        heading,
        true,
    )
}

// `text` in Roboto and the ink color.
fn plain(text: String, size: f32, bold: bool, wrap: bool) -> RichView {
    rich(RichText {
        text,
        spans: Vec::new(),
        brushes: vec![Brush::from(INK)],
        size,
        family: "Roboto",
        bold,
        wrap,
    })
}

// A comment that names the turn, then as much of `code` as arrived, each
// stretch in its color.
fn highlighted(turn: usize, code: &'static str, shown: usize) -> RichText {
    let chars = chat_shown_chars(code, shown.min(code.len()));
    let head = format!("// {}\n", chat_turn_label(turn));
    let brush = |kind: CodeKind| match kind {
        CodeKind::Keyword => 1,
        CodeKind::Text => 2,
        CodeKind::Number => 3,
        CodeKind::Comment => 4,
    };
    let mut spans = vec![(0..head.len(), brush(CodeKind::Comment))];
    for (span, kind) in chat_code_spans(code) {
        if span.start >= chars {
            break;
        }
        spans.push((
            head.len() + span.start..head.len() + span.end.min(chars),
            brush(kind),
        ));
    }
    RichText {
        text: format!("{head}{}", &code[..chars]),
        spans,
        brushes: [INK]
            .into_iter()
            .chain(
                [
                    CodeKind::Keyword,
                    CodeKind::Text,
                    CodeKind::Number,
                    CodeKind::Comment,
                ]
                .map(|kind| rgb(kind.rgb())),
            )
            .map(Brush::from)
            .collect(),
        size: CHAT_CODE_TEXT,
        family: MONO,
        bold: false,
        wrap: true,
    }
}

// The spinner's SVG at four times its size, rasterized once: Masonry has no
// SVG widget.
fn spinner() -> &'static ImageBrush {
    static SPINNER: OnceLock<ImageBrush> = OnceLock::new();
    SPINNER.get_or_init(|| {
        let side = (CHAT_SPINNER_SIZE * 4.0) as u32;
        let mut pixels = tiny_skia::Pixmap::new(side, side).expect("a spinner of some size");
        match usvg::Tree::from_str(CHAT_SVG, &usvg::Options::default()) {
            Ok(tree) => resvg::render(
                &tree,
                tiny_skia::Transform::from_scale(
                    side as f32 / tree.size().width(),
                    side as f32 / tree.size().height(),
                ),
                &mut pixels.as_mut(),
            ),
            Err(error) => log::warn!("chat spinner: {error}"),
        }
        bitmap(
            pixels.take(),
            side,
            side,
            ImageAlphaType::AlphaPremultiplied,
        )
    })
}

fn bitmap(rgba: Vec<u8>, width: u32, height: u32, alpha_type: ImageAlphaType) -> ImageBrush {
    ImageBrush::new(ImageData {
        data: Blob::from(rgba),
        format: ImageFormat::Rgba8,
        alpha_type,
        width,
        height,
    })
}
