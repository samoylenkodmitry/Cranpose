//! The chat beside the patchwork in Rune (`../README.md`): a scroll viewport
//! that follows its end over a growth stack of `virtual_rows`, which builds
//! only the rows in its window. Each block is a memo versioned by how much of
//! it shows, so a token builds the last block alone. The picture is one image
//! per GIF frame that the block's memo selects by version. The spinner is an
//! SVG image that the frame turns. Rune has no gradient for text, so the status
//! line colors each glyph by `chat_shimmer`.

use std::{collections::HashMap, ops::Range};

use perf_data::*;
use rune::{
    Color, El, GrowthSpec, Image, ImageCache, ScrollSpec, Span, Waker, col, el, row, text,
    text::TextSystem,
};

use super::{patchwork::Patchwork, rgb, roboto};

/// The key of the list's viewport.
const LIST: &str = "chat-list";

pub(crate) struct Chat {
    feed: Option<ChatFeed>,
    /// The blocks so far and how much of the last one shows.
    blocks: usize,
    shown: usize,
    token: ChatToken,
    /// The animated picture's frame that the frame shows.
    gif_frame: usize,
    gif: Vec<Image>,
    spinner: Option<Image>,
    /// Keeps the decoder of the pictures alive.
    images: Option<ImageCache>,
    spin: f32,
    status: String,
    shimmer: Vec<Span>,
    text: TextSystem,
    advances: HashMap<char, f32>,
}

impl Chat {
    pub(crate) fn new() -> Self {
        Self {
            feed: ChatFeed::start()
                .map_err(|error| log::warn!("chat data thread: {error}"))
                .ok(),
            blocks: 0,
            shown: 0,
            token: ChatToken::default(),
            gif_frame: 0,
            gif: Vec::new(),
            spinner: None,
            images: None,
            spin: 0.0,
            status: String::new(),
            shimmer: Vec::new(),
            text: TextSystem::new(super::fonts(), ("Roboto", "DejaVu Sans Mono")),
            advances: HashMap::new(),
        }
    }

    /// Decodes the pictures; `wake` redraws the window once they arrive.
    pub(crate) fn setup(&mut self, wake: Waker) {
        let images = ImageCache::new(wake);
        self.spinner = Some(images.load(CHAT_SVG.as_bytes()));
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        match options.read_info(CHAT_GIF) {
            Ok(mut decoder) => {
                let (width, height) = (u32::from(decoder.width()), u32::from(decoder.height()));
                while let Ok(Some(frame)) = decoder.read_next_frame() {
                    self.gif
                        .push(images.rgba(width, height, frame.buffer.to_vec()));
                }
            }
            Err(error) => log::warn!("chat picture: {error}"),
        }
        self.images = Some(images);
    }

    /// Takes the frame's tokens and moves the picture, the spinner and the
    /// status line to frame `frame`.
    pub(crate) fn advance(&mut self, frame: u32) {
        for _ in 0..CHAT_TOKENS_PER_FRAME {
            let Some(token) = self.feed.as_ref().and_then(ChatFeed::take) else {
                break;
            };
            self.blocks = token.block + 1;
            self.shown = token.shown;
            self.token = token;
        }
        self.gif_frame = chat_gif_frame(frame, self.gif.len());
        self.spin = (frame as f32 * CHAT_SPIN_DEGREES).to_radians();
        self.status = chat_status(self.token);
        let [dim, bright] = CHAT_SHIMMER_RGB;
        let mut at = 0.0;
        self.shimmer.clear();
        for (start, character) in self.status.char_indices() {
            let weight = chat_shimmer(at, frame);
            let channel =
                |index: usize| dim[index] + ((bright[index] - dim[index]) as f32 * weight) as u8;
            self.shimmer
                .push(Span::new(start, start + character.len_utf8()).color(rgb([
                    channel(0),
                    channel(1),
                    channel(2),
                ])));
            let text = &mut self.text;
            at += *self.advances.entry(character).or_insert_with(|| {
                text.measure(
                    character.encode_utf8(&mut [0; 4]),
                    roboto(CHAT_TEXT, false),
                    64.0,
                )
                .0
            });
        }
    }

    pub(crate) fn view(chat: &Chat) -> El {
        let list = col()
            .key(LIST)
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .scroll(ScrollSpec { stick_bottom: true })
            .child(
                col()
                    .key("chat-rows")
                    .w_full()
                    .padding(8.0)
                    .gap(8.0)
                    .growth(GrowthSpec::default())
                    .virtual_rows(
                        chat.blocks,
                        40.0,
                        |_: &Patchwork, index| format!("block{index}").into(),
                        |patchwork: &Patchwork, index| block(&patchwork.chat, index),
                    )
                    .virtual_rows_version(()),
            );
        col()
            .w(CHAT_WIDTH)
            .h_full()
            .fill(rgb(CHAT_BACKGROUND_RGB))
            .child(list)
            .child(chat.status_line())
    }

    /// A spinner that turns and the status text under the moving gradient,
    /// both new every frame.
    fn status_line(&self) -> El {
        let spinner = el().size(CHAT_SPINNER_SIZE, CHAT_SPINNER_SIZE);
        let spinner = match &self.spinner {
            Some(image) => spinner.pixels(image.pixels()).rotate(self.spin),
            None => spinner,
        };
        row()
            .w_full()
            .h(CHAT_STATUS_HEIGHT)
            .px(8.0)
            .gap(6.0)
            .center_y()
            .fill(rgb(CHAT_PANEL_RGB))
            .child(spinner)
            .child(
                text(self.status.clone())
                    .spans(self.shimmer.clone())
                    .font(roboto(CHAT_TEXT, false))
                    .color(rgb(CHAT_SHIMMER_RGB[0])),
            )
    }
}

/// Block `index` of the chat: a memo versioned by how much of it shows, which
/// is all of it once a later block began.
fn block(chat: &Chat, index: usize) -> El {
    let kind = CHAT_TURN[index % CHAT_TURN.len()];
    let turn = ChatToken::turn(index);
    let shown = if index + 1 == chat.blocks {
        chat.shown.min(kind.tokens())
    } else {
        kind.tokens()
    };
    let animated = match kind {
        ChatBlock::Gif => chat.gif_frame,
        _ => 0,
    };
    rune::memo(
        format!("block{index}"),
        (shown, animated),
        move || match kind {
            ChatBlock::User(text) => user(text),
            ChatBlock::Heading(text) => typed(turn, text, CHAT_HEADING_TEXT, true, shown),
            ChatBlock::Paragraph(text) => typed(turn, text, CHAT_TEXT, false, shown),
            ChatBlock::Code(code) => code_block(turn, code, shown),
            ChatBlock::Table(rows) => table(rows, shown),
            ChatBlock::Gif => gif(chat, animated),
            ChatBlock::Buttons(labels) => buttons(labels),
        },
    )
}

fn user(message: &str) -> El {
    row().w_full().justify_end().child(
        el().px(10.0)
            .py(6.0)
            .radius(10.0)
            .fill(rgb(CHAT_USER_RGB))
            .child(
                text(message)
                    .font(roboto(CHAT_TEXT, false))
                    .color(Color::WHITE),
            ),
    )
}

/// A paragraph or a heading: the turn's number, then as much of `body` as
/// `shown` tokens show.
fn typed(turn: usize, body: &str, size: f32, bold: bool, shown: usize) -> El {
    text(format!(
        "{} {}",
        chat_turn_label(turn),
        &body[..chat_shown_chars(body, shown)]
    ))
    .w_full()
    .font(roboto(size, bold))
    .color(Color::WHITE)
    .wrap()
}

fn code_block(turn: usize, code: &str, shown: usize) -> El {
    let head = format!("// {}\n", chat_turn_label(turn));
    let chars = chat_shown_chars(code, shown);
    let colored = |range: Range<usize>, kind: CodeKind| {
        Span::new(range.start, range.end).color(rgb(kind.rgb()))
    };
    let mut spans = vec![colored(0..head.len() - 1, CodeKind::Comment)];
    spans.extend(
        chat_code_spans(code)
            .into_iter()
            .filter_map(|(range, kind)| {
                (range.start < chars).then(|| {
                    colored(
                        head.len() + range.start..head.len() + range.end.min(chars),
                        kind,
                    )
                })
            }),
    );
    el().w_full()
        .padding(8.0)
        .radius(6.0)
        .fill(rgb(CHAT_PANEL_RGB))
        .child(
            text(format!("{head}{}", &code[..chars]))
                .spans(spans)
                .font(roboto(CHAT_CODE_TEXT, false).mono())
                .color(Color::WHITE),
        )
}

/// A table whose first `shown` rows appeared.
fn table(rows: &[[&str; CHAT_TABLE_COLUMNS]], shown: usize) -> El {
    col()
        .w_full()
        .children(rows.iter().take(shown).enumerate().map(|(index, cells)| {
            let header = index == 0;
            let line = row().w_full().px(6.0).py(3.0);
            let line = if header || index % 2 == 1 {
                line.fill(rgb(CHAT_PANEL_RGB))
            } else {
                line
            };
            line.children(cells.iter().map(|cell| {
                text(*cell)
                    .grow(1.0)
                    .min_w(0.0)
                    .font(roboto(CHAT_CODE_TEXT, header))
                    .color(Color::WHITE)
            }))
        }))
}

/// The animated picture at its frame `frame`.
fn gif(chat: &Chat, frame: usize) -> El {
    let picture = el().size(CHAT_GIF_SIZE, CHAT_GIF_SIZE);
    match chat.gif.get(frame) {
        Some(image) => picture.pixels(image.pixels()),
        None => picture,
    }
}

fn buttons(labels: &[&str]) -> El {
    row().gap(8.0).children(labels.iter().map(|label| {
        el().px(10.0)
            .py(6.0)
            .radius(6.0)
            .fill(rgb(CHAT_USER_RGB))
            .child(
                text(*label)
                    .font(roboto(CHAT_TEXT, true))
                    .color(Color::WHITE),
            )
    }))
}
