//! The decoder and the handle that receives its pictures.

#[cfg(any(
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "windows"
))]
use super::frame_queue::Frames;
#[cfg(target_os = "android")]
use super::media_codec::Backend;
#[cfg(target_os = "windows")]
use super::media_foundation::Backend;
#[cfg(not(any(
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "windows"
)))]
use super::unavailable::{Backend, Frames};
#[cfg(any(target_os = "macos", target_os = "ios"))]
use super::video_toolbox::Backend;
use super::{
    AccessUnit, DecoderCapabilities, DecoderConfig, DecoderError, DecoderStats, FrameInfo,
    VideoCodec, VideoFrame, capabilities,
};

/// A hardware video decoder for one stream.
///
/// The app feeds it access units with [`submit`](Self::submit), from one thread at a time. A
/// thread of the decoder drains the pictures, so a picture is ready as soon as the hardware has
/// it. [`frames`](Self::frames) gives the handle that receives them.
///
/// With [`DecodeOrder::LowLatency`](super::DecodeOrder::LowLatency) the decoder holds no picture
/// back for reordering, so the stream must have no B-frames. With
/// [`DecodeOrder::Presentation`](super::DecodeOrder::Presentation) the decoder reorders B-frames
/// into presentation-timestamp order; call [`drain`](Self::drain) at end of stream to release the
/// tail of the reorder queue and [`flush`](Self::flush) before a seek. Dropping the decoder stops
/// it. It never waits for the app's threads.
pub struct VideoDecoder {
    backend: Backend,
}

impl VideoDecoder {
    /// Makes a decoder for `config`. The decoder is unavailable on a platform without a hardware
    /// decoder that Cranpose drives.
    pub fn new(config: DecoderConfig) -> Result<Self, DecoderError> {
        Ok(Self {
            backend: Backend::new(config)?,
        })
    }

    /// The limits of the hardware decoder that [`new`](Self::new) opens for `codec`: its largest
    /// picture and bit rate, and through [`DecoderCapabilities::max_frame_rate`] its frame rate
    /// at a size. Android reads them from the device's media codec list. VideoToolbox on macOS and
    /// iOS tells only whether the codec decodes on dedicated hardware, and leaves the sizes and
    /// rates `None`. The call costs a query of the platform, so an app makes it once for each
    /// codec. It reports [`DecoderError::Unavailable`] where the platform gives no limits and
    /// [`DecoderError::NoDecoder`] for a codec that the device cannot decode.
    pub fn capabilities(codec: VideoCodec) -> Result<DecoderCapabilities, DecoderError> {
        capabilities::of(codec)
    }

    /// Gives the decoder one access unit. It waits up to
    /// [`DecoderConfig::input_wait`] for the decoder to take it, and reports
    /// [`DecoderError::Full`] after that, which [`DecoderStats::refused`] counts. The unit's data
    /// is copied, so the app may reuse it. The wait blocks the calling thread, so the app calls
    /// this from the thread that feeds the decoder and never from its frame loop.
    pub fn submit(&mut self, unit: AccessUnit<'_>) -> Result<(), DecoderError> {
        self.backend.submit(unit)
    }

    /// Signals end of stream so pictures held for B-frame reordering finish decoding and enter
    /// [`DecodedFrames`] in presentation order. Resets timestamp admission so a subsequent stream
    /// segment may start at any presentation timestamp. Call from the feeding thread, never from
    /// the frame loop.
    pub fn drain(&mut self) -> Result<(), DecoderError> {
        self.backend.drain()
    }

    /// Discards pending input, reordered pictures, and queued output frames, resetting timestamp
    /// admission so playback can resume cleanly at a new keyframe (for example after a seek).
    pub fn flush(&mut self) -> Result<(), DecoderError> {
        self.backend.flush()
    }

    /// The handle that receives the decoded pictures. It stays usable after the decoder drops,
    /// and returns the pictures that were left.
    pub fn frames(&self) -> DecodedFrames {
        DecodedFrames {
            frames: self.backend.frames(),
        }
    }
}

/// The decoded pictures of a [`VideoDecoder`]. A clone receives from the same decoder, on any
/// thread. Two handles are equal when they receive from the same decoder.
#[derive(Clone, PartialEq)]
pub struct DecodedFrames {
    frames: Frames,
}

impl DecodedFrames {
    /// The oldest picture that the decoder has output, if the output is
    /// [`DecoderOutput::Frames`](super::DecoderOutput::Frames).
    pub fn next_frame(&self) -> Option<VideoFrame> {
        self.frames.next_frame()
    }

    /// The newest picture, if the output is [`DecoderOutput::Frames`](super::DecoderOutput::Frames).
    /// It drops the older pictures.
    pub fn latest_frame(&self) -> Option<VideoFrame> {
        self.frames.latest_frame()
    }

    /// Waits until a picture is ready for [`next_frame`](Self::next_frame) or
    /// [`latest_frame`](Self::latest_frame), and returns at once if one is. The decoder's thread
    /// wakes the task, so an app draws a picture in the frame that follows its output with no
    /// polling. One task at a time waits: a second wait takes the place of the first.
    pub async fn wait_for_frame(&self) {
        std::future::poll_fn(|context| self.frames.poll_ready(context)).await;
    }

    /// The report of the oldest picture that the decoder sent to the surface, if the output is
    /// [`DecoderOutput::Surface`](super::DecoderOutput::Surface). The decoder keeps the latest 32
    /// reports.
    pub fn next_report(&self) -> Option<FrameInfo> {
        self.frames.next_report()
    }

    /// What the decoder has done since it started.
    pub fn stats(&self) -> DecoderStats {
        self.frames.stats()
    }
}
