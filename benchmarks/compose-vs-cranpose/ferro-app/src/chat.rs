//! The chat beside the patchwork (`../README.md`): an agent's answers arrive
//! from a data thread as tokens and the list shows them as they come.
//!
//! In FerroUI's idiom the chat is a `ListBox` over a list of block numbers.
//! Its virtualizing panel realizes the blocks in view and its template
//! recycles the components of blocks that left, so a token touches no list
//! item. A block's text reads the signals of the last block and of the tokens
//! shown, and only the last block reads the second one, so a token sets the
//! text of that block alone. A code block appends colored runs to its text
//! and a table appends rows. The viewer follows the end when its extent
//! changes. The status line's spinner turns in a render transform and its
//! text has a gradient brush whose end points move; the animated picture
//! changes its source when its frame index changes.

use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
};

use ferroui_base::{
    PixelSize, Ref, RelativePoint, RelativeUnit,
    collections::FerroList,
    layout::{HorizontalAlignment, Orientation, VerticalAlignment},
    media::{
        Color, Geometry, GradientSpreadMethod, GradientStop, GradientStops, IBrush, IImage,
        ITransform, LinearGradientBrush, PenLineCap, RotateTransform, ScaleTransform, Stretch,
        TextWrapping, imaging::Bitmap,
    },
    platform::{AlphaFormat, PixelFormats},
};
use ferroui_controls::{
    Canvas, Grid, ItemsSource, ListBox, ScrollViewer, TextBlock,
    documents::{InlineCollection, Run},
    primitives::{ScrollBarVisibility, TemplatedControl},
};
use ferroui_view::{prelude::*, template};
use perf_data::*;

use super::txt;

const INK: &str = "#FFFFFF";
const MONO: &str = "fonts:Device#DejaVu Sans Mono";

fn color(rgb: [u8; 3]) -> Color {
    Color::from_rgb(rgb[0], rgb[1], rgb[2])
}

fn brush(rgb: [u8; 3]) -> Rc<dyn IBrush> {
    Rc::new(ferroui_base::media::immutable::ImmutableSolidColorBrush::new(color(rgb)))
}

/// What the chat shows: the blocks so far, the last block's number, how much
/// of it shows, the last token and the animated picture's frame.
#[derive(Clone)]
struct Shown {
    last: Signal<usize>,
    shown: Signal<usize>,
    token: Signal<ChatToken>,
    gif: Signal<usize>,
    frames: Rc<[Rc<dyn IImage>]>,
}

pub(crate) fn chat(frame: Signal<u32>) -> ferroui_view::El<Grid> {
    let blocks: FerroList<usize> = FerroList::new();
    let state = Shown {
        last: signal(0),
        shown: signal(0),
        token: signal(ChatToken::default()),
        gif: signal(0),
        frames: decode_gif(),
    };
    let feed = ChatFeed::start()
        .map_err(|error| log::warn!("chat data thread: {error}"))
        .ok();
    let list = blocks.clone();
    let driven = state.clone();
    effect(move || {
        let frame = frame.get();
        driven
            .gif
            .set_if_changed(chat_gif_frame(frame, driven.frames.len()));
        let Some(feed) = &feed else { return };
        batch(|| {
            for _ in 0..CHAT_TOKENS_PER_FRAME {
                let Some(token) = feed.take() else { break };
                while list.count() <= token.block {
                    list.add(list.count());
                }
                driven.last.set_if_changed(token.block);
                driven.shown.set(token.shown);
                driven.token.set(token);
            }
        });
    });
    let token = state.token;
    let items = ItemsSource::new(Rc::new(blocks));
    grid()
        .row_definitions(&format!("*,{CHAT_STATUS_HEIGHT}"))
        .background(color(CHAT_BACKGROUND_RGB))
        .child(
            list_box()
                .background("#00000000")
                .border_thickness(0.0)
                .padding((8.0, 0.0, 8.0, 8.0))
                .styles([style("ListBoxItem")
                    .set(TemplatedControl::padding_property(), (0.0, 8.0, 0.0, 0.0))])
                .items_source(items)
                .item_template(template(move |index: usize| block(index, &state)))
                .with(follow_end),
        )
        .child(status(frame, token).grid_row(1))
}

/// A list that shows no scroll bars (scrolled by code) and keeps its viewer at its end while
/// the viewer's extent changes.
fn follow_end(list: &Ref<ListBox>) {
    ScrollViewer::set_horizontal_scroll_bar_visibility_on(list, ScrollBarVisibility::Disabled);
    ScrollViewer::set_vertical_scroll_bar_visibility_on(list, ScrollBarVisibility::Hidden);
    list.add_handler(ScrollViewer::scroll_changed_event(), |_, event| {
        if event.extent_delta().y != 0.0 {
            if let Some(viewer) = event
                .source()
                .and_then(|source| source.cast::<ScrollViewer>())
            {
                viewer.scroll_to_end();
            }
        }
    });
}

/// Block `index` of the chat.
fn block(index: usize, state: &Shown) -> Element {
    match CHAT_TURN[index % CHAT_TURN.len()] {
        ChatBlock::User(text) => user(text),
        ChatBlock::Heading(text) => typed(index, text, true, state),
        ChatBlock::Paragraph(text) => typed(index, text, false, state),
        ChatBlock::Code(code) => code_block(index, code, state),
        ChatBlock::Table(rows) => table(index, rows, state),
        ChatBlock::Gif => gif(state),
        ChatBlock::Buttons(labels) => buttons(labels),
    }
}

fn user(text: &'static str) -> Element {
    border()
        .background(color(CHAT_USER_RGB))
        .corner_radius(10.0)
        .padding((10.0, 6.0, 10.0, 6.0))
        .horizontal_alignment(HorizontalAlignment::Right)
        .child(txt(text, f64::from(CHAT_TEXT), INK, false).text_wrapping(TextWrapping::Wrap))
        .into()
}

/// The characters of a block's text that show: the tokens that arrived while
/// the block is the last one, all of it after.
fn shown_chars(index: usize, length: usize, state: &Shown) -> usize {
    if state.last.get() == index {
        (state.shown.get() * CHAT_TOKEN_CHARS).min(length)
    } else {
        length
    }
}

/// A paragraph, or a heading in the bold face: the turn's number, then as
/// much of `text` as arrived.
fn typed(index: usize, text: &'static str, heading: bool, state: &Shown) -> Element {
    let size = if heading {
        CHAT_HEADING_TEXT
    } else {
        CHAT_TEXT
    };
    let label = chat_turn_label(ChatToken::turn(index));
    let state = state.clone();
    txt("", f64::from(size), INK, heading)
        .text_wrapping(TextWrapping::Wrap)
        .text(move || {
            let chars = shown_chars(index, text.len(), &state);
            format!("{label} {}", &text[..chars])
        })
        .into()
}

/// The pieces of a code block's text, which cover it: a comment that names
/// the turn, then the code with its colored spans.
fn code_pieces(head: usize, code: &str) -> Vec<(Range<usize>, Option<CodeKind>)> {
    let mut pieces = vec![(0..head, Some(CodeKind::Comment))];
    let mut at = head;
    for (span, kind) in chat_code_spans(code) {
        let (start, end) = (head + span.start, head + span.end);
        if at < start {
            pieces.push((at..start, None));
        }
        pieces.push((start..end, Some(kind)));
        at = end;
    }
    if at < head + code.len() {
        pieces.push((at..head + code.len(), None));
    }
    pieces
}

/// Code in the mono face: one run per piece, made when its first character
/// arrives. A token sets the text of the run it ends in and makes the runs
/// it reaches. The runs are in the text block: a block of another number is
/// another control, so the key.
fn code_block(index: usize, code: &'static str, state: &Shown) -> Element {
    let head = format!("// {}\n", chat_turn_label(ChatToken::turn(index)));
    let whole = format!("{head}{code}");
    let pieces = code_pieces(head.len(), code);
    let state = state.clone();
    border()
        .key(index)
        .background(color(CHAT_PANEL_RGB))
        .corner_radius(6.0)
        .padding(8.0)
        .child(
            txt("", f64::from(CHAT_CODE_TEXT), INK, false)
                .font_family(ferroui_base::media::FontFamily::from(MONO))
                .with(move |text_block: &Ref<TextBlock>| {
                    let inlines = InlineCollection::new();
                    text_block.set_inlines(Some(inlines.clone()));
                    let runs: RefCell<Vec<Ref<Run>>> = RefCell::new(Vec::new());
                    let shown = Cell::new(0);
                    let (whole, pieces, state) = (whole.clone(), pieces.clone(), state.clone());
                    let head = head.len();
                    effect(move || {
                        let chars = head + shown_chars(index, code.len(), &state);
                        let before = shown.replace(chars);
                        let mut runs = runs.borrow_mut();
                        for (number, (range, kind)) in pieces.iter().enumerate() {
                            if range.end <= before {
                                continue;
                            }
                            if range.start >= chars {
                                break;
                            }
                            let text = &whole[range.start..range.end.min(chars)];
                            if let Some(run) = runs.get(number) {
                                run.set_text(Some(text));
                            } else {
                                let run = Run::with_text(Some(text));
                                if let Some(kind) = kind {
                                    run.set_foreground(Some(brush(kind.rgb())));
                                }
                                inlines.add(run.clone());
                                runs.push(run);
                            }
                        }
                    });
                }),
        )
        .into()
}

/// A table whose rows arrive a token each.
fn table(
    index: usize,
    rows: &'static [[&'static str; CHAT_TABLE_COLUMNS]],
    state: &Shown,
) -> Element {
    let state = state.clone();
    stack_panel()
        .child(for_each(
            move || (0..shown_rows(index, rows.len(), &state)).collect::<Vec<_>>(),
            |row| *row,
            move |row| table_row(&rows[row], row == 0, row % 2 == 1),
        ))
        .into()
}

fn shown_rows(index: usize, length: usize, state: &Shown) -> usize {
    if state.last.get() == index {
        state.shown.get().min(length)
    } else {
        length
    }
}

fn table_row(row: &[&'static str; CHAT_TABLE_COLUMNS], header: bool, shaded: bool) -> Element {
    let cells = row
        .iter()
        .enumerate()
        .map(|(column, text)| {
            txt(*text, f64::from(CHAT_CODE_TEXT), INK, header).grid_column(column as i32)
        })
        .collect::<Vec<_>>();
    let row = border()
        .padding((6.0, 3.0, 6.0, 3.0))
        .child(grid().column_definitions("*,*,*,*").children(cells));
    if header || shaded {
        row.background(color(CHAT_PANEL_RGB)).into()
    } else {
        row.into()
    }
}

/// Every frame of the GIF as a bitmap; none when it does not decode.
fn decode_gif() -> Rc<[Rc<dyn IImage>]> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut frames: Vec<Rc<dyn IImage>> = Vec::new();
    match options.read_info(CHAT_GIF) {
        Ok(mut decoder) => {
            let (width, height) = (i32::from(decoder.width()), i32::from(decoder.height()));
            while let Ok(Some(frame)) = decoder.read_next_frame() {
                frames.push(Rc::new(Bitmap::from_pixels(
                    PixelFormats::RGBA8888,
                    AlphaFormat::Unpremul,
                    &frame.buffer,
                    PixelSize::new(width, height),
                    ferroui_base::Vector::new(96.0, 96.0),
                    width * 4,
                )));
            }
        }
        Err(error) => log::warn!("chat picture: {error}"),
    }
    frames.into()
}

/// The animated picture: its source follows the frame index.
fn gif(state: &Shown) -> Element {
    let state = state.clone();
    image()
        .width(f64::from(CHAT_GIF_SIZE))
        .height(f64::from(CHAT_GIF_SIZE))
        .horizontal_alignment(HorizontalAlignment::Left)
        .stretch(Stretch::Fill)
        .source(move || state.frames.get(state.gif.get()).cloned())
        .into()
}

fn buttons(labels: &'static [&'static str]) -> Element {
    stack_panel()
        .orientation(Orientation::Horizontal)
        .spacing(8.0)
        .children(
            labels
                .iter()
                .map(|label| {
                    button()
                        .background(color(CHAT_USER_RGB))
                        .corner_radius(6.0)
                        .padding((10.0, 6.0, 10.0, 6.0))
                        .child(txt(*label, f64::from(CHAT_TEXT), INK, true))
                })
                .collect::<Vec<_>>(),
        )
        .into()
}

/// The status line: the spinner and what the agent does.
fn status(frame: Signal<u32>, token: Signal<ChatToken>) -> ferroui_view::El<Border> {
    border()
        .background(color(CHAT_PANEL_RGB))
        .padding((8.0, 0.0, 8.0, 0.0))
        .child(
            stack_panel()
                .orientation(Orientation::Horizontal)
                .spacing(6.0)
                .child(spinner(frame))
                .child(status_text(frame, token)),
        )
}

/// The value of attribute `name` in the tag `tag` of the spinner's SVG.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let start = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    tag[start..].split('"').next()
}

/// The SVG's paths drawn as path controls in a box of the SVG's size, which
/// an inner canvas scales to the spinner's side. FerroUI has no SVG: the paths'
/// data, strokes and caps are read from the SVG's text.
fn spinner(frame: Signal<u32>) -> Element {
    let size: f64 = attribute(CHAT_SVG, "width")
        .and_then(|width| width.parse().ok())
        .unwrap_or(24.0);
    let paths = CHAT_SVG
        .split("<path")
        .skip(1)
        .filter_map(|tag| {
            let tag = tag.split("/>").next()?;
            let data = Geometry::parse(attribute(tag, "d")?).ok()?;
            let width: f64 = attribute(tag, "stroke-width")?.parse().ok()?;
            let line = path()
                .data(data)
                .stroke(attribute(tag, "stroke")?)
                .stroke_thickness(width);
            Some(if attribute(tag, "stroke-linecap") == Some("round") {
                line.stroke_line_cap(PenLineCap::Round)
            } else {
                line
            })
        })
        .collect::<Vec<_>>();
    let scale = ScaleTransform::new();
    scale.set_scale_x(f64::from(CHAT_SPINNER_SIZE) / size);
    scale.set_scale_y(f64::from(CHAT_SPINNER_SIZE) / size);
    let scale: Rc<dyn ITransform> = scale.into();
    canvas()
        .width(f64::from(CHAT_SPINNER_SIZE))
        .height(f64::from(CHAT_SPINNER_SIZE))
        .vertical_alignment(VerticalAlignment::Center)
        .with(move |control: &Ref<Canvas>| {
            let rotate = RotateTransform::new();
            let transform: Rc<dyn ITransform> = rotate.clone().into();
            control.set_render_transform(Some(transform));
            effect(move || rotate.set_angle(f64::from(frame.get()) * f64::from(CHAT_SPIN_DEGREES)));
        })
        .child(
            canvas()
                .width(size)
                .height(size)
                .render_transform(Some(scale))
                .render_transform_origin(RelativePoint::TOP_LEFT)
                .children(paths),
        )
        .into()
}

/// The status text under a gradient that runs dim, bright, dim over
/// `CHAT_SHIMMER_SPAN` points and repeats; each frame moves its end points.
fn status_text(frame: Signal<u32>, token: Signal<ChatToken>) -> ferroui_view::El<TextBlock> {
    txt("", f64::from(CHAT_TEXT), INK, false)
        .vertical_alignment(VerticalAlignment::Center)
        .text(move || chat_status(token.get()))
        .with(move |text_block: &Ref<TextBlock>| {
            let [dim, bright] = CHAT_SHIMMER_RGB;
            let gradient = LinearGradientBrush::new();
            gradient.set_spread_method(GradientSpreadMethod::Repeat);
            gradient.set_gradient_stops(GradientStops::from_items([
                GradientStop::with_color_and_offset(color(dim), 0.0),
                GradientStop::with_color_and_offset(color(bright), 0.5),
                GradientStop::with_color_and_offset(color(dim), 1.0),
            ]));
            text_block.set_foreground(Some(gradient.clone().into()));
            effect(move || {
                let start = f64::from((frame.get() as f32 * CHAT_SHIMMER_STEP) % CHAT_SHIMMER_SPAN);
                gradient.set_start_point(RelativePoint::new(start, 0.0, RelativeUnit::Absolute));
                gradient.set_end_point(RelativePoint::new(
                    start + f64::from(CHAT_SHIMMER_SPAN),
                    0.0,
                    RelativeUnit::Absolute,
                ));
            });
        })
}
