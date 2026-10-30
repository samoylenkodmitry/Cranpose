#![expect(
    unsafe_code,
    reason = "Objective-C artwork handler retains its UIImage"
)]

use std::ptr::NonNull;

use block2::RcBlock;
use cranpose_services::MediaArtwork;
use objc2::{AllocAnyThread, msg_send, rc::Retained};
use objc2_foundation::{NSData, NSSize};
use objc2_media_player::MPMediaItemArtwork;
use objc2_ui_kit::UIImage;

#[derive(Default)]
pub(crate) struct CachedArtwork {
    source: Option<MediaArtwork>,
    image: Option<Retained<MPMediaItemArtwork>>,
}

pub(super) fn image(source: Option<&MediaArtwork>) -> Option<Retained<MPMediaItemArtwork>> {
    // This runs on the main queue. Decode each new cover once; position updates
    // reuse the same artwork object, and an absent cover clears the old one.
    crate::thread_state::with(|state| {
        let mut slot = state.ios.media_artwork.borrow_mut();
        let cache = slot.get_or_insert_with(CachedArtwork::default);
        if cache.source.as_ref() != source {
            cache.image = source.and_then(make_image);
            cache.source = source.cloned();
        }
        cache.image.clone()
    })
}

fn make_image(source: &MediaArtwork) -> Option<Retained<MPMediaItemArtwork>> {
    let data = NSData::with_bytes(&source.bytes);
    let image = UIImage::imageWithData(&data)?;
    let size = unsafe { image.size() };
    let handler = RcBlock::new(move |_requested: NSSize| NonNull::from(&*image));
    // objc2-media-player currently exposes the NSImage form only. iOS accepts
    // the same selector with a UIImage-returning block.
    Some(unsafe {
        msg_send![MPMediaItemArtwork::alloc(), initWithBoundsSize: size, requestHandler: &*handler]
    })
}
