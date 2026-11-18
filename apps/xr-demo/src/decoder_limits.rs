// Logs what the device's hardware decoders take, once, from a thread of its own: the largest picture
// and the highest frame rate at the sizes that a streaming app asks its host for.

use std::thread;

use cranpose::{VideoCodec, VideoDecoder};

const CODECS: [VideoCodec; 3] = [VideoCodec::H264, VideoCodec::Hevc, VideoCodec::Av1];
// The sizes of a picture: full HD, a stereo frame of two 2064 by 2208 eyes side by side, an eye of
// the Quest 3, and the frame that showed green blocks.
const SIZES: [(u32, u32); 5] = [
    (1920, 1080),
    (3840, 2160),
    (2064, 2208),
    (4096, 2208),
    (4128, 2208),
];

// Starts the thread that logs the limits.
pub(crate) fn log_in_background() {
    let spawned = thread::Builder::new()
        .name("decoder-limits".into())
        .spawn(|| {
            for codec in CODECS {
                match VideoDecoder::capabilities(codec) {
                    Ok(limits) => {
                        let rates: Vec<String> = SIZES
                            .iter()
                            .map(|&(width, height)| {
                                let rate = limits.max_frame_rate(width, height);
                                format!("{width}x{height} {rate:?}")
                            })
                            .collect();
                        log::info!(
                            "xr-demo: decoder {codec:?} {limits:?}; frame rates: {}",
                            rates.join(", ")
                        );
                    }
                    Err(error) => log::info!("xr-demo: decoder {codec:?}: {error}"),
                }
            }
        });
    if let Err(error) = spawned {
        log::warn!("xr-demo: no thread for the decoder limits: {error}");
    }
}
