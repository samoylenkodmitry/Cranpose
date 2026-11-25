use std::rc::Rc;

use freya::{elements::image::ImageHandle, engine::prelude::AlphaType, prelude::*};
use perf_data::*;

use crate::gauntlet::{rgb, text};

#[derive(Clone)]
struct Pictures(Rc<Vec<ImageHandle>>);

fn pictures() -> Pictures {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(CHAT_GIF).expect("bundled chat GIF");
    let mut frames = Vec::new();
    while let Some(frame) = decoder.read_next_frame().expect("chat GIF frame") {
        frames.push(
            ImageHandle::from_rgba(
                frame.width.into(),
                frame.height.into(),
                frame.buffer.to_vec().into(),
                AlphaType::Unpremul,
            )
            .expect("chat image"),
        );
    }
    Pictures(Rc::new(frames))
}

pub(super) fn sidebar(token: Option<ChatToken>, frame: u32, scroll: ScrollController) -> Rect {
    use_provide_context(pictures);
    let count = token.map_or(0, |t| t.block + 1);
    let first = count.saturating_sub(2 * CHAT_TURN.len());
    let blocks = (first..count).map(|index| {
        let shown = token.filter(|t| t.block == index).map(|t| t.shown);
        let gif_frame = if matches!(CHAT_TURN[index % CHAT_TURN.len()], ChatBlock::Gif) {
            frame
        } else {
            0
        };
        Block {
            index,
            shown,
            frame: gif_frame,
        }
    });
    rect()
        .width(Size::px(CHAT_WIDTH))
        .height(Size::fill())
        .content(Content::Flex)
        .background(rgb(CHAT_BACKGROUND_RGB))
        .child(
            rect()
                .width(Size::fill())
                .height(Size::flex(1.))
                .overflow(Overflow::Clip)
                .child(
                    ScrollView::new_controlled(scroll)
                        .show_scrollbar(false)
                        .child(
                            rect()
                                .width(Size::fill())
                                .padding(Gaps::new_all(8.))
                                .spacing(8.)
                                .children(blocks),
                        ),
                ),
        )
        .child(status(token, frame))
}

fn bubble(title: &'static str, bold: bool, radius: f32) -> Rect {
    rect()
        .corner_radius(radius)
        .background(rgb(CHAT_USER_RGB))
        .padding(Gaps::new(6., 10., 6., 10.))
        .child(text(title, CHAT_TEXT, Color::WHITE, bold))
}

#[derive(PartialEq)]
struct Block {
    index: usize,
    shown: Option<usize>,
    frame: u32,
}

impl Component for Block {
    fn render_key(&self) -> DiffKey {
        DiffKey::from(&self.index)
    }
    fn render(&self) -> impl IntoElement {
        let turn = chat_turn_label(ChatToken::turn(self.index));
        let block = CHAT_TURN[self.index % CHAT_TURN.len()];
        let prefix = |s: &'static str| &s[..self.shown.map_or(s.len(), |n| chat_shown_chars(s, n))];
        match block {
            ChatBlock::User(title) => rect()
                .width(Size::fill())
                .cross_align(Alignment::End)
                .child(bubble(title, false, 10.)),
            ChatBlock::Heading(s) | ChatBlock::Paragraph(s) => {
                let bold = matches!(block, ChatBlock::Heading(_));
                rect().width(Size::fill()).child(text(
                    format!("{} {}", turn, prefix(s)),
                    if bold { CHAT_HEADING_TEXT } else { CHAT_TEXT },
                    Color::WHITE,
                    bold,
                ))
            }
            ChatBlock::Code(code) => {
                let mut rich = paragraph()
                    .font_size(CHAT_CODE_TEXT)
                    .font_family("DejaVu Sans Mono")
                    .color(Color::WHITE)
                    .span(Span::new(format!("// {turn}\n")).color(rgb(CodeKind::Comment.rgb())));
                let end = prefix(code).len();
                let mut at = 0;
                for (span, kind) in chat_code_spans(code) {
                    if span.start >= end {
                        break;
                    }
                    rich = rich.span(&code[at..span.start]).span(
                        Span::new(&code[span.start..span.end.min(end)]).color(rgb(kind.rgb())),
                    );
                    at = span.end.min(end);
                }
                rect()
                    .width(Size::fill())
                    .corner_radius(6.)
                    .background(rgb(CHAT_PANEL_RGB))
                    .padding(Gaps::new_all(8.))
                    .child(rich.span(&code[at..end]))
            }
            ChatBlock::Table(rows) => rect().width(Size::fill()).children(
                rows.iter()
                    .take(self.shown.unwrap_or(rows.len()))
                    .enumerate()
                    .map(|(i, row)| {
                        let mut line = rect()
                            .width(Size::fill())
                            .horizontal()
                            .content(Content::Flex)
                            .padding(Gaps::new(3., 6., 3., 6.));
                        if i == 0 || i % 2 == 1 {
                            line = line.background(rgb(CHAT_PANEL_RGB));
                        }
                        line.children(row.iter().map(|s| {
                            rect().width(Size::flex(1.)).child(text(
                                *s,
                                CHAT_CODE_TEXT,
                                Color::WHITE,
                                i == 0,
                            ))
                        }))
                    }),
            ),
            ChatBlock::Gif => {
                let frames = use_consume::<Pictures>();
                rect().child(
                    image(frames.0[chat_gif_frame(self.frame, frames.0.len())].clone())
                        .width(Size::px(CHAT_GIF_SIZE))
                        .height(Size::px(CHAT_GIF_SIZE)),
                )
            }
            ChatBlock::Buttons(labels) => rect()
                .horizontal()
                .spacing(8.)
                .children(labels.iter().map(|s| bubble(s, true, 6.).on_press(|_| {}))),
        }
    }
}

fn status(token: Option<ChatToken>, frame: u32) -> Rect {
    let width = CHAT_WIDTH - 16. - CHAT_SPINNER_SIZE - 6.;
    let color = |x| {
        let [a, b] = CHAT_SHIMMER_RGB;
        rgb(std::array::from_fn(|i| {
            (a[i] as f32 + (b[i] as f32 - a[i] as f32) * chat_shimmer(x, frame)) as u8
        }))
    };
    let mut gradient = LinearGradient::new().angle(270.).stop((color(0.), 0.));
    let half = CHAT_SHIMMER_SPAN / 2.;
    let mut x = (frame as f32 * CHAT_SHIMMER_STEP) % half;
    while x < width {
        if x > 0. {
            gradient = gradient.stop((color(x), 100. * x / width));
        }
        x += half;
    }
    rect()
        .width(Size::fill())
        .height(Size::px(CHAT_STATUS_HEIGHT))
        .horizontal()
        .cross_align(Alignment::Center)
        .background(rgb(CHAT_PANEL_RGB))
        .spacing(6.)
        .padding(Gaps::new(0., 8., 0., 8.))
        .child(
            rect().rotation(frame as f32 * CHAT_SPIN_DEGREES).child(
                SvgViewer::new(CHAT_SVG.as_bytes())
                    .width(Size::px(CHAT_SPINNER_SIZE))
                    .height(Size::px(CHAT_SPINNER_SIZE)),
            ),
        )
        .maybe_child(token.map(|t| {
            label()
                .width(Size::px(width))
                .text(chat_status(t))
                .font_size(CHAT_TEXT)
                .color(gradient.stop((color(width), 100.)))
        }))
}
