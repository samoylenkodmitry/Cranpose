//! The chat beside the patchwork in GPUI (`../../README.md`): an agent's
//! answers arrive from a data thread as tokens and the screen shows them as
//! they come.
//!
//! The list of blocks is GPUI's own `list` over a `ListState` that follows
//! its tail: it starts at the top while the blocks are short and keeps the
//! end in view once they fill it. A token splices in a block only when one
//! begins; the list renders the blocks on screen and measures them again,
//! which GPUI cannot skip for a list that scrolls, since a cached view needs
//! a definite size and a moved view is drawn again. The text of a block is
//! shaped once: GPUI keeps the layout of a line it drew in the frame before.
//! The status line is a view of its own, cached at its size, that the frame
//! notifies. Its spinner is the progress SVG, one layer for each of its
//! paths, turned by the layers' transformation, which GPUI applies without
//! rasterizing again. GPUI has no gradient for text: each glyph takes the
//! color of its place.

use std::{borrow::Cow, io::Cursor, ops::Range, sync::Arc};

use gpui::{
    AnyElement, AssetSource, Context, Div, FollowMode, FontWeight, HighlightStyle, Hsla,
    ImageSource, IntoElement, ListAlignment, ListState, ParentElement, Render, RenderImage,
    SharedString, Styled, StyledText, TextRun, Transformation, Window, div, img, list,
    prelude::FluentBuilder, px, radians, svg,
};
use perf_data::*;

use super::rgb;

/// The family of the monospaced face the launcher loads after Roboto.
pub(super) const MONO_FAMILY: &str = "DejaVu Sans Mono";
/// The space around the list and between its blocks, in points.
const GAP: f32 = 8.0;
/// The blocks the list holds: those of the last two turns.
const HELD: usize = 2 * CHAT_TURN.len();

/// The tokens of the chat's data thread.
pub(super) struct Chat {
    feed: Option<ChatFeed>,
}

impl Chat {
    pub(super) fn start() -> Self {
        Self {
            feed: ChatFeed::start()
                .map_err(|error| log::warn!("chat data thread: {error}"))
                .ok(),
        }
    }

    /// The frame's last token, taking up to `CHAT_TOKENS_PER_FRAME` without
    /// waiting.
    pub(super) fn take(&self) -> Option<ChatToken> {
        let feed = self.feed.as_ref()?;
        (0..CHAT_TOKENS_PER_FRAME).map_while(|_| feed.take()).last()
    }
}

/// Every frame of the animated picture as an image of its own: the chat
/// shows the one the frame index names. None when it does not decode.
pub(super) fn gif_frames() -> Arc<Vec<Arc<RenderImage>>> {
    use image::AnimationDecoder;
    let frames = image::codecs::gif::GifDecoder::new(Cursor::new(CHAT_GIF))
        .and_then(|decoder| decoder.into_frames().collect_frames());
    Arc::new(match frames {
        Ok(frames) => frames
            .into_iter()
            .map(|frame| {
                // GPUI's frames hold BGRA pixels.
                let mut buffer = frame.into_buffer();
                for pixel in buffer.as_chunks_mut::<4>().0 {
                    pixel.swap(0, 2);
                }
                Arc::new(RenderImage::new([image::Frame::new(buffer)]))
            })
            .collect(),
        Err(error) => {
            log::warn!("chat picture: {error}");
            Vec::new()
        }
    })
}

// The paths of the spinner's SVG, one line each.
fn spinner_paths() -> impl Iterator<Item = &'static str> {
    CHAT_SVG.lines().filter(|line| line.contains("<path"))
}

fn spinner_path(layer: usize) -> SharedString {
    format!("chat-spinner/{layer}.svg").into()
}

/// The spinner's SVG with one path in each document, which GPUI draws as a
/// mask in one color: the layers' colors are the strokes of their paths.
pub(super) struct SpinnerLayers;

impl SpinnerLayers {
    /// The colors of the layers, the bottom one first.
    fn colors() -> Vec<Hsla> {
        spinner_paths()
            .map(|path| {
                let stroke = path.split("stroke=\"#").nth(1).unwrap_or_default();
                let value = u32::from_str_radix(stroke.get(..6).unwrap_or("FFFFFF"), 16)
                    .unwrap_or(0xFFFFFF);
                rgb([(value >> 16) as u8, (value >> 8) as u8, value as u8])
            })
            .collect()
    }
}

impl AssetSource for SpinnerLayers {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let header = CHAT_SVG.lines().next().unwrap_or_default();
        Ok(spinner_paths()
            .enumerate()
            .find(|(layer, _)| spinner_path(*layer) == path)
            .map(|(_, line)| Cow::Owned(format!("{header}\n{line}\n</svg>").into_bytes())))
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok((0..spinner_paths().count()).map(spinner_path).collect())
    }
}

/// The list of blocks: a view cached at its size that the frame notifies
/// when a token arrives or the animated picture moves on.
pub(super) struct ChatList {
    list: ListState,
    /// The first block the list holds and how many it holds.
    base: usize,
    held: usize,
    token: Option<ChatToken>,
    frame: u32,
    gif: Arc<Vec<Arc<RenderImage>>>,
    /// The colored stretches of each block of a turn that holds code.
    spans: Arc<Vec<Vec<(Range<usize>, CodeKind)>>>,
}

impl ChatList {
    pub(super) fn new(
        gif: Arc<Vec<Arc<RenderImage>>>,
        token: Option<ChatToken>,
        frame: u32,
    ) -> Self {
        let list = ListState::new(0, ListAlignment::Top, px(0.0));
        list.set_follow_mode(FollowMode::Tail);
        let spans = CHAT_TURN
            .iter()
            .map(|block| match block {
                ChatBlock::Code(code) => chat_code_spans(code),
                _ => Vec::new(),
            })
            .collect();
        let mut chat = Self {
            list,
            base: 0,
            held: 0,
            token,
            frame,
            gif,
            spans: Arc::new(spans),
        };
        if let Some(token) = token {
            chat.hold(token);
        }
        chat
    }

    /// Takes the frame's token, if one arrived, and the frame's index.
    pub(super) fn show(&mut self, token: Option<ChatToken>, frame: u32, cx: &mut Context<Self>) {
        let moved = chat_gif_frame(frame, self.gif.len())
            != chat_gif_frame(frame.wrapping_sub(1), self.gif.len());
        self.frame = frame;
        if let Some(token) = token {
            self.hold(token);
            self.token = Some(token);
        }
        if token.is_some() || moved {
            cx.notify();
        }
    }

    // Keeps the list's items to the blocks up to `token`'s, the last two
    // turns of them.
    fn hold(&mut self, token: ChatToken) {
        let total = token.block + 1;
        let base = total.saturating_sub(HELD);
        let dropped = base.saturating_sub(self.base).min(self.held);
        self.list.splice(0..dropped, 0);
        let kept = self.held - dropped;
        self.held = total - base;
        self.list.splice(kept..kept, self.held - kept);
        self.base = base;
    }
}

impl Render for ChatList {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (base, frame) = (self.base, self.frame);
        let (gif, spans) = (self.gif.clone(), self.spans.clone());
        div()
            .size_full()
            .bg(rgb(CHAT_BACKGROUND_RGB))
            .when_some(self.token, |chat, token| {
                chat.child(
                    list(self.list.clone(), move |ix, _, _| {
                        block(base + ix, token, frame, &gif, &spans)
                    })
                    .size_full()
                    .pb(px(GAP)),
                )
            })
    }
}

fn panel() -> Hsla {
    rgb(CHAT_PANEL_RGB)
}

fn bubble() -> Hsla {
    rgb(CHAT_USER_RGB)
}

// Block `index` of the chat, whole or as far as the last token shows it.
fn block(
    index: usize,
    token: ChatToken,
    frame: u32,
    gif: &[Arc<RenderImage>],
    spans: &[Vec<(Range<usize>, CodeKind)>],
) -> AnyElement {
    let turn = ChatToken::turn(index);
    let shown = (index == token.block).then_some(token.shown);
    let place = index % CHAT_TURN.len();
    let content = match CHAT_TURN[place] {
        ChatBlock::User(text) => div().flex().justify_end().child(
            div()
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(10.0))
                .bg(bubble())
                .text_size(px(CHAT_TEXT))
                .child(text),
        ),
        ChatBlock::Heading(text) => {
            typed(turn, text, shown, CHAT_HEADING_TEXT).font_weight(FontWeight::BOLD)
        }
        ChatBlock::Paragraph(text) => typed(turn, text, shown, CHAT_TEXT),
        ChatBlock::Code(code) => {
            let chars = shown.map_or(code.len(), |shown| chat_shown_chars(code, shown));
            code_block(turn, &code[..chars], &spans[place])
        }
        ChatBlock::Table(rows) => {
            table(&rows[..shown.map_or(rows.len(), |shown| shown.min(rows.len()))])
        }
        ChatBlock::Gif => div().when_some(
            gif.get(chat_gif_frame(frame, gif.len())),
            |slot, picture| {
                slot.child(
                    img(ImageSource::Render(picture.clone()))
                        .flex_none()
                        .size(px(CHAT_GIF_SIZE)),
                )
            },
        ),
        ChatBlock::Buttons(labels) => {
            div()
                .flex()
                .gap(px(GAP))
                .children(labels.iter().map(|label| {
                    div()
                        .px(px(10.0))
                        .py(px(6.0))
                        .rounded(px(6.0))
                        .bg(bubble())
                        .text_size(px(CHAT_TEXT))
                        .font_weight(FontWeight::BOLD)
                        .child(*label)
                }))
        }
    };
    div()
        .px(px(GAP))
        .pt(px(GAP))
        .child(content)
        .into_any_element()
}

// A paragraph or a heading: the turn's number, then as much of `text` as
// arrived.
fn typed(turn: usize, text: &'static str, shown: Option<usize>, size: f32) -> Div {
    let chars = shown.map_or(text.len(), |shown| chat_shown_chars(text, shown));
    div().w_full().text_size(px(size)).child(format!(
        "{} {}",
        chat_turn_label(turn),
        &text[..chars]
    ))
}

// `code` under a comment that names its turn, each stretch in its color.
fn code_block(turn: usize, code: &str, spans: &[(Range<usize>, CodeKind)]) -> Div {
    let head = format!("// {}\n", chat_turn_label(turn));
    let color = |kind: CodeKind| HighlightStyle {
        color: Some(rgb(kind.rgb())),
        ..Default::default()
    };
    let colored = spans
        .iter()
        .take_while(|(span, _)| span.start < code.len())
        .map(|(span, kind)| {
            (
                head.len() + span.start..head.len() + span.end.min(code.len()),
                color(*kind),
            )
        });
    let highlights = std::iter::once((0..head.len(), color(CodeKind::Comment))).chain(colored);
    let text = StyledText::new(format!("{head}{code}")).with_highlights(highlights);
    div()
        .w_full()
        .p(px(GAP))
        .rounded(px(6.0))
        .bg(panel())
        .font_family(MONO_FAMILY)
        .text_size(px(CHAT_CODE_TEXT))
        .child(text)
}

// The table of `rows`, the first of which is its header.
fn table(rows: &[[&'static str; CHAT_TABLE_COLUMNS]]) -> Div {
    div()
        .w_full()
        .children(rows.iter().enumerate().map(|(index, row)| {
            div()
                .flex()
                .w_full()
                .px(px(6.0))
                .py(px(3.0))
                .text_size(px(CHAT_CODE_TEXT))
                .when(index == 0, |row| row.font_weight(FontWeight::BOLD))
                .when(index == 0 || index % 2 == 1, |row| row.bg(panel()))
                .children(row.iter().map(|cell| div().flex_1().child(*cell)))
        }))
}

/// The status line under the list: a view cached at its size that the frame
/// notifies, since its spinner and its text change every frame.
pub(super) struct ChatStatus {
    token: Option<ChatToken>,
    frame: u32,
    colors: Vec<Hsla>,
}

impl ChatStatus {
    pub(super) fn new(token: Option<ChatToken>, frame: u32) -> Self {
        Self {
            token,
            frame,
            colors: SpinnerLayers::colors(),
        }
    }

    /// Takes the frame's token, if one arrived, and the frame's index.
    pub(super) fn show(&mut self, token: Option<ChatToken>, frame: u32, cx: &mut Context<Self>) {
        self.frame = frame;
        self.token = token.or(self.token);
        cx.notify();
    }

    // The status text with each glyph in the gradient's color at its place.
    fn text(&self, token: ChatToken, window: &Window) -> StyledText {
        let text = chat_status(token);
        let run = TextRun {
            len: text.len(),
            font: gpui::font("Roboto"),
            color: gpui::white(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let places =
            window
                .text_system()
                .shape_line(text.clone().into(), px(CHAT_TEXT), &[run], None);
        let [dim, bright] = CHAT_SHIMMER_RGB;
        let highlights = text
            .char_indices()
            .map(|(start, glyph)| {
                let weight = chat_shimmer(f32::from(places.x_for_index(start)), self.frame);
                let channel = |index: usize| {
                    (f32::from(dim[index])
                        + (f32::from(bright[index]) - f32::from(dim[index])) * weight)
                        as u8
                };
                let color = rgb([channel(0), channel(1), channel(2)]);
                let style = HighlightStyle {
                    color: Some(color),
                    ..Default::default()
                };
                (start..start + glyph.len_utf8(), style)
            })
            .collect::<Vec<_>>();
        StyledText::new(text).with_highlights(highlights)
    }
}

impl Render for ChatStatus {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let degrees = self.frame as f32 * CHAT_SPIN_DEGREES;
        div()
            .size_full()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(GAP))
            .bg(panel())
            .text_size(px(CHAT_TEXT))
            .child(
                div()
                    .relative()
                    .flex_none()
                    .size(px(CHAT_SPINNER_SIZE))
                    .children(self.colors.iter().enumerate().map(|(layer, color)| {
                        svg()
                            .path(spinner_path(layer))
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .text_color(*color)
                            .with_transformation(Transformation::rotate(radians(
                                degrees.to_radians(),
                            )))
                    })),
            )
            .when_some(self.token, |line, token| {
                line.child(self.text(token, window))
            })
    }
}
