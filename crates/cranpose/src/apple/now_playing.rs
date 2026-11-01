#![expect(unsafe_code, reason = "Objective-C messages to MediaPlayer")]
//! The system's Now Playing information and the remote commands that control
//! the playing item, which iOS and macOS share.

use std::time::Duration;

use block2::RcBlock;
use cranpose_services::{MediaCommand, MediaMetadata, publish_media_command};
use objc2::runtime::AnyObject;
use objc2_foundation::{NSDictionary, NSNumber, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemArtwork, MPMediaItemPropertyAlbumTitle,
    MPMediaItemPropertyArtist, MPMediaItemPropertyArtwork, MPMediaItemPropertyPlaybackDuration,
    MPMediaItemPropertyTitle, MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyElapsedPlaybackTime,
    MPNowPlayingInfoPropertyPlaybackRate, MPRemoteCommandCenter, MPRemoteCommandEvent,
    MPRemoteCommandHandlerStatus,
};

pub(crate) fn install_remote_commands() {
    unsafe {
        let center = MPRemoteCommandCenter::sharedCommandCenter();
        for (command, action) in [
            (center.playCommand(), MediaCommand::Play),
            (center.pauseCommand(), MediaCommand::Pause),
            (
                center.togglePlayPauseCommand(),
                MediaCommand::TogglePlayPause,
            ),
            (center.stopCommand(), MediaCommand::Stop),
            (center.nextTrackCommand(), MediaCommand::Next),
            (center.previousTrackCommand(), MediaCommand::Previous),
        ] {
            let handler = RcBlock::new(
                move |_event: std::ptr::NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                    publish_media_command(action);
                    MPRemoteCommandHandlerStatus::Success
                },
            );
            command.setEnabled(true);
            let _ = command.addTargetWithHandler(&handler);
        }

        let seek = center.changePlaybackPositionCommand();
        let handler = RcBlock::new(
            move |event: std::ptr::NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                let event = event.as_ref();
                let Some(event) = event.downcast_ref::<MPChangePlaybackPositionCommandEvent>()
                else {
                    return MPRemoteCommandHandlerStatus::CommandFailed;
                };
                let seconds = event.positionTime();
                if !seconds.is_finite() || seconds < 0.0 {
                    return MPRemoteCommandHandlerStatus::CommandFailed;
                }
                publish_media_command(MediaCommand::SeekTo(Duration::from_secs_f64(seconds)));
                MPRemoteCommandHandlerStatus::Success
            },
        );
        seek.setEnabled(true);
        let _ = seek.addTargetWithHandler(&handler);
    }
}

// Runs on the main thread.
pub(crate) fn publish(
    metadata: &MediaMetadata,
    position: Duration,
    rate: f32,
    artwork: Option<&MPMediaItemArtwork>,
) {
    unsafe {
        let title = NSString::from_str(&metadata.title);
        let artist = NSString::from_str(&metadata.artist);
        let album = NSString::from_str(&metadata.album);
        let duration = NSNumber::new_f64(
            metadata
                .duration
                .map_or(0.0, |duration| duration.as_secs_f64()),
        );
        let elapsed = NSNumber::new_f64(position.as_secs_f64());
        let rate = NSNumber::new_f32(rate);

        let mut keys: Vec<&NSString> = vec![
            MPMediaItemPropertyTitle,
            MPMediaItemPropertyArtist,
            MPMediaItemPropertyAlbumTitle,
            MPMediaItemPropertyPlaybackDuration,
            MPNowPlayingInfoPropertyElapsedPlaybackTime,
            MPNowPlayingInfoPropertyPlaybackRate,
        ];
        let mut values: Vec<&AnyObject> = vec![
            title.as_ref(),
            artist.as_ref(),
            album.as_ref(),
            duration.as_ref(),
            elapsed.as_ref(),
            rate.as_ref(),
        ];
        if let Some(artwork) = artwork {
            keys.push(MPMediaItemPropertyArtwork);
            values.push(artwork.as_ref());
        }
        let info = NSDictionary::from_slices(&keys, &values);
        MPNowPlayingInfoCenter::defaultCenter().setNowPlayingInfo(Some(&info));
    }
}
