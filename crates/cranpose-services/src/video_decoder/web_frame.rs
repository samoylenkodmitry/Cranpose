//! Web zero-copy video frame backing storage: a WebCodecs `VideoFrame` or live `HTMLVideoElement`.
#![expect(
    unsafe_code,
    reason = "single-threaded wasm32 browser JS handles implement Send/Sync without atomics"
)]

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

// Count of decoded pictures currently held by the queue or the application.
pub(super) type LivePictures = Arc<AtomicUsize>;

/// Zero-copy browser video source held by a Web [`VideoFrame`](super::VideoFrame).
#[derive(Clone, Copy, Debug)]
pub enum WebVideoSource<'a> {
    /// A WebCodecs `VideoFrame` decoded on the GPU by the browser's video decoder.
    VideoFrame(&'a web_sys::VideoFrame),
    /// A live `HTMLVideoElement` whose current decoded video frame is copied directly on the GPU.
    HtmlVideoElement(&'a web_sys::HtmlVideoElement),
}

enum Storage {
    VideoFrame(web_sys::VideoFrame),
    HtmlVideoElement(web_sys::HtmlVideoElement),
}

pub(super) struct Image {
    storage: Storage,
    live: Option<LivePictures>,
}

// SAFETY: browser wasm32-unknown-unknown without atomics executes exclusively on one JS thread,
// matching `wgpu`'s `fragile-send-sync-non-atomic-wasm` invariant.
#[cfg(not(target_feature = "atomics"))]
unsafe impl Send for Image {}
// SAFETY: as for Send.
#[cfg(not(target_feature = "atomics"))]
unsafe impl Sync for Image {}

impl Image {
    pub(super) fn from_video_frame(
        frame: web_sys::VideoFrame,
        live: Option<&LivePictures>,
    ) -> Self {
        if let Some(counter) = live {
            counter.fetch_add(1, Ordering::Relaxed);
        }
        Self {
            storage: Storage::VideoFrame(frame),
            live: live.map(Arc::clone),
        }
    }

    pub(super) fn from_html_video_element(element: web_sys::HtmlVideoElement) -> Self {
        Self {
            storage: Storage::HtmlVideoElement(element),
            live: None,
        }
    }

    pub(super) fn source(&self) -> WebVideoSource<'_> {
        match &self.storage {
            Storage::VideoFrame(frame) => WebVideoSource::VideoFrame(frame),
            Storage::HtmlVideoElement(element) => WebVideoSource::HtmlVideoElement(element),
        }
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        if let Storage::VideoFrame(frame) = &self.storage {
            frame.close();
        }
        if let Some(counter) = &self.live {
            counter.fetch_sub(1, Ordering::Relaxed);
        }
    }
}
