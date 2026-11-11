// The roll of a performance: a scrolling column of the song's seconds, with
// the song's start at the bottom. The parts in its content compose their
// notes into each second, each note a rectangle over the key it plays.
//
// The place of the column and the clock of the performance are one value.
// The lower edge of the roll is the line the notes sound at. While the piece
// plays, the clock moves the column. When a hand moves the column, by a
// drag, a wheel, a fling or the scrollbar, the column moves the clock, and
// the notes that cross the line sound.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{
    compositionLocalOf, CompositionLocal, CompositionLocalProvider, LaunchedEffect, SideEffect,
};
use cranpose_foundation::lazy::{rememberLazyListState, LazyListState};
use cranpose_ui::{
    composable, Box as CranposeBox, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Color,
    LazyColumn, LazyColumnSpec, LazyListScope, Modifier,
};
use cranpose_ui_graphics::whole;

use super::{
    key_range::KeyRange,
    performance::{local_performance, Performance},
    song::note_seconds,
};
use crate::app::{
    lazy_scrollbar::{LazyScrollbarRail, LazyScrollbarStyle, RailStart},
    ticking::seconds_between,
};

// The column holds the song in slices of one second, one item each.
const SLICE_HEIGHT: f32 = 120.0;
// The line at the lower edge.
const LINE_HEIGHT: f32 = 1.5;
const LINE: Color = Color(1.0, 1.0, 1.0, 0.28);
// Places of the column closer than half a pixel are one place.
const SAME_PLACE_SECONDS: f64 = 0.5 / SLICE_HEIGHT as f64;
// A hand that has not moved the column for this long has let go of the clock.
const HAND_REST_SECONDS: f32 = 0.25;
// The time one frame of the display takes, to tell a fling slower than the
// piece from a faster one. A display twice as fast gives the clock back at
// twice the speed, which is as good.
const FRAME_SECONDS: f64 = 1.0 / 60.0;
const SCROLLBAR_TAG: &str = "MusicRollScrollbar";
const SCROLLBAR: LazyScrollbarStyle = LazyScrollbarStyle {
    rail_width: 10.0,
    thumb_width: 4.0,
    min_thumb_height: 28.0,
    rail_color: Color::TRANSPARENT,
    thumb_color: Color(1.0, 1.0, 1.0, 0.35),
};

// One second of a roll, as the content composed in it sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RollSlice {
    // The song seconds the slice starts at; it ends one second later.
    pub from: f32,
    pub keys: KeyRange,
    // The width of the roll, which the keys of `keys` share.
    pub width: f32,
}

impl RollSlice {
    // The height of `seconds` of the song on a roll.
    pub fn height_of(seconds: f32) -> f32 {
        seconds * SLICE_HEIGHT
    }

    // The seconds of the song that `height` of a roll shows.
    pub fn seconds_of(height: f32) -> f32 {
        height / SLICE_HEIGHT
    }

    // The song seconds the slice ends at.
    pub fn to(self) -> f32 {
        self.from + 1.0
    }
}

// The slice of a roll the caller is composed in, if it is in one.
pub fn local_roll_slice() -> CompositionLocal<Option<RollSlice>> {
    thread_local! {
        static LOCAL_ROLL_SLICE: RefCell<Option<CompositionLocal<Option<RollSlice>>>> = const { RefCell::new(None) };
    }
    LOCAL_ROLL_SLICE.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(|| None))
            .clone()
    })
}

// What a roll composes in each of its slices.
#[derive(Clone)]
struct SliceContent(Rc<dyn Fn()>);

impl PartialEq for SliceContent {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

// The whole part of a count that is not negative.
fn count(value: f32) -> usize {
    usize::try_from(whole::to_i32(value)).unwrap_or(0)
}

// The seconds a slice starts at.
fn slice_start(index: usize) -> f32 {
    f32::from(u16::try_from(index).unwrap_or(u16::MAX))
}

// The item at the line and how far it is scrolled past it, for a clock at
// `seconds`.
fn place_of(seconds: f64) -> (usize, f32) {
    let seconds = note_seconds(seconds.max(0.0));
    let index = count(seconds);
    (index, (seconds - slice_start(index)) * SLICE_HEIGHT)
}

// The song seconds at the line, for a column at this place.
fn seconds_at(index: usize, offset: f32) -> f64 {
    f64::from(slice_start(index)) + f64::from(offset / SLICE_HEIGHT)
}

// Scrolls the song of the performance around the call over the keys of
// `keys`. `content` is composed once in each second of the song that is on
// the roll or near it, and finds that second in `local_roll_slice`: a `Part`
// in it composes its notes of that second there.
#[composable]
pub fn Roll(keys: KeyRange, modifier: Modifier, content: impl Fn() + 'static) {
    let Some(performance) = local_performance().current() else {
        return;
    };
    let list_state = rememberLazyListState();
    let song_seconds = performance.player().length();
    let content = SliceContent(Rc::new(content));

    FollowClock(list_state, performance.clone());
    FollowHand(list_state, performance);

    BoxWithConstraints(modifier.clip_to_bounds(), move |scope| {
        let width = scope.constraints().max_width;
        let height = scope.constraints().max_height;
        // The slices of the song, and above them as many empty ones as fill
        // the roll, so the last note can come down to the line.
        let slices = count(song_seconds.ceil()) + count((height / SLICE_HEIGHT).ceil());
        let content = content.clone();
        LazyColumn(
            Modifier::empty().fill_max_size(),
            list_state,
            LazyColumnSpec::new().reverse_layout(true),
            move |scope| {
                scope.items(slices, move |index| {
                    Slice(
                        RollSlice {
                            from: slice_start(index),
                            keys,
                            width,
                        },
                        content.clone(),
                    );
                });
            },
        );
        CranposeBox(
            Modifier::empty()
                .offset(0.0, height - LINE_HEIGHT)
                .fill_max_width()
                .height(LINE_HEIGHT)
                .background(LINE),
            BoxSpec::default(),
            || {},
        );
        CranposeBox(
            Modifier::empty()
                .offset(width - SCROLLBAR.rail_width, 0.0)
                .fill_max_height(),
            BoxSpec::default(),
            move || LazyScrollbarRail(list_state, SCROLLBAR_TAG, SCROLLBAR, RailStart::Bottom),
        );
    });
}

// One second of the song: the roll's content, composed for this second.
#[composable]
fn Slice(slice: RollSlice, content: SliceContent) {
    CranposeBox(
        Modifier::empty().fill_max_width().height(SLICE_HEIGHT),
        BoxSpec::default(),
        move || {
            let content = content.clone();
            CompositionLocalProvider([local_roll_slice().provides(Some(slice))], move || {
                (content.0)();
            });
        },
    );
}

fn stands_at(list_state: LazyListState) -> f64 {
    seconds_at(
        list_state.first_visible_item_index_non_reactive(),
        list_state.first_visible_item_scroll_offset_non_reactive(),
    )
}

fn put_at(list_state: LazyListState, seconds: f64) {
    let (index, offset) = place_of(seconds);
    list_state.scroll_to_item(index, offset);
}

// Puts the column where the clock stands. It composes again with each move
// of the clock and of nothing else.
#[composable]
fn FollowClock(list_state: LazyListState, performance: Performance) {
    let seconds = performance.seconds();
    SideEffect(move || {
        // A finger on the column, or a turn of the wheel the column has not
        // laid out yet, is the hand taking the clock: a place put here would
        // undo it. The pushes of a fling the hand let go of are undone.
        let wheel = !list_state.is_scroll_in_progress() && list_state.peek_scroll_delta() != 0.0;
        if performance.is_dragged() || list_state.is_dragged() || wheel {
            return;
        }
        if (stands_at(list_state) - seconds).abs() > SAME_PLACE_SECONDS {
            put_at(list_state, seconds);
        }
    });
}

// Moves the clock where a hand put the column, and gives the clock back
// when the hand rests. It composes again with each move of the column.
#[composable]
fn FollowHand(list_state: LazyListState, performance: Performance) {
    let column = seconds_at(
        list_state.first_visible_item_index(),
        list_state.first_visible_item_scroll_offset(),
    );
    let finger = list_state.is_dragged();
    {
        let performance = performance.clone();
        SideEffect(move || {
            let clock = performance.seconds();
            let apart = (column - clock).abs();
            if apart <= SAME_PLACE_SECONDS {
                return;
            }
            let fling = list_state.is_scroll_in_progress() && !finger;
            if fling && !performance.is_dragged() {
                // The last pushes of a fling the hand already let go of.
                put_at(list_state, clock);
                return;
            }
            performance.drag_to(column);
            // A fling slower than the piece itself gives the clock back to
            // a piece that ran.
            let frame_of_play = f64::from(performance.player().tempo()) * FRAME_SECONDS;
            if fling && apart < frame_of_play {
                performance.end_running_drag();
            }
        });
    }
    let moves = performance.drag_moves();
    LaunchedEffect((moves, finger), async move |scope| {
        if moves.is_none() || finger {
            return;
        }
        // The rest is told in frames, the time the picture itself keeps.
        let since = scope.with_frame_nanos(|now| now).await;
        while seconds_between(since, scope.with_frame_nanos(|now| now).await) < HAND_REST_SECONDS {}
        performance.end_drag();
    });
}
