//! WebCodecs hardware video decoder for browser targets (`wasm32-unknown-unknown`).
//!
//! Binds directly to the browser's `VideoDecoder` and `EncodedVideoChunk` classes through
//! `wasm-bindgen` so standard builds require no `--cfg=web_sys_unstable_apis`. Outputs zero-copy
//! `web_sys::VideoFrame` pictures into [`DecodedFrames`](super::DecodedFrames) without blocking
//! the UI thread.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use parking_lot::Mutex;
use wasm_bindgen::{JsValue, closure::Closure, prelude::wasm_bindgen};
use web_sys::{VideoColorPrimaries, VideoTransferCharacteristics};
use web_time::Instant;

use super::{
    AccessUnit, ColorPrimaries, ColorRange, ColorTransfer, CropRect, DecodeOrder, DecoderConfig,
    DecoderError, DecoderOutput, VideoCodec, VideoColorInfo, VideoFrame, VideoRotation,
    admission::Admission,
    annex_b::AccessUnitNals,
    frame_queue::{FrameQueue, Frames},
    pending_units::{PendingUnit, PendingUnits},
    sps::{StreamMetadata, webcodecs_codec_string},
    web_codecs_capabilities::webcodecs_available,
    web_frame::Image,
};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = "VideoDecoder")]
    type JsVideoDecoder;

    #[wasm_bindgen(catch, constructor, js_class = "VideoDecoder")]
    fn new(init: &js_sys::Object) -> Result<JsVideoDecoder, JsValue>;

    #[wasm_bindgen(catch, method, js_class = "VideoDecoder")]
    fn configure(this: &JsVideoDecoder, config: &js_sys::Object) -> Result<(), JsValue>;

    #[wasm_bindgen(catch, method, js_class = "VideoDecoder")]
    fn decode(this: &JsVideoDecoder, chunk: &JsEncodedVideoChunk) -> Result<(), JsValue>;

    #[wasm_bindgen(method, js_class = "VideoDecoder")]
    fn flush(this: &JsVideoDecoder) -> js_sys::Promise;

    #[wasm_bindgen(catch, method, js_class = "VideoDecoder")]
    fn reset(this: &JsVideoDecoder) -> Result<(), JsValue>;

    #[wasm_bindgen(catch, method, js_class = "VideoDecoder")]
    fn close(this: &JsVideoDecoder) -> Result<(), JsValue>;

    #[wasm_bindgen(method, getter, js_class = "VideoDecoder")]
    fn state(this: &JsVideoDecoder) -> String;

    #[wasm_bindgen(method, getter, js_class = "VideoDecoder", js_name = "decodeQueueSize")]
    fn decode_queue_size(this: &JsVideoDecoder) -> u32;

    #[wasm_bindgen(js_name = "EncodedVideoChunk")]
    type JsEncodedVideoChunk;

    #[wasm_bindgen(catch, constructor, js_class = "EncodedVideoChunk")]
    fn new(init: &js_sys::Object) -> Result<JsEncodedVideoChunk, JsValue>;
}

const MAX_DECODE_QUEUE_SIZE: u32 = 16;

struct Shared {
    queue: Arc<FrameQueue>,
    pending: Mutex<PendingUnits>,
    live: Arc<AtomicUsize>,
    depth: usize,
    order: DecodeOrder,
    rotation: VideoRotation,
    config_color: Option<VideoColorInfo>,
    stream_meta: Mutex<StreamMetadata>,
}

pub(super) struct Backend {
    decoder: JsVideoDecoder,
    _on_output: Closure<dyn FnMut(web_sys::VideoFrame)>,
    on_error: Closure<dyn FnMut(JsValue)>,
    shared: Arc<Shared>,
    codec: VideoCodec,
    width: u32,
    height: u32,
    nals: AccessUnitNals,
    admission: Admission,
    configured_codec: Option<String>,
}

impl Backend {
    pub(super) fn new(config: DecoderConfig) -> Result<Self, DecoderError> {
        if !webcodecs_available() {
            return Err(DecoderError::Unavailable);
        }
        let DecoderConfig {
            codec,
            width,
            height,
            output,
            order,
            rotation,
            color,
            ..
        } = config;
        let DecoderOutput::Frames { depth } = output else {
            return Err(DecoderError::Unavailable);
        };
        let depth = depth.max(2);
        let shared = Arc::new(Shared {
            queue: Arc::new(FrameQueue::new(depth, 0)),
            pending: Mutex::new(PendingUnits::new()),
            live: Arc::new(AtomicUsize::new(0)),
            depth,
            order,
            rotation,
            config_color: color,
            stream_meta: Mutex::new(StreamMetadata::default()),
        });
        let output_shared = Arc::clone(&shared);
        let on_output = Closure::wrap(Box::new(move |frame: web_sys::VideoFrame| {
            handle_output(&output_shared, frame);
        }) as Box<dyn FnMut(web_sys::VideoFrame)>);
        let on_error = Closure::wrap(Box::new(move |err: JsValue| {
            log::warn!("webcodecs video decoder error: {err:?}");
        }) as Box<dyn FnMut(JsValue)>);

        let init = js_sys::Object::new();
        set_prop(&init, "output", on_output.as_ref());
        set_prop(&init, "error", on_error.as_ref());
        let decoder = JsVideoDecoder::new(&init)
            .map_err(|err| DecoderError::Configure(format!("VideoDecoder::new: {err:?}")))?;

        let mut backend = Self {
            decoder,
            _on_output: on_output,
            on_error,
            shared,
            codec,
            width,
            height,
            nals: AccessUnitNals::default(),
            admission: Admission::new((width as usize) * (height as usize), order),
            configured_codec: None,
        };
        if codec == VideoCodec::Av1 {
            backend.configure_decoder(None)?;
        }
        Ok(backend)
    }

    pub(super) fn submit(&mut self, unit: AccessUnit<'_>) -> Result<(), DecoderError> {
        let pts_us = self.admission.check(&unit)?;
        if self.decoder.state() == "closed" {
            return Err(DecoderError::Failed("WebCodecs decoder closed".into()));
        }
        if self.decoder.decode_queue_size() >= MAX_DECODE_QUEUE_SIZE {
            self.shared.queue.stats.refused();
            return Err(DecoderError::Full);
        }
        let is_key = if self.codec == VideoCodec::Av1 {
            self.configured_codec.is_none()
        } else {
            self.nals.parse(self.codec, unit.data);
            if let Some(sps) = self.nals.sequence_parameter_set(unit.data) {
                if let Some(meta) = StreamMetadata::parse_sps(self.codec, sps) {
                    *self.shared.stream_meta.lock() = meta;
                }
                let codec_str = webcodecs_codec_string(self.codec, Some(sps));
                if self.configured_codec.as_deref() != Some(codec_str.as_str()) {
                    self.configure_decoder(Some(&codec_str))?;
                }
            }
            self.nals.has_keyframe()
        };
        if self.configured_codec.is_none() {
            return Err(DecoderError::NeedsKeyframe);
        }
        let chunk_init = js_sys::Object::new();
        set_prop(
            &chunk_init,
            "type",
            &JsValue::from_str(if is_key { "key" } else { "delta" }),
        );
        set_prop(&chunk_init, "timestamp", &JsValue::from_f64(pts_us as f64));
        let bytes = js_sys::Uint8Array::from(unit.data);
        set_prop(&chunk_init, "data", bytes.as_ref());
        let chunk = JsEncodedVideoChunk::new(&chunk_init)
            .map_err(|err| DecoderError::Failed(format!("EncodedVideoChunk: {err:?}")))?;

        self.shared.pending.lock().push(PendingUnit {
            pts_us,
            timestamp: unit.timestamp,
            metadata: unit.metadata,
            submitted: Instant::now(),
        });
        if let Err(err) = self.decoder.decode(&chunk) {
            self.shared.pending.lock().remove(pts_us);
            return Err(DecoderError::Failed(format!(
                "VideoDecoder::decode: {err:?}"
            )));
        }
        self.admission.accept(pts_us);
        self.shared.queue.stats.submitted();
        Ok(())
    }

    pub(super) fn drain(&mut self) -> Result<(), DecoderError> {
        if self.decoder.state() == "configured" {
            let _ = self.decoder.flush().catch(&self.on_error);
        }
        self.admission.reset();
        Ok(())
    }

    pub(super) fn flush(&mut self) -> Result<(), DecoderError> {
        if self.decoder.state() != "closed" {
            let _ = self.decoder.reset();
        }
        self.configured_codec = None;
        self.shared.pending.lock().clear();
        self.shared.queue.frames.lock().clear();
        self.admission.reset();
        if self.codec == VideoCodec::Av1 {
            self.configure_decoder(None)?;
        }
        Ok(())
    }

    pub(super) fn frames(&self) -> Frames {
        Frames(Arc::clone(&self.shared.queue))
    }

    fn configure_decoder(&mut self, explicit_codec: Option<&str>) -> Result<(), DecoderError> {
        let codec_string =
            explicit_codec.map_or_else(|| webcodecs_codec_string(self.codec, None), String::from);
        let cfg = js_sys::Object::new();
        set_prop(&cfg, "codec", &JsValue::from_str(&codec_string));
        set_prop(
            &cfg,
            "codedWidth",
            &JsValue::from_f64(f64::from(self.width)),
        );
        set_prop(
            &cfg,
            "codedHeight",
            &JsValue::from_f64(f64::from(self.height)),
        );
        set_prop(
            &cfg,
            "hardwareAcceleration",
            &JsValue::from_str("prefer-hardware"),
        );
        set_prop(
            &cfg,
            "optimizeForLatency",
            &JsValue::from_bool(self.shared.order == DecodeOrder::LowLatency),
        );
        if self.codec == VideoCodec::H264 {
            let avc = js_sys::Object::new();
            set_prop(&avc, "format", &JsValue::from_str("annexb"));
            set_prop(&cfg, "avc", avc.as_ref());
        } else if self.codec == VideoCodec::Hevc {
            let hevc = js_sys::Object::new();
            set_prop(&hevc, "format", &JsValue::from_str("annexb"));
            set_prop(&cfg, "hevc", hevc.as_ref());
        }
        self.decoder
            .configure(&cfg)
            .map_err(|err| DecoderError::Configure(format!("VideoDecoder::configure: {err:?}")))?;
        self.configured_codec = Some(codec_string);
        Ok(())
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        if self.decoder.state() != "closed" {
            let _ = self.decoder.close();
        }
    }
}

fn handle_output(shared: &Shared, frame: web_sys::VideoFrame) {
    let coded_w = frame.coded_width();
    let coded_h = frame.coded_height();
    if coded_w == 0 || coded_h == 0 {
        frame.close();
        return;
    }
    let crop = visible_crop(&frame, (coded_w, coded_h));
    let color = resolve_frame_color(shared, &frame, crop.height);
    let pts_f = frame.timestamp().max(0.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "WebCodecs microsecond timestamps are non-negative integers within u64 range"
    )]
    let pts_us = pts_f.round() as u64;
    let unit = match shared.order {
        DecodeOrder::LowLatency => shared.pending.lock().take(pts_us),
        DecodeOrder::Presentation => shared.pending.lock().remove(pts_us),
    };
    let info = PendingUnit::into_frame_info(unit, pts_us);
    let image = Image::from_video_frame(frame, Some(&shared.live));
    let video_frame = VideoFrame::new(
        info,
        (coded_w, coded_h),
        crop,
        color,
        shared.rotation,
        image,
    );
    let mut frames = shared.queue.frames.lock();
    while shared.live.load(Ordering::Relaxed) > shared.depth {
        shared.queue.stats.dropped();
        if frames.pop_front().is_none() {
            return;
        }
    }
    frames.push_back(video_frame);
    drop(frames);
    shared.queue.wake();
    shared.queue.stats.decoded(info.decode_time);
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "DOMRect coordinates are clamped to non-negative frame dimensions before u32 conversion"
)]
fn visible_crop(frame: &web_sys::VideoFrame, (coded_w, coded_h): (u32, u32)) -> CropRect {
    let Some(rect) = frame.visible_rect() else {
        return CropRect {
            x: 0,
            y: 0,
            width: coded_w,
            height: coded_h,
        };
    };
    let x = (rect.x().max(0.0).floor() as u32).min(coded_w.saturating_sub(1));
    let y = (rect.y().max(0.0).floor() as u32).min(coded_h.saturating_sub(1));
    let w = (rect.width().max(1.0).round() as u32).min(coded_w.saturating_sub(x));
    let h = (rect.height().max(1.0).round() as u32).min(coded_h.saturating_sub(y));
    CropRect {
        x,
        y,
        width: w.max(1),
        height: h.max(1),
    }
}

fn resolve_frame_color(
    shared: &Shared,
    frame: &web_sys::VideoFrame,
    height: u32,
) -> VideoColorInfo {
    let cs = frame.color_space();
    let primaries = cs.primaries().and_then(|p| match p {
        VideoColorPrimaries::Bt709 => Some(ColorPrimaries::Bt709),
        VideoColorPrimaries::Bt470bg | VideoColorPrimaries::Smpte170m => {
            Some(ColorPrimaries::Bt601)
        }
        VideoColorPrimaries::Bt2020 => Some(ColorPrimaries::Bt2020),
        _ => None,
    });
    let range = cs.full_range().map(|full| {
        if full {
            ColorRange::Full
        } else {
            ColorRange::Limited
        }
    });
    let transfer = cs.transfer().and_then(|t| match t {
        VideoTransferCharacteristics::Pq => Some(ColorTransfer::Pq),
        VideoTransferCharacteristics::Hlg => Some(ColorTransfer::Hlg),
        VideoTransferCharacteristics::Bt709
        | VideoTransferCharacteristics::Smpte170m
        | VideoTransferCharacteristics::Iec6196621
        | VideoTransferCharacteristics::Linear => Some(ColorTransfer::Sdr),
        _ => None,
    });
    let stream = *shared.stream_meta.lock();
    VideoColorInfo {
        primaries: primaries
            .or(stream.primaries)
            .or_else(|| shared.config_color.map(|c| c.primaries))
            .unwrap_or_else(|| ColorPrimaries::default_for_height(height)),
        range: range
            .or(stream.range)
            .or_else(|| shared.config_color.map(|c| c.range))
            .unwrap_or(ColorRange::Limited),
        transfer: transfer
            .or(stream.transfer)
            .or_else(|| shared.config_color.map(|c| c.transfer))
            .unwrap_or(ColorTransfer::Sdr),
    }
}

fn set_prop(obj: &js_sys::Object, key: &str, value: &JsValue) {
    let _ = js_sys::Reflect::set(obj, &JsValue::from_str(key), value);
}
