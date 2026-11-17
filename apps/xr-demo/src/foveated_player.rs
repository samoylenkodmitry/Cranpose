// Plays a synthetic foveated clip through a hardware decoder in frames mode, for a streamed scene
// that unwarps it. `examples/foveated_clip.rs` made the clip: a test picture with a grid of equal
// squares, which ALVR's foveated encoding compressed at the edges, side by side for both eyes.
// After the unwarp the grid shows straight and uniform in both eyes, and the red square lies at the
// top left.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use cranpose::{
    AccessUnit, CropRect, DecodedFrames, DecoderConfig, DecoderError, DecoderOutput, VideoCodec,
    VideoDecoder,
};
use cranpose_xr::{FieldOfView, FoveatedEncoding, Pose, StreamedView, StreamedViews};

use crate::clip_player::{DEPTH, access_units, target_display_time};

const CLIP: &[u8] = include_bytes!("../assets/foveated_test.h264");
// The clip's size in pixels: two encoded eyes side by side
const SIZE: (u32, u32) = (768, 384);
const RATE: u32 = 30;
// The size of an encoded eye and of the full view that it unwarps to
const EYE: u32 = 384;
const FULL: (u32, u32) = (640, 720);

// The values that the clip's encoder aligned: the output of `examples/foveated_clip.rs`.
const LEFT: FoveatedEncoding = FoveatedEncoding {
    full_size: FULL,
    center_size: [0.45, 0.388_888_9],
    center_shift: [0.409_090_9, 0.113_636_37],
    edge_ratio: [4.0, 5.0],
    eye_size_ratio: [0.979_166_7, 0.958_333_43],
    mirrored: false,
};

// A player of the clip. It stops when dropped.
pub(crate) struct FoveatedPlayer {
    stop: Arc<AtomicBool>,
}

impl FoveatedPlayer {
    // Makes the decoder and starts the thread that feeds it. Returns the player and the decoder's
    // frames.
    pub(crate) fn start() -> Result<(Self, DecodedFrames), DecoderError> {
        let mut decoder = VideoDecoder::new(DecoderConfig::new(
            VideoCodec::H264,
            SIZE.0,
            SIZE.1,
            DecoderOutput::Frames { depth: DEPTH },
        ))?;
        let frames = decoder.frames();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let spawned = thread::Builder::new()
            .name("foveated-player".into())
            .spawn(move || play(&mut decoder, &flag));
        if let Err(error) = spawned {
            log::error!("xr demo: no foveated clip thread: {error}");
        }
        Ok((Self { stop }, frames))
    }
}

impl Drop for FoveatedPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn play(decoder: &mut VideoDecoder, stop: &AtomicBool) {
    let units = access_units(CLIP);
    let step = Duration::from_secs(1) / RATE;
    let started = Instant::now();
    let mut index = 0_u64;
    while !stop.load(Ordering::Relaxed) {
        for unit in &units {
            let timestamp = step * u32::try_from(index).unwrap_or(u32::MAX);
            let access_unit = AccessUnit {
                data: unit,
                timestamp,
                metadata: index,
            };
            if let Err(error) = decoder.submit(access_unit) {
                log::warn!("xr demo: foveated picture {index} dropped: {error}");
            }
            index += 1;
            thread::sleep(
                (started + step * u32::try_from(index).unwrap_or(u32::MAX))
                    .saturating_duration_since(Instant::now()),
            );
            if stop.load(Ordering::Relaxed) {
                return;
            }
        }
    }
}

// The views of a frame of the clip: both eyes look 25 degrees up from level, above the panels, with
// the shape of the full view that the clip holds. The right eye's picture is mirrored.
pub(crate) fn views(index: u64, base: Duration) -> StreamedViews {
    const UP: f32 = 25.0 * std::f32::consts::PI / 180.0;
    let (sin, cos) = (UP / 2.0).sin_cos();
    let eye = |x: f32, left: u32, mirrored: bool| StreamedView {
        pose: Pose {
            position: [x, 0.0, 0.0],
            orientation: [sin, 0.0, 0.0, cos],
        },
        fov: FieldOfView::symmetric(0.7176, 0.8),
        rect: CropRect {
            x: left,
            y: 0,
            width: EYE,
            height: EYE,
        },
        foveation: Some(FoveatedEncoding { mirrored, ..LEFT }),
    };
    StreamedViews {
        left: eye(-0.032, 0, false),
        right: eye(0.032, EYE, true),
        target_display_time: Some(target_display_time(base, index)),
    }
}
