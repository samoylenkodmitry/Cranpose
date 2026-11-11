// The roll of a performance: a scrolling column of its notes, each a
// rectangle over the key it plays, with the song's start at the bottom.
//
// The place of the column and the clock of the performance are one value.
// The lower edge of the roll is the line the notes sound at. While the piece
// plays, the clock moves the column. When a hand moves the column, by a
// drag, a wheel, a fling or the scrollbar, the column moves the clock, and
// the notes that cross the line sound.

use std::rc::Rc;

use cranpose_animation::Lerp;
use cranpose_core::{with_key, LaunchedEffect, SideEffect};
use cranpose_foundation::lazy::{rememberLazyListState, LazyListState};
use cranpose_ui::{
    composable, Box as CranposeBox, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Brush,
    Canvas, Color, CornerRadii, LazyColumn, LazyColumnSpec, LazyListScope, Modifier, Point, Rect,
};
use cranpose_ui_graphics::whole;

use super::{
    key_range::KeyRange,
    performance::{local_performance, Performance},
    player::ShownPart,
    song::{note_seconds, Note},
};
use crate::app::{
    lazy_scrollbar::{LazyScrollbarRail, LazyScrollbarStyle, RailStart},
    ticking::seconds_between,
};

// The column holds the song in slices of one second, one item each.
const SLICE_HEIGHT: f32 = 120.0;
const NOTE_CORNER: f32 = 4.0;
// The time the pedal lets a note ring past its key is drawn this faint.
const PEDAL_ALPHA: f32 = 0.2;
// A note that waits for the line is drawn dimmer than one that sounds.
const WAITING_ALPHA: f32 = 0.78;
// How far a note that sounds is mixed toward white.
const SOUNDING_LIGHT: f32 = 0.35;
// The halo around a note that sounds, and the bright end every note leads with.
const HALO: f32 = 4.0;
const HALO_ALPHA: f32 = 0.3;
const HEAD_HEIGHT: f32 = 5.0;
const HEAD_LIGHT: f32 = 0.55;
// The line at the lower edge, and the glow of a key that is down: its
// height and strength at the strike, the share of both it keeps while the
// key is held, and how fast it falls there, per second.
const LINE_HEIGHT: f32 = 1.5;
const LINE_ALPHA: f32 = 0.28;
const GLOW_HEIGHT: f32 = 150.0;
const GLOW_ALPHA: f32 = 0.6;
const GLOW_REST: f32 = 0.3;
const GLOW_FADE: f32 = 3.0;
const SPARK_RADIUS: f32 = 14.0;
const SPARK_ALPHA: f32 = 0.9;
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

#[composable]
pub fn Roll(keys: KeyRange, modifier: Modifier) {
    let Some(performance) = local_performance().current() else {
        return;
    };
    let list_state = rememberLazyListState();
    let parts: Rc<[ShownPart]> = performance.parts().into();
    let song_seconds = performance.player().length();

    FollowClock(list_state, performance.clone());
    FollowHand(list_state, performance.clone());

    BoxWithConstraints(modifier.clip_to_bounds(), move |scope| {
        let width = scope.constraints().max_width;
        let height = scope.constraints().max_height;
        // The slices of the song, and above them as many empty ones as fill
        // the roll, so the last note can come down to the line.
        let slices = count(song_seconds.ceil()) + count((height / SLICE_HEIGHT).ceil());
        let parts = Rc::clone(&parts);
        let performance = performance.clone();
        LazyColumn(
            Modifier::empty().fill_max_size(),
            list_state,
            LazyColumnSpec::new().reverse_layout(true),
            move |scope| {
                scope.items(slices, move |index| {
                    Slice(index, Rc::clone(&parts), keys, width);
                });
            },
        );
        Sounding(keys, performance);
        CranposeBox(
            Modifier::empty()
                .offset(width - SCROLLBAR.rail_width, 0.0)
                .fill_max_height(),
            BoxSpec::default(),
            move || LazyScrollbarRail(list_state, SCROLLBAR_TAG, SCROLLBAR, RailStart::Bottom),
        );
    });
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

// What follows the clock on the roll, in one layer over the notes: the line
// the notes sound at, each note that sounds now drawn lit with a halo, and
// the glow of its key at the line, bright as the key goes down and fading
// while it is held. The notes under it are drawn once and only scroll.
#[composable]
fn Sounding(keys: KeyRange, performance: Performance) {
    Canvas(Modifier::empty().fill_max_size(), move |scope| {
        let size = scope.size();
        scope.draw_rect_at(
            Rect {
                x: 0.0,
                y: size.height - LINE_HEIGHT,
                width: size.width,
                height: LINE_HEIGHT,
            },
            Brush::solid(Color::WHITE.with_alpha(LINE_ALPHA)),
        );
        performance.for_each_key_down(|held| {
            let Some((left, width)) = keys.note_span(held.key, size.width) else {
                return;
            };
            // The note from the line up to the lift of its key. The part of
            // it under the line lies past the edge of the roll.
            let top = size.height - held.seconds_left * SLICE_HEIGHT;
            let bar = Rect {
                x: left,
                y: top,
                width,
                height: size.height - top + NOTE_CORNER,
            };
            scope.draw_round_rect_at(
                Rect {
                    x: bar.x - HALO,
                    y: bar.y - HALO,
                    width: bar.width + HALO * 2.0,
                    height: bar.height + HALO,
                },
                Brush::solid(held.color.with_alpha(HALO_ALPHA)),
                CornerRadii::uniform(NOTE_CORNER + HALO),
            );
            scope.draw_round_rect_at(
                bar,
                Brush::solid(held.color.lerp(&Color::WHITE, SOUNDING_LIGHT)),
                CornerRadii::uniform(NOTE_CORNER),
            );

            let fresh = (-held.seconds_down * GLOW_FADE).exp();
            let strength = GLOW_REST + (1.0 - GLOW_REST) * fresh;
            let glow_top = size.height - GLOW_HEIGHT * strength;
            scope.draw_rect_at(
                Rect {
                    x: left - width * 0.5,
                    y: glow_top,
                    width: width * 2.0,
                    height: size.height - glow_top,
                },
                Brush::vertical_gradient(
                    vec![
                        held.color.with_alpha(0.0),
                        held.color.with_alpha(GLOW_ALPHA * strength),
                    ],
                    glow_top,
                    size.height,
                ),
            );
            let center = Point {
                x: left + width * 0.5,
                y: size.height,
            };
            let radius = SPARK_RADIUS * (2.0 - fresh);
            scope.draw_circle(
                Brush::radial_gradient(
                    vec![
                        Color::WHITE.with_alpha(SPARK_ALPHA * fresh),
                        held.color.with_alpha(0.0),
                    ],
                    center,
                    radius,
                ),
                center,
                radius,
            );
        });
    });
}

// One second of the song: the notes of every part that sound in it.
#[composable]
fn Slice(index: usize, parts: Rc<[ShownPart]>, keys: KeyRange, width: f32) {
    CranposeBox(
        Modifier::empty().fill_max_width().height(SLICE_HEIGHT),
        BoxSpec::default(),
        move || {
            let from = slice_start(index);
            for (part_index, part) in parts.iter().enumerate() {
                let earliest = f64::from(from - part.notes.longest_ring());
                let first = part.notes.first_at_or_after(earliest);
                let end = part.notes.first_at_or_after(f64::from(from + 1.0));
                for (note_index, note) in (first..end).zip(&part.notes.as_slice()[first..end]) {
                    if note.silent_at() <= from {
                        continue;
                    }
                    with_key(&(part_index, note_index), || {
                        NoteBar(*note, part.transpose, part.color, from, keys, width);
                    });
                }
            }
        },
    );
}

// The part of one note that lies in the slice from `slice_from` seconds: the
// time its key is down as a bar with a bright end toward the line, and the
// time the pedal lets it ring past that as a faint bar. Nothing here reads
// the clock, so the note is drawn once.
#[composable]
fn NoteBar(note: Note, transpose: i8, color: Color, slice_from: f32, keys: KeyRange, width: f32) {
    let key = i16::from(note.key) + i16::from(transpose);
    let Some((left, bar_width)) = u8::try_from(key)
        .ok()
        .and_then(|key| keys.note_span(key, width))
    else {
        return;
    };
    let slice_to = slice_from + 1.0;
    // Seconds within the slice, from its start: later is higher on the roll.
    let rings = (note.start - slice_from).max(0.0)..(note.silent_at() - slice_from).min(1.0);
    let held = rings.start..(note.key_released_at() - slice_from).min(1.0);
    let starts_here = note.start >= slice_from;
    let key_lifts_here = note.key_released_at() <= slice_to;
    let pedal_ends_here = note.silent_at() <= slice_to;
    // A cut between two slices stays square.
    let corners = move |top_ends: bool| {
        let corner = |rounded: bool| if rounded { NOTE_CORNER } else { 0.0 };
        CornerRadii {
            top_left: corner(top_ends),
            top_right: corner(top_ends),
            bottom_right: corner(starts_here),
            bottom_left: corner(starts_here),
        }
    };

    CranposeBox(
        Modifier::empty()
            .offset(left, (1.0 - rings.end) * SLICE_HEIGHT)
            .size_points(bar_width, (rings.end - rings.start) * SLICE_HEIGHT)
            .draw_behind(move |scope| {
                let size = scope.size();
                scope.draw_round_rect(
                    Brush::solid(color.with_alpha(PEDAL_ALPHA)),
                    corners(pedal_ends_here),
                );
                if held.end <= held.start {
                    return;
                }
                let height = (held.end - held.start) * SLICE_HEIGHT;
                let bar = Rect {
                    x: 0.0,
                    y: size.height - height,
                    width: size.width,
                    height,
                };
                scope.draw_round_rect_at(
                    bar,
                    Brush::solid(color.with_alpha(WAITING_ALPHA)),
                    corners(key_lifts_here),
                );
                if starts_here {
                    // The end of the note that reaches the line first.
                    scope.draw_round_rect_at(
                        Rect {
                            y: size.height - HEAD_HEIGHT.min(height),
                            height: HEAD_HEIGHT.min(height),
                            ..bar
                        },
                        Brush::solid(color.lerp(&Color::WHITE, HEAD_LIGHT)),
                        CornerRadii::uniform(NOTE_CORNER),
                    );
                }
            }),
        BoxSpec::default(),
        || {},
    );
}
