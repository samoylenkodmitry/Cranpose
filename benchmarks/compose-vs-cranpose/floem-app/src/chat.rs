//! The chat beside the patchwork (`../README.md`): an agent's answers arrive
//! from a data thread as tokens and the list shows them as they come.
//!
//! A token sets one signal. A keyed `dyn_stack` makes the views of the blocks
//! the token reaches and drops the oldest ones; the last block's text and rows
//! read a memo of how much of the block shows, so a token shapes that block
//! alone and a finished block reads nothing that changes. The scroll view
//! keeps its end in view after each layout. The status line shapes its text
//! again each frame in the colors of a gradient that Floem has no brush for in
//! text, so each glyph has its own.

use std::rc::Rc;

use floem::{
    IntoView,
    peniko::Color,
    prelude::*,
    reactive::{Memo, create_effect, create_memo},
    views::{dyn_stack, rich_text},
};
use perf_data::*;

use crate::{
    frame::Frame,
    picture::{bitmap, picture, vector},
    rgb,
    text::{Face, Type},
};

const INK: Color = rgb(WALL_INK_RGB);
const PANEL: Color = rgb(CHAT_PANEL_RGB);
const BUBBLE: Color = rgb(CHAT_USER_RGB);
// The turns whose blocks the list holds: more than it shows.
const TURNS_HELD: usize = 2;

const BODY: Type = Type::new(CHAT_TEXT, 1.4, Face::Regular, INK);
const HEADING: Type = Type::new(CHAT_HEADING_TEXT, 1.4, Face::Bold, INK);
const CODE: Type = Type::new(CHAT_CODE_TEXT, 1.4, Face::Mono, INK);

type Token = RwSignal<Option<ChatToken>>;

pub fn chat(frame: Frame, height: f32) -> impl IntoView {
    let token: Token = RwSignal::new(None);
    match ChatFeed::start() {
        // Each frame takes the tokens that arrived, up to the most it may.
        Ok(feed) => create_effect(move |_| {
            frame.get();
            let latest = (0..CHAT_TOKENS_PER_FRAME).map_while(|_| feed.take()).last();
            if latest.is_some() {
                token.set(latest);
            }
        }),
        Err(error) => log::warn!("chat data thread: {error}"),
    }
    let images = Rc::new(gif_frames());
    // The blocks the list holds change when a token starts a block.
    let held = create_memo(move |_| {
        token.get().map(|token| {
            let first = (token.block + 1).saturating_sub(TURNS_HELD * CHAT_TURN.len());
            (first, token.block)
        })
    });
    let list = scroll(
        dyn_stack(
            move || {
                held.get()
                    .into_iter()
                    .flat_map(|(first, last)| first..=last)
            },
            |block| *block,
            move |block| written(block, token, frame, images.clone()),
        )
        .style(|s| s.flex_col().gap(8.0).padding(8.0).width_full()),
    )
    .scroll_to_percent(move || {
        token.get();
        100.0
    })
    .scroll_style(|s| s.hide_bars(true))
    .style(|s| {
        s.width_full()
            .flex_grow(1.0_f32)
            .flex_basis(0.0)
            .min_height(0.0)
    });
    v_stack((list, status(frame, token))).style(move |s| {
        s.width(CHAT_WIDTH)
            .height(height)
            .flex_shrink(0.0_f32)
            .background(rgb(CHAT_BACKGROUND_RGB))
    })
}

// The style of a text of type `ty` that fills its line.
fn text_style(ty: Type) -> impl Fn(floem::style::Style) -> floem::style::Style + Copy {
    move |s| {
        s.width_full()
            .font_size(ty.size)
            .line_height(ty.line)
            .color(ty.color)
            .apply_if(ty.face == Face::Bold, |s| s.font_bold())
    }
}

// Block `block`, as far as the token shows it.
fn written(
    block: usize,
    token: Token,
    frame: Frame,
    images: Rc<Vec<floem::peniko::Image>>,
) -> impl IntoView {
    let turn = ChatToken::turn(block);
    let shown = create_memo(move |_| match token.get() {
        Some(token) if token.block == block => token.shown,
        _ => usize::MAX,
    });
    let typed = move |said: &'static str| {
        let chars = chat_shown_chars(said, shown.get().min(said.len()));
        format!("{} {}", chat_turn_label(turn), &said[..chars])
    };
    match CHAT_TURN[block % CHAT_TURN.len()] {
        ChatBlock::User(said) => user_block(said).into_any(),
        ChatBlock::Heading(said) => label(move || typed(said))
            .style(text_style(HEADING))
            .into_any(),
        ChatBlock::Paragraph(said) => label(move || typed(said))
            .style(text_style(BODY))
            .into_any(),
        ChatBlock::Code(code) => code_block(turn, code, shown).into_any(),
        ChatBlock::Table(rows) => table_block(rows, shown).into_any(),
        ChatBlock::Gif => {
            let count = images.len();
            picture(images, move || chat_gif_frame(frame.get(), count))
                .style(|s| s.size(CHAT_GIF_SIZE, CHAT_GIF_SIZE).flex_shrink(0.0_f32))
                .into_any()
        }
        ChatBlock::Buttons(labels) => buttons_block(labels).into_any(),
    }
}

fn user_block(said: &'static str) -> impl IntoView {
    container(container(text(said).style(text_style(BODY))).style(|s| {
        s.padding_vert(6.0)
            .padding_horiz(10.0)
            .background(BUBBLE)
            .border_radius(10.0)
    }))
    .style(|s| s.width_full().justify_end())
}

fn code_block(turn: usize, code: &'static str, shown: Memo<usize>) -> impl IntoView {
    container(
        rich_text(move || highlighted(turn, code, shown.get()))
            .style(|s| s.width_full().min_width(0.0)),
    )
    .style(|s| {
        s.width_full()
            .padding(8.0)
            .background(PANEL)
            .border_radius(6.0)
    })
}

// A table whose rows appear as the memo says.
fn table_block(
    rows: &'static [[&'static str; CHAT_TABLE_COLUMNS]],
    shown: Memo<usize>,
) -> impl IntoView {
    dyn_stack(
        move || 0..shown.get().min(rows.len()),
        |row| *row,
        move |row| {
            let header = row == 0;
            let shaded = header || row % 2 == 1;
            let cell_type = Type::new(
                CHAT_CODE_TEXT,
                1.4,
                if header { Face::Bold } else { Face::Regular },
                INK,
            );
            h_stack_from_iter(rows[row].iter().map(|cell| {
                text(*cell).style(move |s| {
                    text_style(cell_type)(s)
                        .width(0.0)
                        .flex_grow(1.0_f32)
                        .flex_basis(0.0)
                })
            }))
            .style(move |s| {
                s.width_full()
                    .padding_vert(3.0)
                    .padding_horiz(6.0)
                    .apply_if(shaded, |s| s.background(PANEL))
            })
        },
    )
    .style(|s| s.flex_col().width_full())
}

fn buttons_block(labels: &'static [&'static str]) -> impl IntoView {
    h_stack_from_iter(labels.iter().map(|name| {
        container(text(*name).style(text_style(Type::new(CHAT_TEXT, 1.4, Face::Bold, INK)))).style(
            |s| {
                s.padding_vert(6.0)
                    .padding_horiz(10.0)
                    .background(BUBBLE)
                    .border_radius(6.0)
            },
        )
    }))
    .style(|s| s.gap(8.0))
}

// A comment that names the turn, then as much of `code` as arrived, each
// stretch in its color.
fn highlighted(turn: usize, code: &'static str, shown: usize) -> floem::text::TextLayout {
    let chars = chat_shown_chars(code, shown.min(code.len()));
    let head = format!("// {}\n", chat_turn_label(turn));
    let mut spans = vec![(0..head.len(), rgb(CodeKind::Comment.rgb()))];
    for (span, kind) in chat_code_spans(code) {
        if span.start >= chars {
            break;
        }
        spans.push((
            head.len() + span.start..head.len() + span.end.min(chars),
            rgb(kind.rgb()),
        ));
    }
    CODE.shape_spans(&format!("{head}{}", &code[..chars]), &spans, None)
}

// The status line under the list: a spinner that turns each frame, and what
// the agent does, glyph by glyph in a gradient that moves each frame.
fn status(frame: Frame, token: Token) -> impl IntoView {
    let spinner = vector(CHAT_SVG).style(move |s| {
        s.size(CHAT_SPINNER_SIZE, CHAT_SPINNER_SIZE)
            .flex_shrink(0.0_f32)
            .rotate((frame.get() as f32 * CHAT_SPIN_DEGREES).to_radians())
    });
    let status_type = Type::new(CHAT_TEXT, 1.4, Face::Regular, INK);
    // Where each glyph of the status lies depends on its text alone, which a
    // token changes.
    let glyphs = create_memo(move |_| {
        let said = token.get().map(chat_status).unwrap_or_default();
        let plain = status_type.shape(&said, None);
        let places = plain
            .layout_runs()
            .flat_map(|run| {
                run.glyphs
                    .iter()
                    .map(|glyph| (glyph.start..glyph.end, glyph.x))
            })
            .collect::<Vec<_>>();
        (said, places)
    });
    let [dim, bright] = CHAT_SHIMMER_RGB;
    let said = rich_text(move || {
        let (said, places) = glyphs.get();
        let frame = frame.get();
        let spans: Vec<_> = places
            .into_iter()
            .map(|(range, x)| {
                let weight = chat_shimmer(x, frame);
                let channel = |index: usize| {
                    dim[index] + ((bright[index] - dim[index]) as f32 * weight) as u8
                };
                (range, Color::rgb8(channel(0), channel(1), channel(2)))
            })
            .collect();
        status_type.shape_spans(&said, &spans, None)
    });
    h_stack((spinner, said)).style(|s| {
        s.width_full()
            .height(CHAT_STATUS_HEIGHT)
            .flex_shrink(0.0_f32)
            .padding_horiz(8.0)
            .gap(6.0)
            .items_center()
            .background(PANEL)
    })
}

// Every frame of the animated picture; none when it does not decode.
fn gif_frames() -> Vec<floem::peniko::Image> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut frames = Vec::new();
    match options.read_info(CHAT_GIF) {
        Ok(mut decoder) => {
            let (width, height) = (u32::from(decoder.width()), u32::from(decoder.height()));
            while let Ok(Some(frame)) = decoder.read_next_frame() {
                frames.push(bitmap(width, height, frame.buffer.to_vec()));
            }
        }
        Err(error) => log::warn!("chat picture: {error}"),
    }
    frames
}
