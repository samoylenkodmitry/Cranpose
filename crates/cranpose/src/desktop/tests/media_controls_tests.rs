use std::time::{Duration, Instant};

use cranpose_services::{MediaError, MediaMetadata, PlaybackState};

use super::*;

fn playing(title: &str, seconds: u64) -> NowPlaying {
    NowPlaying {
        metadata: MediaMetadata::titled(title),
        status: Status::Playing,
        position: Duration::from_secs(seconds),
    }
}

#[test]
fn nothing_open_and_nothing_shown_needs_nothing() {
    assert_eq!(change(None, None, Instant::now(), None), Change::Nothing);
}

#[test]
fn an_item_opening_or_closing_changes_everything() {
    let at = Instant::now();
    let shown = (playing("Song", 0), at);
    assert_eq!(
        change(None, Some(&playing("Song", 0)), at, None),
        Change::Everything
    );
    assert_eq!(change(Some(&shown), None, at, None), Change::Everything);
}

#[test]
fn another_item_or_state_changes_everything() {
    let at = Instant::now();
    let shown = (playing("Song", 10), at);
    let paused = NowPlaying {
        status: Status::Paused,
        ..playing("Song", 10)
    };
    assert_eq!(
        change(Some(&shown), Some(&playing("Other", 10)), at, None),
        Change::Everything
    );
    assert_eq!(
        change(Some(&shown), Some(&paused), at, None),
        Change::Everything
    );
}

#[test]
fn playing_on_at_the_counted_pace_needs_nothing() {
    let since = Instant::now();
    let shown = (playing("Song", 10), since);
    let later = since + Duration::from_secs(3);
    assert_eq!(
        change(Some(&shown), Some(&playing("Song", 13)), later, None),
        Change::Nothing
    );
}

#[test]
fn a_seek_moves_the_position() {
    let since = Instant::now();
    let shown = (playing("Song", 10), since);
    let later = since + Duration::from_secs(3);
    assert_eq!(
        change(Some(&shown), Some(&playing("Song", 60)), later, None),
        Change::Position
    );
}

#[test]
fn a_seek_while_paused_moves_the_position() {
    let since = Instant::now();
    let paused = |seconds| NowPlaying {
        status: Status::Paused,
        ..playing("Song", seconds)
    };
    let shown = (paused(10), since);
    let later = since + Duration::from_secs(30);
    assert_eq!(
        change(Some(&shown), Some(&paused(10)), later, None),
        Change::Nothing
    );
    assert_eq!(
        change(Some(&shown), Some(&paused(40)), later, None),
        Change::Position
    );
}

#[test]
fn a_platform_that_does_not_count_on_is_told_the_position_again() {
    let since = Instant::now();
    let shown = (playing("Song", 10), since);
    let refresh = Some(Duration::from_secs(5));
    let soon = since + Duration::from_secs(2);
    let later = since + Duration::from_secs(5);
    assert_eq!(
        change(Some(&shown), Some(&playing("Song", 12)), soon, refresh),
        Change::Nothing
    );
    assert_eq!(
        change(Some(&shown), Some(&playing("Song", 15)), later, refresh),
        Change::Position
    );
}

#[test]
fn loading_shows_as_paused_and_an_end_or_failure_as_stopped() {
    assert_eq!(status_of(&PlaybackState::Loading), Status::Paused);
    assert_eq!(status_of(&PlaybackState::Ended), Status::Stopped);
    assert_eq!(
        status_of(&PlaybackState::Failed(MediaError::Unsupported)),
        Status::Stopped
    );
}
