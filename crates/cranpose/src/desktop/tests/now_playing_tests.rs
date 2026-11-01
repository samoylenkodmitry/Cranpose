use cranpose_services::{MediaArtwork, MediaMetadata};
use objc2_foundation::NSString;
use objc2_media_player::{MPMediaItemPropertyArtwork, MPMediaItemPropertyTitle};

use super::*;

// A one-pixel red PNG.
const COVER: [u8; 70] = [
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0,
    5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

#[test]
fn the_playing_item_and_its_cover_reach_now_playing_until_it_closes() {
    let mut controls = Controls::default();
    let mut metadata = MediaMetadata::titled("Desk Song");
    metadata.artwork = Some(MediaArtwork::new(COVER.to_vec(), "image/png"));
    controls.show(Some(&NowPlaying {
        metadata,
        status: Status::Playing,
        position: Duration::from_secs(5),
    }));

    let center = unsafe { MPNowPlayingInfoCenter::defaultCenter() };
    let info = unsafe { center.nowPlayingInfo() }.expect("Now Playing shows the item");
    let title = info
        .objectForKey(unsafe { MPMediaItemPropertyTitle })
        .and_then(|title| title.downcast::<NSString>().ok())
        .map(|title| title.to_string());
    assert_eq!(title.as_deref(), Some("Desk Song"));
    assert!(
        info.objectForKey(unsafe { MPMediaItemPropertyArtwork })
            .is_some()
    );
    assert_eq!(
        unsafe { center.playbackState() },
        MPNowPlayingPlaybackState::Playing
    );

    controls.show(None);
    assert!(unsafe { center.nowPlayingInfo() }.is_none());
    assert_eq!(
        unsafe { center.playbackState() },
        MPNowPlayingPlaybackState::Stopped
    );
}
