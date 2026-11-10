//! The chat beside the patchwork (`../../README.md`): an agent's answers
//! arrive from a data thread as tokens and the list shows them as they come.
//!
//! In BUI's idiom a block is an element under a key of its own, so the
//! reconciler keeps the layout of every block a token did not touch. A
//! paragraph hands the layout slices of the turn's static text, never a
//! copy, so a token costs the live block's text and nothing for the others.
//! The list is a clipped column that aligns its content to the bottom, with a
//! minimum height of the list: it starts at the top while short and shows the
//! end once full, with no offset to set. It holds the blocks of the last two
//! turns. The spinner is an image turned by a transform, which is drawn
//! again and never laid out again. BUI has no gradient for text
//! (`background-clip: text` keeps the box color), so each character of the
//! status line has its own color.

use std::{ops::Range, sync::Arc};

use blenderui::prelude::*;
use perf_data::*;

use crate::{BOLD, REGULAR, color, text};

const MONO_FAMILY: &str = "DejaVu Sans Mono";

/// The chat's state: the last token, the decoded pictures and where the
/// status line's characters stood in the last layout.
pub(crate) struct Chat {
    feed: Option<ChatFeed>,
    token: Option<ChatToken>,
    gif: Vec<Arc<Image>>,
    spinner: Option<Arc<Image>>,
    /// The colored stretches of each code block of the turn.
    code: Vec<Vec<(Range<usize>, CodeKind)>>,
    /// The left edge of each character of the status line, from the text's
    /// start, as the last layout placed them.
    status_x: Vec<f32>,
}

impl Chat {
    pub(crate) fn new() -> Self {
        let feed = ChatFeed::start()
            .map_err(|error| log::warn!("chat data thread: {error}"))
            .ok();
        let mut gif = Vec::new();
        match blenderui::Animation::new(CHAT_GIF, 0, 0) {
            Some(mut animation) => {
                for _ in 0..animation.frames {
                    if let Some((frame, _)) = animation.next_frame() {
                        gif.push(Arc::new(frame));
                    }
                }
            }
            None => log::warn!("chat picture does not decode"),
        }
        let spinner = Image::decode(CHAT_SVG.as_bytes(), 64, 64).map(Arc::new);
        if spinner.is_none() {
            log::warn!("chat spinner does not decode");
        }
        Self {
            feed,
            token: None,
            gif,
            spinner,
            code: CHAT_TURN
                .iter()
                .map(|block| match block {
                    ChatBlock::Code(code) => chat_code_spans(code),
                    _ => Vec::new(),
                })
                .collect(),
            status_x: Vec::new(),
        }
    }

    /// Takes the frame's tokens without waiting.
    pub(crate) fn take_tokens(&mut self) {
        let Some(feed) = &self.feed else { return };
        for _ in 0..CHAT_TOKENS_PER_FRAME {
            match feed.take() {
                Some(token) => self.token = Some(token),
                None => break,
            }
        }
    }

    /// Reads where the status line's characters stand in the last layout.
    pub(crate) fn read_status(&mut self, window: &Window) {
        self.status_x.clear();
        let left = window
            .bounds_of("status-0")
            .map_or(0.0, |bounds| bounds.origin.x);
        let mut index = 0;
        while let Some(bounds) = window.bounds_of(&format!("status-{index}")) {
            self.status_x.push(bounds.origin.x - left);
            index += 1;
        }
    }

    pub(crate) fn view(&self, frame: u32) -> Div {
        let blocks = self.token.map(|token| {
            let first = ChatToken::turn(token.block).saturating_sub(1) * CHAT_TURN.len();
            (first..=token.block).map(move |index| self.block(index, token, frame))
        });
        div()
            .flex_none()
            .w(px(CHAT_WIDTH))
            .h_full()
            .flex()
            .flex_col()
            .bg(color(CHAT_BACKGROUND_RGB))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .child(
                        div()
                            .flex_none()
                            .min_h_full()
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .p(px(8.0))
                            .children(blocks.into_iter().flatten()),
                    ),
            )
            .child(self.status(frame))
    }

    /// Block `index`; `token` says how much of the last block shows.
    fn block(&self, index: usize, token: ChatToken, frame: u32) -> Div {
        let turn = ChatToken::turn(index);
        let kind = CHAT_TURN[index % CHAT_TURN.len()];
        let shown = (index == token.block).then_some(token.shown);
        let root = div().key(format!("block-{index}"));
        match kind {
            ChatBlock::User(said) => root.flex().justify_end().child(
                div()
                    .max_w_full()
                    .px(px(10.0))
                    .py(px(6.0))
                    .rounded(px(10.0))
                    .bg(color(CHAT_USER_RGB))
                    .child(wrapped(said, CHAT_TEXT, REGULAR)),
            ),
            ChatBlock::Heading(said) => {
                root.child(typed(turn, said, CHAT_HEADING_TEXT, BOLD, shown))
            }
            ChatBlock::Paragraph(said) => root.child(typed(turn, said, CHAT_TEXT, REGULAR, shown)),
            ChatBlock::Code(code) => root.child(self.code_block(index, turn, code, shown)),
            ChatBlock::Table(rows) => root.child(table(rows, shown)),
            ChatBlock::Gif => root.child(self.picture(frame)),
            ChatBlock::Buttons(labels) => {
                root.flex()
                    .gap(px(8.0))
                    .children(labels.iter().map(|label| {
                        div()
                            .px(px(10.0))
                            .py(px(6.0))
                            .rounded(px(6.0))
                            .bg(color(CHAT_USER_RGB))
                            .child(text(*label, CHAT_TEXT, BOLD, white()))
                    }))
            }
        }
    }

    /// The comment that names the turn, then as much of `code` as arrived,
    /// each stretch in its color.
    fn code_block(
        &self,
        index: usize,
        turn: usize,
        code: &'static str,
        shown: Option<usize>,
    ) -> Div {
        let end = shown.map_or(code.len(), |tokens| chat_shown_chars(code, tokens));
        let mut body = div()
            .font_family(MONO_FAMILY)
            .text_size(px(CHAT_CODE_TEXT))
            .line_height(px(CHAT_CODE_TEXT * 1.4))
            .text_color(white())
            .css("white-space", "pre")
            .child(
                span()
                    .text_color(color(CodeKind::Comment.rgb()))
                    .child(format!("// {}\n", chat_turn_label(turn))),
            );
        let mut at = 0;
        for (range, kind) in &self.code[index % CHAT_TURN.len()] {
            if range.start >= end {
                break;
            }
            if range.start > at {
                body = body.child(&code[at..range.start]);
            }
            at = range.end.min(end);
            body = body.child(
                span()
                    .text_color(color(kind.rgb()))
                    .child(&code[range.start..at]),
            );
        }
        if at < end {
            body = body.child(&code[at..end]);
        }
        div()
            .w_full()
            .p(px(8.0))
            .rounded(px(6.0))
            .bg(color(CHAT_PANEL_RGB))
            .child(body)
    }

    /// The animated picture at the frame's place in its loop.
    fn picture(&self, frame: u32) -> Div {
        let picture = img().flex_none().w(px(CHAT_GIF_SIZE)).h(px(CHAT_GIF_SIZE));
        match self.gif.get(chat_gif_frame(frame, self.gif.len())) {
            Some(image) => picture.image(image.clone()),
            None => picture,
        }
    }

    /// The spinner turned to the frame's angle and what the agent does, each
    /// character in the gradient's color at its place.
    fn status(&self, frame: u32) -> Div {
        let [dim, bright] = CHAT_SHIMMER_RGB;
        let channel = |index: usize, weight: f32| {
            f32::from(dim[index]) + (f32::from(bright[index]) - f32::from(dim[index])) * weight
        };
        let said = self.token.map(chat_status).unwrap_or_default();
        let spinner = match &self.spinner {
            Some(image) => img().image(image.clone()),
            None => img(),
        }
        .flex_none()
        .w(px(CHAT_SPINNER_SIZE))
        .h(px(CHAT_SPINNER_SIZE))
        .css(
            "transform",
            format!("rotate({}deg)", frame as f32 * CHAT_SPIN_DEGREES % 360.0),
        );
        div()
            .flex_none()
            .w_full()
            .h(px(CHAT_STATUS_HEIGHT))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .bg(color(CHAT_PANEL_RGB))
            .child(spinner)
            .child(
                div()
                    .flex()
                    .css("white-space", "pre")
                    .text_size(px(CHAT_TEXT))
                    .line_height(px(CHAT_TEXT * 1.4))
                    .children(said.chars().enumerate().map(|(index, character)| {
                        let weight = chat_shimmer(
                            self.status_x.get(index).copied().unwrap_or_default(),
                            frame,
                        );
                        div()
                            .key(format!("status-{index}"))
                            .text_color(rgb((channel(0, weight) as u32) << 16
                                | (channel(1, weight) as u32) << 8
                                | channel(2, weight) as u32))
                            .child(character.to_string())
                    })),
            )
    }
}

/// Wrapped text in white.
fn wrapped(content: impl Into<String>, size: f32, weight: FontWeight) -> Div {
    div()
        .text_size(px(size))
        .line_height(px(size * 1.4))
        .font_weight(weight)
        .text_color(white())
        .child(content.into())
}

/// The turn's number, then as much of `said` as arrived.
fn typed(
    turn: usize,
    said: &'static str,
    size: f32,
    weight: FontWeight,
    shown: Option<usize>,
) -> Div {
    let end = shown.map_or(said.len(), |tokens| chat_shown_chars(said, tokens));
    div()
        .w_full()
        .text_size(px(size))
        .line_height(px(size * 1.4))
        .font_weight(weight)
        .text_color(white())
        .child(format!("{} ", chat_turn_label(turn)))
        .child(&said[..end])
}

/// The rows of the table that arrived, the header first.
fn table(rows: &'static [[&'static str; CHAT_TABLE_COLUMNS]], shown: Option<usize>) -> Div {
    let count = shown.map_or(rows.len(), |tokens| tokens.min(rows.len()));
    div()
        .w_full()
        .children(rows[..count].iter().enumerate().map(|(index, row)| {
            let panel = index == 0 || index % 2 == 1;
            div()
                .flex()
                .px(px(6.0))
                .py(px(3.0))
                .when(panel, |row| row.bg(color(CHAT_PANEL_RGB)))
                .children(row.iter().map(|cell| {
                    div().flex_1().min_w(px(0.0)).child(text(
                        *cell,
                        CHAT_CODE_TEXT,
                        if index == 0 { BOLD } else { REGULAR },
                        white(),
                    ))
                }))
        }))
}
