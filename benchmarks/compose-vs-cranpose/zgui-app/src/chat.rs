//! The chat beside the patchwork in zgui (`../README.md`): an agent's answers
//! arrive from a data thread as tokens and the list shows them as they come.
//!
//! In zgui's idiom a block is built once, and its text reads the signal of how
//! much of the last block shows. A block that is no longer the last one stops
//! reading it. A code block is a text of colored runs, each run a text node
//! that reads a memo of its own length, so a token writes the one run that
//! grows. The table is a keyed list of rows, so a token mounts one row. The
//! animated picture reads the frame for its source, and the spinner and the
//! gradient over the status text read it for a transform and a background.
//! The list is not virtual: it holds the blocks of the last two turns, and
//! the frame loop scrolls its container to the end.

use std::{ops::Range, rc::Rc};

use perf_data::*;
use zgui::{
    elements::{self as e},
    geom::{DevicePx, Point},
    prelude::*,
    view::{AnyView, ForProps},
};
use zgui_image::ImageBytes;

use crate::{picture, sheet::hex};

/// The turns whose blocks the list holds.
const TURNS_HELD: usize = 2;

/// The chat's state: the data thread and what the frame loop writes from the
/// tokens it sends.
pub(crate) struct Chat {
    feed: Option<ChatFeed>,
    /// The blocks the chat holds so far, how much of the last one shows and
    /// the last token.
    blocks: RwSignal<usize>,
    shown: RwSignal<usize>,
    token: RwSignal<ChatToken>,
    list: NodeRef,
}

impl Chat {
    pub(crate) fn new() -> Self {
        Self {
            feed: ChatFeed::start()
                .map_err(|error| log::warn!("chat data thread: {error}"))
                .ok(),
            blocks: RwSignal::new(0),
            shown: RwSignal::new(0),
            token: RwSignal::new(ChatToken::default()),
            list: NodeRef::new(),
        }
    }

    /// Takes the frame's tokens and keeps the list at its end.
    pub(crate) fn advance(&self) {
        let Some(feed) = &self.feed else {
            return;
        };
        let mut last = None;
        for _ in 0..CHAT_TOKENS_PER_FRAME {
            match feed.take() {
                Some(token) => last = Some(token),
                None => break,
            }
        }
        if let Some(token) = last {
            if self.blocks.get_untracked() != token.block + 1 {
                self.blocks.set(token.block + 1);
            }
            self.shown.set(token.shown);
            self.token.set(token);
            self.list.scroll_to(
                ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(f32::MAX))),
                ScrollBehavior::Instant,
            );
        }
    }

    /// The chat: the list of blocks over the status line.
    pub(crate) fn view(&self, frame: RwSignal<u32>) -> impl IntoView + use<> {
        let pictures = Rc::new(gif_pictures());
        let (blocks, shown) = (self.blocks, self.shown);
        let list = ForProps::builder()
            .each(move || {
                let end = blocks.get();
                (end.saturating_sub(TURNS_HELD * CHAT_TURN.len())..end).collect::<Vec<_>>()
            })
            .key(|block: &usize| *block)
            .children(move |index| block(index, blocks, shown, frame, &pictures))
            .build()
            .render();
        e::column()
            .class("chat")
            .child(
                e::scroll()
                    .class("chat-list")
                    .node_ref(self.list)
                    .child(e::column().class("chat-blocks").child(list)),
            )
            .child(status(frame, self.token))
    }
}

/// The status line: a spinner that turns each frame and what the agent does,
/// under a gradient that moves each frame.
fn status(frame: RwSignal<u32>, token: RwSignal<ChatToken>) -> impl IntoView {
    let [dim, bright] = CHAT_SHIMMER_RGB;
    e::row()
        .class("status")
        .child(
            e::vector()
                .class("spinner")
                .document(CHAT_SVG)
                .style_property("transform", move || {
                    Some(format!(
                        "rotate({}deg)",
                        frame.get() as f32 * CHAT_SPIN_DEGREES
                    ))
                }),
        )
        .child(
            e::text()
                .class("status-text")
                .style_property("background-image", move || {
                    let start = (frame.get() as f32 * CHAT_SHIMMER_STEP) % CHAT_SHIMMER_SPAN;
                    let half = CHAT_SHIMMER_SPAN / 2.0;
                    Some(format!(
                        "repeating-linear-gradient(90deg, {} {start}px, {} {}px, {} {}px)",
                        hex(dim),
                        hex(bright),
                        start + half,
                        hex(dim),
                        start + CHAT_SHIMMER_SPAN,
                    ))
                })
                .child(move || chat_status(token.get())),
        )
}

/// Block `index` of the chat. It is live while it is the last block, the one
/// whose text reads how much of it shows.
fn block(
    index: usize,
    blocks: RwSignal<usize>,
    shown: RwSignal<usize>,
    frame: RwSignal<u32>,
    pictures: &Rc<Vec<ImageBytes>>,
) -> AnyView {
    let live = Memo::new(move |_| blocks.get() == index + 1);
    let turn = ChatToken::turn(index);
    match CHAT_TURN[index % CHAT_TURN.len()] {
        ChatBlock::User(text) => AnyView::new(e::text().class("bubble").child(text)),
        ChatBlock::Heading(text) => AnyView::new(typed(turn, text, "chat-heading", live, shown)),
        ChatBlock::Paragraph(text) => AnyView::new(typed(turn, text, "chat-text", live, shown)),
        ChatBlock::Code(code) => AnyView::new(code_block(turn, code, live, shown)),
        ChatBlock::Table(rows) => AnyView::new(table(rows, live, shown)),
        ChatBlock::Gif => {
            let pictures = Rc::clone(pictures);
            // The source changes every `CHAT_GIF_HOLD` frames, not every frame.
            let shown = Memo::new({
                let frames = pictures.len();
                move |_| chat_gif_frame(frame.get(), frames)
            });
            AnyView::new(
                e::image()
                    .class("gif")
                    .src(move || pictures.get(shown.get()).map(|picture| picture.url())),
            )
        }
        ChatBlock::Buttons(labels) => AnyView::new(
            e::row().class("buttons").children(
                labels
                    .iter()
                    .map(|label| AnyView::new(e::control().class("button").child(*label))),
            ),
        ),
    }
}

/// The characters of `text` a block shows: all of them once it is no longer
/// the last block.
fn visible(text: &str, live: Memo<bool>, shown: RwSignal<usize>) -> usize {
    if live.get() {
        chat_shown_chars(text, shown.get())
    } else {
        text.len()
    }
}

/// A paragraph or a heading: the turn's number, then as much of `text` as
/// arrived.
fn typed(
    turn: usize,
    text: &'static str,
    class: &'static str,
    live: Memo<bool>,
    shown: RwSignal<usize>,
) -> impl IntoView {
    e::text().class(class).child(move || {
        format!(
            "{} {}",
            chat_turn_label(turn),
            &text[..visible(text, live, shown)]
        )
    })
}

/// The runs of the code block's text, in order: the comment that names the
/// turn, then the code cut at its colored stretches.
fn code_runs(turn: usize, code: &str) -> Vec<(String, Option<[u8; 3]>)> {
    let head = format!("// {}\n", chat_turn_label(turn));
    let mut runs = vec![(head, Some(CodeKind::Comment.rgb()))];
    let mut at = 0;
    for (span, kind) in chat_code_spans(code) {
        if span.start > at {
            runs.push((code[at..span.start].to_owned(), None));
        }
        runs.push((code[span.clone()].to_owned(), Some(kind.rgb())));
        at = span.end;
    }
    if at < code.len() {
        runs.push((code[at..].to_owned(), None));
    }
    runs
}

fn code_block(
    turn: usize,
    code: &'static str,
    live: Memo<bool>,
    shown: RwSignal<usize>,
) -> impl IntoView {
    let runs = code_runs(turn, code);
    let head = runs[0].0.len();
    // How much of the text, comment included, shows.
    let cut = Memo::new(move |_| head + visible(code, live, shown));
    let mut start = 0;
    let texts = runs.into_iter().map(|(text, color)| {
        let range: Range<usize> = start..start + text.len();
        start = range.end;
        // The part of this run the cut leaves: a memo that changes only for
        // the run the cut is inside.
        let length = Memo::new(move |_| cut.get().saturating_sub(range.start).min(range.len()));
        let run = e::text().child(move || text[..length.get()].to_owned());
        AnyView::new(match color {
            Some(rgb) => run.style_property("color", Some(hex(rgb))),
            None => run,
        })
    });
    e::column()
        .class("code")
        .child(e::text().class("code-text").children(texts))
}

fn table(
    rows: &'static [[&'static str; CHAT_TABLE_COLUMNS]],
    live: Memo<bool>,
    shown: RwSignal<usize>,
) -> impl IntoView {
    let count = Memo::new(move |_| {
        if live.get() {
            shown.get().min(rows.len())
        } else {
            rows.len()
        }
    });
    e::column().class("table").child(
        ForProps::builder()
            .each(move || (0..count.get()).collect::<Vec<_>>())
            .key(|row: &usize| *row)
            .children(move |row| {
                let header = row == 0;
                e::row()
                    .class(if header || row % 2 == 1 {
                        "table-row table-shaded"
                    } else {
                        "table-row"
                    })
                    .children(rows[row].iter().map(move |cell| {
                        let cell = e::text().class("table-cell").child(*cell);
                        AnyView::new(if header { cell.class("bold") } else { cell })
                    }))
            })
            .build()
            .render(),
    )
}

/// Every frame of the animated picture as a picture of its own; none when it
/// does not decode. The `image` element shows the first frame of a GIF.
fn gif_pictures() -> Vec<ImageBytes> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut pictures = Vec::new();
    match options.read_info(CHAT_GIF) {
        Ok(mut decoder) => {
            let (width, height) = (u32::from(decoder.width()), u32::from(decoder.height()));
            while let Ok(Some(frame)) = decoder.read_next_frame() {
                pictures.push(picture::from_rgba(width, height, &frame.buffer));
            }
        }
        Err(error) => log::warn!("chat picture: {error}"),
    }
    pictures
}
