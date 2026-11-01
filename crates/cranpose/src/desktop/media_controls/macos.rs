#![expect(unsafe_code, reason = "Objective-C messages to MediaPlayer and AppKit")]

use std::{ptr::NonNull, sync::Once, time::Duration};

use block2::RcBlock;
use cranpose_services::MediaArtwork;
use objc2::{AllocAnyThread, rc::Retained};
use objc2_app_kit::NSImage;
use objc2_core_foundation::CGSize;
use objc2_foundation::NSData;
use objc2_media_player::{MPMediaItemArtwork, MPNowPlayingInfoCenter, MPNowPlayingPlaybackState};
use winit::window::Window;

use super::{NowPlaying, Status};

// Now Playing counts the position on from the playback rate it is given.
pub(super) const POSITION_REFRESH: Option<Duration> = None;

#[derive(Default)]
pub(super) struct Controls {
    // The cover last shown and the artwork made from it, decoded once a cover.
    artwork: Option<(MediaArtwork, Option<Retained<MPMediaItemArtwork>>)>,
}

impl Controls {
    pub(super) fn new(_window: &dyn Window) -> Option<Self> {
        static COMMANDS: Once = Once::new();
        COMMANDS.call_once(crate::apple::now_playing::install_remote_commands);
        Some(Self::default())
    }

    pub(super) fn show(&mut self, now: Option<&NowPlaying>) {
        match now {
            Some(now) => self.show_position(now),
            None => {
                self.artwork = None;
                unsafe {
                    let center = MPNowPlayingInfoCenter::defaultCenter();
                    center.setNowPlayingInfo(None);
                    center.setPlaybackState(MPNowPlayingPlaybackState::Stopped);
                }
            }
        }
    }

    pub(super) fn show_position(&mut self, now: &NowPlaying) {
        let artwork = self.artwork(now.metadata.artwork.as_ref());
        let rate = if now.status == Status::Playing {
            1.0
        } else {
            0.0
        };
        crate::apple::now_playing::publish(&now.metadata, now.position, rate, artwork.as_deref());
        let state = match now.status {
            Status::Playing => MPNowPlayingPlaybackState::Playing,
            Status::Paused => MPNowPlayingPlaybackState::Paused,
            Status::Stopped => MPNowPlayingPlaybackState::Stopped,
        };
        unsafe { MPNowPlayingInfoCenter::defaultCenter().setPlaybackState(state) };
    }

    fn artwork(&mut self, source: Option<&MediaArtwork>) -> Option<Retained<MPMediaItemArtwork>> {
        let Some(source) = source else {
            self.artwork = None;
            return None;
        };
        if self
            .artwork
            .as_ref()
            .is_none_or(|(shown, _)| shown != source)
        {
            self.artwork = Some((source.clone(), artwork_of(source)));
        }
        self.artwork
            .as_ref()
            .and_then(|(_, artwork)| artwork.clone())
    }
}

fn artwork_of(source: &MediaArtwork) -> Option<Retained<MPMediaItemArtwork>> {
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(&source.bytes))?;
    let size = image.size();
    let handler = RcBlock::new(move |_requested: CGSize| NonNull::from(&*image));
    Some(unsafe {
        MPMediaItemArtwork::initWithBoundsSize_requestHandler(
            MPMediaItemArtwork::alloc(),
            size,
            &handler,
        )
    })
}

#[cfg(test)]
#[path = "../tests/now_playing_tests.rs"]
mod tests;
