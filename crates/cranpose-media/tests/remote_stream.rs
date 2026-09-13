use std::{thread::sleep, time::Duration};

use cranpose_media::SoftwareMediaPlayer;
use cranpose_services::{MediaItem, MediaPlayer, PlaybackState, playback_progress, playback_state};

const URL: &str = "CRANPOSE_REMOTE_AUDIO_URL";

fn url() -> String {
    std::env::var(URL).unwrap_or_else(|_| panic!("set {URL} to an audio URL to run this"))
}

#[test]
#[ignore = "network and an output device: run with CRANPOSE_REMOTE_AUDIO_URL set"]
fn a_remote_track_plays_and_its_position_advances() {
    let item = MediaItem::new(url());
    let player = SoftwareMediaPlayer::new();

    let probed = player.probe_duration(&item);
    player.prepare(&item).expect("the remote item opens");
    player.play().expect("the remote item plays");

    sleep(Duration::from_secs(3));
    let early = playback_progress();
    sleep(Duration::from_secs(3));
    let later = playback_progress();
    let state = playback_state();
    player.stop();

    assert_eq!(state, PlaybackState::Playing, "{early:?} then {later:?}");
    assert!(
        probed.is_some_and(|duration| duration > Duration::from_secs(1)),
        "the probe read no duration from the stream: {probed:?}"
    );
    assert_eq!(later.duration, probed, "the duration changed mid-item");
    assert!(
        early.position > Duration::ZERO,
        "nothing had played after three seconds"
    );
    assert!(
        later.position > early.position + Duration::from_secs(2),
        "the position went from {:?} to {:?} over three seconds",
        early.position,
        later.position
    );
}

#[test]
#[ignore = "network and an output device: run with CRANPOSE_REMOTE_AUDIO_URL set"]
fn a_remote_track_seeks_without_reading_what_it_skipped() {
    let item = MediaItem::new(url());
    let player = SoftwareMediaPlayer::new();

    player.prepare(&item).expect("the remote item opens");
    player.play().expect("the remote item plays");
    sleep(Duration::from_secs(1));
    player
        .seek_to(Duration::from_secs(120))
        .expect("a ranged server seeks");
    sleep(Duration::from_secs(3));
    let after = playback_progress();
    let state = playback_state();
    player.stop();

    assert_eq!(state, PlaybackState::Playing);
    assert!(
        after.position >= Duration::from_secs(121),
        "the seek left the position at {:?}",
        after.position
    );
}
