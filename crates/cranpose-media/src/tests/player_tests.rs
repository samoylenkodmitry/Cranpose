use super::*;

#[test]
fn playing_before_anything_is_opened_reports_that_nothing_is_loaded() {
    let player = SoftwareMediaPlayer::new();

    assert_eq!(player.play(), Err(MediaError::NothingLoaded));
    assert_eq!(
        player.seek_to(Duration::from_secs(1)),
        Err(MediaError::NothingLoaded)
    );
}

#[test]
fn a_uri_no_platform_claims_is_refused_before_a_device_is_opened() {
    let player = SoftwareMediaPlayer::new();

    assert_eq!(
        player.prepare(&MediaItem::new("content://media/audio/1")),
        Err(MediaError::UnsupportedSource(
            "content://media/audio/1".to_string()
        ))
    );
}

#[test]
fn a_file_that_is_not_there_says_so_rather_than_failing_silently() {
    let player = SoftwareMediaPlayer::new();

    let error = player
        .prepare(&MediaItem::new("file:///nowhere/missing.mp3"))
        .expect_err("a missing file cannot be opened");

    assert!(matches!(error, MediaError::Failed(message) if message.contains("missing.mp3")));
}

#[test]
fn the_backend_reports_what_it_can_actually_do() {
    let capabilities = SoftwareMediaPlayer::new().capabilities();

    assert!(capabilities.seeking);
    assert!(capabilities.speed);
    assert!(capabilities.looping);
    assert!(capabilities.analysis);
    assert!(capabilities.probing);
    assert!(!capabilities.session);
}

#[test]
fn volume_and_speed_are_remembered_for_the_next_item() {
    let player = SoftwareMediaPlayer::new();

    player.set_volume(4.0);
    assert_eq!(*player.shared.volume.lock(), 1.0);
    player.set_volume(0.25);
    assert_eq!(*player.shared.volume.lock(), 0.25);

    assert!(player.set_speed(100.0));
    assert_eq!(*player.shared.speed.lock(), 4.0);
    assert!(player.set_speed(1.5));
    assert_eq!(*player.shared.speed.lock(), 1.5);
}

#[test]
fn analysis_is_off_until_it_is_asked_for() {
    let player = SoftwareMediaPlayer::new();
    assert!(!player.shared.analysis.is_enabled());

    assert!(player.set_analysis_enabled(true));

    assert!(player.shared.analysis.is_enabled());
}
