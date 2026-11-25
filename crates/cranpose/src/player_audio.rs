//! Shared real-time PCM audio output and [`MasterClock`] pacer for native player backends
//! (Android AAudio, Linux ALSA/PipeWire, Windows WASAPI).
//!
//! Feeds interleaved float samples through `cranpose-audio`'s lock-free SPSC ring buffer into the
//! platform audio callback without allocating or locking on the real-time thread, automatically
//! recovers when the platform audio route disconnects, and anchors video presentation timing to
//! the audio clock within the 40 ms lip-sync window.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

#[cfg(feature = "audio")]
use cranpose_audio::{
    RenderStatus,
    backend::{self, AudioSink, Renderer},
    ring,
};
use cranpose_services::{FrameTiming, MasterClock};
use portable_atomic::AtomicU64;

#[cfg(feature = "audio")]
const RING_BUFFER_SECONDS: usize = 1;
#[cfg(feature = "audio")]
const MAX_DEVICE_RATE: usize = 96_000;
#[cfg(feature = "audio")]
const MIN_RING_SAMPLES: usize = 16_384;

struct AtomicF32(AtomicU32);

impl AtomicF32 {
    fn new(value: f32) -> Self {
        Self(AtomicU32::new(value.to_bits()))
    }

    fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    fn store(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }
}

struct AudioShared {
    device_rate: AtomicU32,
    device_channels: AtomicU32,
    volume: AtomicF32,
    speed: AtomicF32,
    paused: AtomicBool,
    generation: AtomicU64,
    frames_played: AtomicU64,
    base_pts_ns: AtomicU64,
    samples_pushed: AtomicU64,
    samples_consumed: AtomicU64,
}

impl AudioShared {
    fn new(volume: f32, speed: f32) -> Self {
        Self {
            device_rate: AtomicU32::new(48_000),
            device_channels: AtomicU32::new(2),
            volume: AtomicF32::new(volume),
            speed: AtomicF32::new(speed),
            paused: AtomicBool::new(true),
            generation: AtomicU64::new(0),
            frames_played: AtomicU64::new(0),
            base_pts_ns: AtomicU64::new(0),
            samples_pushed: AtomicU64::new(0),
            samples_consumed: AtomicU64::new(0),
        }
    }

    fn device_format(&self) -> (u32, usize) {
        let rate = self.device_rate.load(Ordering::Acquire).max(8_000);
        let channels = usize::try_from(self.device_channels.load(Ordering::Acquire))
            .unwrap_or(2)
            .max(1);
        (rate, channels)
    }

    fn played_position(&self) -> Duration {
        let base = Duration::from_nanos(self.base_pts_ns.load(Ordering::Relaxed));
        let frames = self.frames_played.load(Ordering::Relaxed);
        let rate = u64::from(self.device_rate.load(Ordering::Relaxed).max(1));
        let speed = f64::from(self.speed.load().max(0.01));
        let elapsed_secs = (frames as f64 / rate as f64) * speed;
        if !elapsed_secs.is_finite() || elapsed_secs <= 0.0 {
            base
        } else {
            base.saturating_add(Duration::from_secs_f64(elapsed_secs))
        }
    }

    fn rebase(&self, pts: Duration) {
        let nanos = u64::try_from(pts.as_nanos()).unwrap_or(u64::MAX);
        self.base_pts_ns.store(nanos, Ordering::Relaxed);
        self.frames_played.store(0, Ordering::Relaxed);
    }

    fn queued_samples(&self) -> u64 {
        let pushed = self.samples_pushed.load(Ordering::Acquire);
        let consumed = self.samples_consumed.load(Ordering::Acquire);
        pushed.saturating_sub(consumed)
    }
}

#[cfg(feature = "audio")]
struct PcmRenderer {
    consumer: ring::Consumer<f32>,
    shared: Arc<AudioShared>,
    generation: u64,
    channels: usize,
}

#[cfg(feature = "audio")]
impl Renderer for PcmRenderer {
    fn set_device_format(&mut self, sample_rate: f32, channels: usize) {
        let rate = clamp_sample_rate(sample_rate);
        let channels = channels.max(1);
        if self.channels == channels && self.shared.device_rate.load(Ordering::Relaxed) == rate {
            return;
        }
        self.channels = channels;
        let channels_u32 = u32::try_from(channels).unwrap_or(2);
        self.shared
            .device_channels
            .store(channels_u32, Ordering::Release);
        self.shared.device_rate.store(rate, Ordering::Release);
    }

    fn render(&mut self, out: &mut [f32]) -> RenderStatus {
        let generation = self.shared.generation.load(Ordering::Acquire);
        if generation != self.generation {
            let mut discarded = 0_u64;
            while self.consumer.pop().is_some() {
                discarded += 1;
            }
            if discarded > 0 {
                self.shared
                    .samples_consumed
                    .fetch_add(discarded, Ordering::AcqRel);
            }
            self.generation = generation;
        }

        if self.shared.paused.load(Ordering::Acquire) {
            out.fill(0.0);
            return RenderStatus::Continue;
        }

        let volume = self.shared.volume.load();
        let mut taken = 0_u64;
        for slot in out.iter_mut() {
            match self.consumer.pop() {
                Some(sample) => {
                    taken += 1;
                    *slot = sample * volume;
                }
                None => *slot = 0.0,
            }
        }
        if taken > 0 {
            self.shared
                .samples_consumed
                .fetch_add(taken, Ordering::AcqRel);
            let frames = taken / (self.channels.max(1) as u64);
            self.shared
                .frames_played
                .fetch_add(frames, Ordering::Relaxed);
        }
        RenderStatus::Continue
    }
}

#[cfg(feature = "audio")]
fn clamp_sample_rate(sample_rate: f32) -> u32 {
    if !sample_rate.is_finite() || sample_rate < 8_000.0 {
        return 48_000;
    }
    let clamped = sample_rate.round().clamp(8_000.0, 192_000.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to finite 8_000..=192_000; budgeted lint only active under clippy_budget"
    )]
    {
        clamped as u32
    }
}

#[cfg(feature = "audio")]
fn ring_capacity() -> usize {
    let samples = MAX_DEVICE_RATE * RING_BUFFER_SECONDS * backend::NOMINAL_CHANNELS;
    samples.max(MIN_RING_SAMPLES).next_power_of_two()
}

pub(crate) struct PcmAudioOutput {
    clock: MasterClock,
    shared: Arc<AudioShared>,
    #[cfg(feature = "audio")]
    sink: Option<Box<dyn AudioSink>>,
    #[cfg(feature = "audio")]
    producer: Option<ring::Producer<f32>>,
    has_audio_track: bool,
    device_failed: bool,
    sink_suspended: bool,
    playing: bool,
    speed: f32,
    resample_prev: Vec<f32>,
    resample_frac: f64,
    pending_out: Vec<f32>,
    pending_cursor: usize,
}

impl PcmAudioOutput {
    pub(crate) fn new(volume: f32, speed: f32) -> Self {
        let clamped_speed = speed.clamp(0.1, 4.0);
        let clock = MasterClock::new();
        clock.set_rate(clamped_speed);
        Self {
            clock,
            shared: Arc::new(AudioShared::new(volume.clamp(0.0, 1.0), clamped_speed)),
            #[cfg(feature = "audio")]
            sink: None,
            #[cfg(feature = "audio")]
            producer: None,
            has_audio_track: false,
            device_failed: false,
            sink_suspended: false,
            playing: false,
            speed: clamped_speed,
            resample_prev: Vec::new(),
            resample_frac: 0.0,
            pending_out: Vec::new(),
            pending_cursor: 0,
        }
    }

    pub(crate) fn set_has_audio_track(&mut self, has_audio: bool) {
        self.has_audio_track = has_audio;
        if !has_audio {
            self.close_device();
        }
    }

    pub(crate) fn set_volume(&mut self, volume: f32) {
        self.shared.volume.store(volume.clamp(0.0, 1.0));
    }

    pub(crate) fn set_speed(&mut self, speed: f32) {
        let clamped = speed.clamp(0.1, 4.0);
        let pos = self.position();
        self.speed = clamped;
        self.shared.speed.store(clamped);
        self.shared.rebase(pos);
        self.clock.set_rate(clamped);
        self.clock.seek(pos);
    }

    pub(crate) fn set_playing(&mut self, playing: bool) {
        if self.playing == playing {
            return;
        }
        let pos = self.position();
        self.playing = playing;
        self.shared.paused.store(!playing, Ordering::Release);
        self.shared.rebase(pos);
        self.clock.seek(pos);
        if playing {
            self.clock.resume();
            self.ensure_connected();
            #[cfg(feature = "audio")]
            if self.sink_suspended
                && let Some(sink) = &self.sink
            {
                sink.resume();
                self.sink_suspended = false;
            }
        } else {
            self.clock.pause();
            #[cfg(feature = "audio")]
            if !self.sink_suspended
                && let Some(sink) = &self.sink
            {
                sink.suspend();
                self.sink_suspended = true;
            }
        }
    }

    pub(crate) fn reset(&mut self, position: Duration) {
        self.pending_out.clear();
        self.pending_cursor = 0;
        self.resample_prev.clear();
        self.resample_frac = 0.0;
        self.shared.rebase(position);
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        self.clock.seek(position);
        if self.playing {
            self.clock.resume();
        } else {
            self.clock.pause();
        }
    }

    pub(crate) fn position(&mut self) -> Duration {
        #[cfg(feature = "audio")]
        if self.has_audio_track && self.sink.is_some() {
            let played = self.shared.played_position();
            if self.playing {
                self.clock.sync_audio(played);
            }
            return played;
        }
        self.clock.position()
    }

    pub(crate) fn frame_action(&mut self, video_pts: Duration) -> FrameTiming {
        let _: Duration = self.position();
        self.clock.frame_action(video_pts)
    }

    pub(crate) fn buffered_duration(&self) -> Duration {
        #[cfg(feature = "audio")]
        if self.sink.is_some() {
            let (rate, channels) = self.shared.device_format();
            let ring_samples = self.shared.queued_samples();
            let pending_samples =
                (self.pending_out.len().saturating_sub(self.pending_cursor)) as u64;
            let total_frames =
                ring_samples.saturating_add(pending_samples) / (channels.max(1) as u64);
            let secs = total_frames as f64 / f64::from(rate.max(1));
            if secs.is_finite() && secs > 0.0 {
                return Duration::from_secs_f64(secs);
            }
        }
        Duration::ZERO
    }

    pub(crate) fn ensure_connected(&mut self) {
        if !self.has_audio_track || !self.playing {
            return;
        }
        #[cfg(feature = "audio")]
        {
            if let Some(sink) = &self.sink {
                if sink.is_running() {
                    return;
                }
                log::info!("cranpose player: audio route disconnected, reopening output stream");
                let pos = self.shared.played_position();
                self.close_device();
                self.shared.rebase(pos);
                self.clock.seek(pos);
                self.device_failed = false;
            }
            if self.device_failed || !backend::is_compiled() {
                return;
            }
            let (producer, consumer) = ring::channel::<f32>(ring_capacity());
            let generation = self.shared.generation.load(Ordering::Acquire);
            let renderer = PcmRenderer {
                consumer,
                shared: Arc::clone(&self.shared),
                generation,
                channels: backend::NOMINAL_CHANNELS,
            };
            match backend::open(Box::new(renderer)) {
                Ok(sink) => {
                    self.sink = Some(sink);
                    self.producer = Some(producer);
                    self.shared.paused.store(!self.playing, Ordering::Release);
                    self.device_failed = false;
                    self.sink_suspended = false;
                }
                Err(error) => {
                    log::warn!(
                        "cranpose player: audio output unavailable ({error}); using wall-clock sync"
                    );
                    self.device_failed = true;
                }
            }
        }
    }

    pub(crate) fn flush_pending(&mut self) -> bool {
        #[cfg(feature = "audio")]
        {
            let Some(producer) = self.producer.as_mut() else {
                self.pending_out.clear();
                self.pending_cursor = 0;
                return true;
            };
            while self.pending_cursor < self.pending_out.len() {
                let sample = self.pending_out[self.pending_cursor];
                if producer.push(sample).is_err() {
                    return false;
                }
                self.pending_cursor += 1;
                self.shared.samples_pushed.fetch_add(1, Ordering::AcqRel);
            }
            self.pending_out.clear();
            self.pending_cursor = 0;
            true
        }
        #[cfg(not(feature = "audio"))]
        {
            self.pending_out.clear();
            self.pending_cursor = 0;
            true
        }
    }

    pub(crate) fn push_pcm_i16(&mut self, pcm: &[i16], src_rate: u32, src_channels: usize) {
        self.push_pcm_samples(
            pcm.len() / src_channels.max(1),
            src_rate,
            src_channels,
            |frame, ch| sample_i16_frame(pcm, frame, ch, src_channels),
        );
    }

    #[allow(
        dead_code,
        reason = "used by Linux/Windows and float-output Android audio decoders"
    )]
    pub(crate) fn push_pcm_f32(&mut self, pcm: &[f32], src_rate: u32, src_channels: usize) {
        self.push_pcm_samples(
            pcm.len() / src_channels.max(1),
            src_rate,
            src_channels,
            |frame, ch| sample_f32_frame(pcm, frame, ch, src_channels),
        );
    }

    fn push_pcm_samples(
        &mut self,
        src_frames: usize,
        src_rate: u32,
        src_channels: usize,
        read_sample: impl Fn(usize, usize) -> f32,
    ) {
        if src_frames == 0 || src_rate == 0 || src_channels == 0 {
            return;
        }
        self.ensure_connected();
        #[cfg(feature = "audio")]
        if self.producer.is_none() {
            return;
        }
        #[cfg(not(feature = "audio"))]
        return;

        let (dst_rate, dst_channels) = self.shared.device_format();
        let step = (f64::from(src_rate) * f64::from(self.speed)) / f64::from(dst_rate.max(1));
        if !step.is_finite() || step <= 0.0 {
            return;
        }
        if self.resample_prev.len() != src_channels {
            self.resample_prev.clear();
            self.resample_prev.resize(src_channels, 0.0);
        }

        let mut pos = self.resample_frac;
        let limit = src_frames as f64;
        while pos < limit {
            let idx = floor_usize(pos);
            let frac = frac_f32(pos, idx);
            for ch in 0..dst_channels {
                let a = if idx == 0 {
                    sample_channel_f32(&self.resample_prev, ch, src_channels)
                } else {
                    read_sample(idx - 1, ch)
                };
                let b = read_sample(idx.min(src_frames - 1), ch);
                self.pending_out.push(a + (b - a) * frac);
            }
            pos += step;
        }
        self.resample_frac = pos - limit;
        let last_frame = src_frames - 1;
        for (ch, slot) in self.resample_prev.iter_mut().enumerate().take(src_channels) {
            *slot = read_sample(last_frame, ch);
        }
        let _: bool = self.flush_pending();
    }

    pub(crate) fn close_device(&mut self) {
        #[cfg(feature = "audio")]
        {
            self.sink = None;
            self.producer = None;
        }
        self.sink_suspended = false;
        self.pending_out.clear();
        self.pending_cursor = 0;
    }
}

impl Drop for PcmAudioOutput {
    fn drop(&mut self) {
        self.close_device();
    }
}

fn frac_f32(pos: f64, idx: usize) -> f32 {
    let frac = (pos - idx as f64).clamp(0.0, 1.0);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "interpolation fraction clamped to 0.0..=1.0; budgeted lint only active under clippy_budget"
    )]
    {
        frac as f32
    }
}

fn floor_usize(value: f64) -> usize {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    let floored = value.floor();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "non-negative finite frame index; budgeted lint only active under clippy_budget"
    )]
    {
        floored as usize
    }
}

fn sample_channel_f32(frame: &[f32], dst_ch: usize, src_channels: usize) -> f32 {
    if src_channels == 1 {
        frame.first().copied().unwrap_or(0.0)
    } else if dst_ch < src_channels {
        frame[dst_ch]
    } else {
        0.0
    }
}

fn sample_i16_frame(pcm: &[i16], frame: usize, dst_ch: usize, src_channels: usize) -> f32 {
    let base = frame * src_channels;
    let raw = if src_channels == 1 {
        pcm.get(base).copied().unwrap_or(0)
    } else if dst_ch < src_channels {
        pcm.get(base + dst_ch).copied().unwrap_or(0)
    } else {
        0
    };
    f32::from(raw) / 32_768.0
}

#[allow(
    dead_code,
    reason = "used by push_pcm_f32 for float-output audio decoders"
)]
fn sample_f32_frame(pcm: &[f32], frame: usize, dst_ch: usize, src_channels: usize) -> f32 {
    let base = frame * src_channels;
    if src_channels == 1 {
        pcm.get(base).copied().unwrap_or(0.0)
    } else if dst_ch < src_channels {
        pcm.get(base + dst_ch).copied().unwrap_or(0.0)
    } else {
        0.0
    }
}
