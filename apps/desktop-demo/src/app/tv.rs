// A TV home screen a remote drives: rows of cards that the D-pad walks, each
// row keeping its card while focus visits other rows, and a player that
// takes Left and Right for seeking and Back to close. The rows are lazy and
// longer than the screen in both directions, which is the case a held key
// measures on a TV.

use cranpose::LazyItems;
use cranpose_core::{rememberMutableStateOf, LaunchedEffect, MutableState};
use cranpose_foundation::lazy::{rememberLazyListState, LazyListScope};
use cranpose_ui::{
    composable,
    text::{SpanStyle, TextUnit},
    widgets::{LazyColumn, LazyColumnSpec, LazyRow, LazyRowSpec},
    Box as CranposeBox, BoxSpec, Color, Column, ColumnSpec, FocusRequester, Key, KeyEvent,
    KeyEventType, LinearArrangement, Modifier, PaddingValues, RoundedCornerShape, Size, Text,
    TextStyle,
};

const ROWS: usize = 12;
const CARDS: usize = 30;
const CARD: Size = Size {
    width: 220.0,
    height: 124.0,
};
const FOCUSED_SCALE: f32 = 1.1;
// The side margin a TV screen keeps clear, Compose for TV's 58 dp.
const EDGE: f32 = 58.0;
const SEEK_SECONDS: u32 = 10;
const LENGTH_SECONDS: u32 = 3600;
const BACKGROUND: Color = Color::from_rgb_u8(0x10, 0x12, 0x18);
const TEXT: Color = Color::from_rgb_u8(0xEC, 0xEE, 0xF4);
const DIM: Color = Color::from_rgb_u8(0x9A, 0xA1, 0xB2);
const PALETTE: [Color; 6] = [
    Color::from_rgb_u8(0x2F, 0x5D, 0x8C),
    Color::from_rgb_u8(0x7A, 0x3E, 0x6F),
    Color::from_rgb_u8(0x2E, 0x6E, 0x55),
    Color::from_rgb_u8(0x8A, 0x5A, 0x2B),
    Color::from_rgb_u8(0x4B, 0x46, 0x8F),
    Color::from_rgb_u8(0x7D, 0x34, 0x34),
];

fn style(size: f32, color: Color) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(size),
            color: Some(color),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[composable]
pub(crate) fn TvTab() {
    let playing = rememberMutableStateOf(|| None::<(usize, usize)>);
    match playing.get() {
        Some(card) => Player(card, playing),
        None => Home(playing),
    }
}

#[composable]
fn Home(playing: MutableState<Option<(usize, usize)>>) {
    LazyColumn(
        Modifier::empty().fill_max_size().background(BACKGROUND),
        rememberLazyListState(),
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(LazyItems::new(ROWS), move |row| CardRow(row, playing));
        },
    );
}

#[composable]
fn CardRow(row: usize, playing: MutableState<Option<(usize, usize)>>) {
    Column(
        Modifier::empty()
            .fill_max_width()
            .padding_each(0.0, 24.0, 0.0, 8.0),
        ColumnSpec::default(),
        move || {
            Text(
                format!("Row {}", row + 1),
                Modifier::empty().padding_each(EDGE, 0.0, 0.0, 0.0),
                style(22.0, TEXT),
            );
            LazyRow(
                Modifier::empty()
                    .fill_max_width()
                    .height(CARD.height * FOCUSED_SCALE + 24.0)
                    .focus_restorer(None),
                rememberLazyListState(),
                LazyRowSpec {
                    horizontal_arrangement: LinearArrangement::SpacedBy(20.0),
                    content_padding: PaddingValues::symmetric(EDGE, 0.0),
                    ..LazyRowSpec::default()
                },
                move |scope| {
                    scope.items(LazyItems::new(CARDS), move |card| {
                        Card(row, card, playing);
                    });
                },
            );
        },
    );
}

#[composable]
fn Card(row: usize, card: usize, playing: MutableState<Option<(usize, usize)>>) {
    let focused = rememberMutableStateOf(|| false);
    let is_focused = focused.get();
    let scale = if is_focused { FOCUSED_SCALE } else { 1.0 };
    let border = if is_focused {
        Color::WHITE
    } else {
        Color::TRANSPARENT
    };
    CranposeBox(
        Modifier::empty()
            .padding_each(12.0, 12.0, 0.0, 12.0)
            .size(CARD)
            .scale(scale)
            .background(PALETTE[(row + card) % PALETTE.len()])
            .rounded_corners(12.0)
            .border(3.0, border, RoundedCornerShape::uniform(12.0))
            .on_focus_changed(move |state| focused.set(state.is_focused()))
            .clickable(move |_| playing.set(Some((row, card))))
            .content_description(format!("Row {}, title {}", row + 1, card + 1)),
        BoxSpec::default(),
        move || {
            Text(
                format!("Title {}", card + 1),
                Modifier::empty().padding(16.0),
                style(18.0, TEXT),
            );
        },
    );
}

#[composable]
fn Player(card: (usize, usize), playing: MutableState<Option<(usize, usize)>>) {
    let position = rememberMutableStateOf(|| 0_u32);
    let requester = cranpose_core::remember(FocusRequester::new).with(Clone::clone);
    let focus = requester.clone();
    LaunchedEffect(card, move |_| {
        focus.request_focus().ok();
    });
    let seconds = position.get();
    CranposeBox(
        Modifier::empty()
            .fill_max_size()
            .background(Color::BLACK)
            .padding(48.0)
            .focus_requester(&requester)
            .focus_target()
            .on_key_event(move |event| seek(event, position, playing)),
        BoxSpec::default(),
        move || {
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                Text(
                    format!("Row {}, title {}", card.0 + 1, card.1 + 1),
                    Modifier::empty(),
                    style(32.0, TEXT),
                );
                Text(
                    format!(
                        "{}:{:02} of {}:00 - Left and Right seek, Back closes",
                        seconds / 60,
                        seconds % 60,
                        LENGTH_SECONDS / 60
                    ),
                    Modifier::empty().padding_each(0.0, 16.0, 0.0, 16.0),
                    style(20.0, DIM),
                );
                CranposeBox(
                    Modifier::empty()
                        .fill_max_width()
                        .height(8.0)
                        .background(Color::from_rgb_u8(0x30, 0x34, 0x40)),
                    BoxSpec::default(),
                    move || {
                        CranposeBox(
                            Modifier::empty()
                                .fill_max_width_fraction(seconds as f32 / LENGTH_SECONDS as f32)
                                .height(8.0)
                                .background(Color::WHITE),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            });
        },
    );
}

// Takes the player's keys before focus moves: Left and Right seek, held keys
// keep seeking, and Back or Escape closes the player.
fn seek(
    event: &KeyEvent,
    position: MutableState<u32>,
    playing: MutableState<Option<(usize, usize)>>,
) -> bool {
    if event.type_() != KeyEventType::KeyDown {
        return false;
    }
    match event.key() {
        Key::DirectionLeft | Key::MediaRewind => {
            position.set(position.get().saturating_sub(SEEK_SECONDS));
            true
        }
        Key::DirectionRight | Key::MediaFastForward => {
            position.set((position.get() + SEEK_SECONDS).min(LENGTH_SECONDS));
            true
        }
        Key::Back | Key::Escape => {
            playing.set(None);
            true
        }
        _ => false,
    }
}
