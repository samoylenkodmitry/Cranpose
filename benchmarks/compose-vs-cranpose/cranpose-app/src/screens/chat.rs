//! The chat beside the patchwork (`../../../README.md`): an agent's answers
//! arrive from a data thread as tokens and the list shows them as they come.
//!
//! In Cranpose's idiom the list reads only the number of blocks, so a token
//! composes the last block alone, the one that reads how much of it shows.
//! The spinner turns in a graphics layer; the animated picture and the
//! moving gradient each read the frame in a composable of their own.

use std::{cell::OnceCell, rc::Rc};

use cranpose::{LazyItems, WebView, prelude::*};
use cranpose_ui::{
    text::{AnnotatedString, FontFamily, FontWeight, SpanStyle},
    widgets::rememberSvg,
};

use super::{
    text_style,
    wall::{INK, rgb},
};
use crate::data::{self, CHAT_TURN, ChatBlock, ChatToken};

// The family of the monospaced face the launcher loads after Roboto.
const MONO_FAMILY: &str = "DejaVu Sans Mono";

const MUTED: Color = rgb(data::CHAT_SHIMMER_RGB[0]);
const PANEL: Color = rgb(data::CHAT_PANEL_RGB);
const BUBBLE: Color = rgb(data::CHAT_USER_RGB);

/// What the chat shows: its blocks so far and how much of the last one.
/// The frame loop writes both as tokens arrive.
#[derive(Clone, Copy, PartialEq)]
pub struct ChatShown {
    pub blocks: MutableState<usize>,
    pub last: MutableState<usize>,
    pub list: LazyListState,
}

impl ChatShown {
    /// Takes the frame's tokens from `feed` and keeps the list at its end.
    pub fn advance(self, feed: &data::ChatFeed) {
        for _ in 0..data::CHAT_TOKENS_PER_FRAME {
            let Some(ChatToken { block, shown }) = feed.take() else {
                break;
            };
            if self.blocks.get_non_reactive() != block + 1 {
                self.blocks.set(block + 1);
            }
            self.last.set(shown);
        }
        self.list.dispatch_scroll_delta(-1.0e6);
    }
}

#[composable]
pub fn Chat(frame: MutableState<u32>, shown: ChatShown) {
    let count = shown.blocks.get();
    LazyColumn(
        Modifier::empty()
            .width(data::CHAT_WIDTH)
            .fill_max_height()
            .background(rgb(data::CHAT_BACKGROUND_RGB)),
        shown.list,
        LazyColumnSpec::new()
            .vertical_arrangement(LinearArrangement::spaced_by(8.0))
            .content_padding_all(8.0),
        move |scope| {
            scope.items(
                LazyItems::new(count)
                    .key(|index| index as u64)
                    .content_type(|index| (index % CHAT_TURN.len()) as u64),
                move |index| Block(index, index + 1 == count, frame, shown.last),
            );
        },
    );
}

// Block `index` of the chat; `live` while it is the last one, the one that
// reads how much of it `shown` tokens show.
#[composable]
fn Block(index: usize, live: bool, frame: MutableState<u32>, shown: MutableState<usize>) {
    match CHAT_TURN[index % CHAT_TURN.len()] {
        ChatBlock::User(text) => User(text),
        ChatBlock::Progress(label) => Progress(label, live, frame),
        ChatBlock::Paragraph(text) => Paragraph(ChatToken::turn(index), text, live, shown),
        ChatBlock::Code(code) => Code(ChatToken::turn(index), code, live, shown),
        ChatBlock::Table(rows) => Table(rows, live, shown),
        ChatBlock::Gif => Gif(frame),
        ChatBlock::Web => Web(),
        ChatBlock::Buttons(labels) => Buttons(labels),
    }
}

#[composable]
fn User(text: &'static str) {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::End),
        move || {
            Text(
                text,
                Modifier::empty()
                    .background(BUBBLE)
                    .rounded_corners(10.0)
                    .padding_symmetric(10.0, 6.0),
                text_style(data::CHAT_TEXT, INK, false),
            );
        },
    );
}

#[composable]
fn Progress(label: &'static str, live: bool, frame: MutableState<u32>) {
    Row(
        Modifier::empty(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(6.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let spinner = rememberSvg(data::CHAT_SVG.as_bytes())
                .map_err(|error| log::warn!("chat spinner: {error}"));
            if let Ok(spinner) = spinner {
                Image(
                    Painter::from_svg(spinner),
                    None,
                    Modifier::empty()
                        .size_points(16.0, 16.0)
                        .graphics_layer(move || GraphicsLayer {
                            rotation_z: if live {
                                frame.get() as f32 * data::CHAT_SPIN_DEGREES
                            } else {
                                0.0
                            },
                            ..Default::default()
                        }),
                    Alignment::CENTER,
                    ContentScale::Fit,
                    1.0,
                    None,
                );
            }
            if live {
                Shimmer(label, frame);
            } else {
                Text(
                    label,
                    Modifier::empty(),
                    text_style(data::CHAT_TEXT, MUTED, false),
                );
            }
        },
    );
}

// The live progress line's label under a gradient that moves each frame.
#[composable]
fn Shimmer(label: &'static str, frame: MutableState<u32>) {
    let start = (frame.get() as f32 * data::CHAT_SHIMMER_STEP) % data::CHAT_SHIMMER_SPAN;
    let mut style = text_style(data::CHAT_TEXT, INK, false);
    style.span_style.color = None;
    style.span_style.brush = Some(Brush::horizontal_gradient_tiled(
        vec![MUTED, rgb(data::CHAT_SHIMMER_RGB[1]), MUTED],
        start,
        start + data::CHAT_SHIMMER_SPAN,
        TileMode::Repeated,
    ));
    Text(label, Modifier::empty(), style);
}

#[composable]
fn Paragraph(turn: usize, text: &'static str, live: bool, shown: MutableState<usize>) {
    let chars = if live {
        data::chat_shown_chars(text, shown.get())
    } else {
        text.len()
    };
    Text(
        format!("{} {}", data::chat_turn_label(turn), &text[..chars]),
        Modifier::empty().fill_max_width(),
        text_style(data::CHAT_TEXT, INK, false),
    );
}

// The whole block colored once: a comment that names the turn, then `code`.
fn highlighted(turn: usize, code: &'static str) -> (usize, AnnotatedString) {
    let head = format!("// {}\n", data::chat_turn_label(turn));
    let colored = |kind: data::CodeKind| SpanStyle {
        color: Some(rgb(kind.rgb())),
        ..Default::default()
    };
    let mut builder = AnnotatedString::builder()
        .push_style(colored(data::CodeKind::Comment))
        .append(&head)
        .pop();
    let mut at = 0;
    for (span, kind) in data::chat_code_spans(code) {
        builder = builder
            .append(&code[at..span.start])
            .push_style(colored(kind))
            .append(&code[span.clone()])
            .pop();
        at = span.end;
    }
    (
        head.len(),
        builder.append(&code[at..]).to_annotated_string(),
    )
}

#[composable]
fn Code(turn: usize, code: &'static str, live: bool, shown: MutableState<usize>) {
    let whole = rememberKeyed(turn, move |_| Rc::new(highlighted(turn, code)));
    Box(
        Modifier::empty()
            .fill_max_width()
            .background(PANEL)
            .rounded_corners(6.0)
            .padding(8.0),
        BoxSpec::default(),
        move || CodeText(whole.clone(), code.len(), live, shown),
    );
}

#[composable]
fn CodeText(
    whole: Rc<(usize, AnnotatedString)>,
    length: usize,
    live: bool,
    shown: MutableState<usize>,
) {
    let chars = if live {
        (shown.get() * data::CHAT_TOKEN_CHARS).min(length)
    } else {
        length
    };
    let mut style = text_style(data::CHAT_CODE_TEXT, INK, false);
    style.span_style.font_family = Some(FontFamily::named(MONO_FAMILY));
    Text(
        whole.1.subsequence(0..whole.0 + chars),
        Modifier::empty(),
        style,
    );
}

#[composable]
fn Table(
    rows: &'static [[&'static str; data::CHAT_TABLE_COLUMNS]],
    live: bool,
    shown: MutableState<usize>,
) {
    let count = if live {
        shown.get().min(rows.len())
    } else {
        rows.len()
    };
    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::default(),
        move || {
            for (index, row) in rows[..count].iter().enumerate() {
                TableRow(row, index == 0, index % 2 == 1);
            }
        },
    );
}

#[composable]
fn TableRow(row: &'static [&'static str; data::CHAT_TABLE_COLUMNS], header: bool, shaded: bool) {
    let fill = if header || shaded {
        PANEL
    } else {
        Color::TRANSPARENT
    };
    Row(
        Modifier::empty()
            .fill_max_width()
            .background(fill)
            .padding_symmetric(6.0, 3.0),
        RowSpec::default(),
        move || {
            for cell in row {
                let mut style = text_style(data::CHAT_CODE_TEXT, INK, false);
                style.span_style.font_weight = header.then_some(FontWeight::BOLD);
                Text(*cell, Modifier::empty().weight(1.0), style);
            }
        },
    );
}

thread_local! {
    // The animated picture's frames, decoded once for every block that
    // shows it.
    static GIF_FRAMES: OnceCell<Rc<[ImageBitmap]>> = const { OnceCell::new() };
}

// Every frame of the GIF as a bitmap; none when it does not decode.
fn decode_gif() -> Rc<[ImageBitmap]> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut frames = Vec::new();
    match options.read_info(data::CHAT_GIF) {
        Ok(mut decoder) => {
            let (width, height) = (u32::from(decoder.width()), u32::from(decoder.height()));
            while let Ok(Some(frame)) = decoder.read_next_frame() {
                match ImageBitmap::from_rgba8(width, height, frame.buffer.to_vec()) {
                    Ok(bitmap) => frames.push(bitmap),
                    Err(error) => log::warn!("chat picture frame: {error}"),
                }
            }
        }
        Err(error) => log::warn!("chat picture: {error}"),
    }
    frames.into()
}

// The animated picture: the one composable of its block that reads the frame.
#[composable]
fn Gif(frame: MutableState<u32>) {
    let frames = GIF_FRAMES.with(|frames| frames.get_or_init(decode_gif).clone());
    if frames.is_empty() {
        return;
    }
    Image(
        BitmapPainter(frames[data::chat_gif_frame(frame.get(), frames.len())].clone()),
        None,
        Modifier::empty().size_points(data::CHAT_GIF_SIZE, data::CHAT_GIF_SIZE),
        Alignment::CENTER,
        ContentScale::Fit,
        1.0,
        None,
    );
}

#[composable]
fn Web() {
    let (width, height) = data::CHAT_WEB_SIZE;
    WebView(
        data::chat_html_url(),
        Modifier::empty().size_points(width, height),
        |_| {},
    );
}

#[composable]
fn Buttons(labels: &'static [&'static str]) {
    Row(
        Modifier::empty(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(8.0)),
        move || {
            for label in labels {
                Button(
                    Modifier::empty()
                        .background(BUBBLE)
                        .rounded_corners(6.0)
                        .padding_symmetric(10.0, 6.0),
                    ButtonSpec::new(),
                    || {},
                    move || {
                        Text(
                            *label,
                            Modifier::empty(),
                            text_style(data::CHAT_TEXT, INK, true),
                        );
                    },
                );
            }
        },
    );
}
