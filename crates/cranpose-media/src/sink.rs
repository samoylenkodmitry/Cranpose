//! The output device and the thread that feeds it.
//!
//! Three participants, and it is worth naming what each may do:
//!
//! * The **decode thread** owns the [`SampleSource`] chain. It is the only
//!   thread that touches the decoder, so seeking is a message to it rather than
//!   a lock around it. It converts the item's rate and channel layout to the
//!   device's and pushes the result into the ring.
//! * The **device callback** owns the consuming half of the ring. It pops,
//!   scales by the volume, and writes. It allocates nothing, locks nothing and
//!   never blocks — an underrun is silence, not a stall.
//! * The **transport** (whatever thread calls [`Sink`]'s methods) owns neither.
//!   It reads atomics and sends commands.
//!
//! The ring is [`cranpose_audio::ring`], the same wait-free queue the audio
//! engine feeds its own callback through.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc,
        mpsc::{Receiver, Sender, TryRecvError},
        Arc,
    },
    time::Duration,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cranpose_audio::ring;
use cranpose_services::MediaError;

use crate::source::{SampleSource, SeekError};

/// How much decoded audio the ring holds, as a fraction of a second.
///
/// Long enough that a decode thread descheduled for an ordinary time slice does
/// not underrun, short enough that discarding it after a seek costs a fraction
/// of a callback's budget.
const BUFFER_SECONDS: f32 = 0.2;

/// The smallest ring worth allocating, for a device that reports an
/// implausibly low rate.
const MIN_BUFFER_SAMPLES: usize = 4096;

/// How long the decode thread sleeps when the ring is full or playback is
/// paused. A fraction of the buffer, so it wakes with room to spare.
const DECODE_IDLE_NAP: Duration = Duration::from_millis(5);

/// Volume is shared with the callback as a bit pattern because there is no
/// atomic `f32`.
struct AtomicVolume(AtomicU32);

impl AtomicVolume {
    fn new(value: f32) -> AtomicVolume {
        AtomicVolume(AtomicU32::new(value.to_bits()))
    }

    fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    fn set(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }
}

/// What the three participants share.
struct Shared {
    /// Bumped by the decode thread after a seek. The callback discards
    /// everything queued under an older generation, which is what stops audio
    /// from before the seek being heard after it.
    generation: AtomicU64,
    /// Device frames the callback has written since the position was last
    /// rebased.
    frames_written: AtomicU64,
    /// Media position at the last rebase, in nanoseconds.
    base_nanos: AtomicU64,
    /// Device frames the decode thread has pushed, and frames the callback has
    /// taken. The item has finished when the source is done and these meet.
    frames_pushed: AtomicU64,
    frames_taken: AtomicU64,
    source_done: AtomicBool,
    paused: AtomicBool,
    volume: AtomicVolume,
    /// Playback rate, read by the decode thread on every block.
    speed: AtomicVolume,
    device_rate: u32,
}

impl Shared {
    /// Media position: where the last rebase put us, plus the device frames
    /// written since, converted through the speed those frames were produced at.
    fn position(&self) -> Duration {
        let base = Duration::from_nanos(self.base_nanos.load(Ordering::Relaxed));
        let frames = self.frames_written.load(Ordering::Relaxed);
        if self.device_rate == 0 {
            return base;
        }
        let elapsed = frames as f64 / f64::from(self.device_rate) * f64::from(self.speed.get());
        base.saturating_add(Duration::from_secs_f64(elapsed.max(0.0)))
    }

    /// Moves the position to `position` and starts counting from zero again.
    /// Called whenever the mapping from written frames to media time changes:
    /// after a seek, and after a speed change.
    fn rebase(&self, position: Duration) {
        self.base_nanos.store(
            position.as_nanos().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
        self.frames_written.store(0, Ordering::Relaxed);
    }

    fn ended(&self) -> bool {
        self.source_done.load(Ordering::Acquire)
            && self.frames_taken.load(Ordering::Acquire)
                >= self.frames_pushed.load(Ordering::Acquire)
    }
}

/// What the transport asks the decode thread to do.
enum Command {
    Seek(Duration),
    Stop,
}

/// An open item on an open device.
pub(crate) struct Sink {
    /// Held for its lifetime: dropping it closes the output stream.
    stream: cpal::Stream,
    shared: Arc<Shared>,
    commands: Sender<Command>,
    /// Joined on drop so the decode thread never outlives the sink that owns
    /// its ring.
    decoder: Option<std::thread::JoinHandle<()>>,
    seekable: Arc<AtomicBool>,
}

impl Sink {
    /// Opens the default output device and starts decoding `source` into it,
    /// paused at its start.
    pub(crate) fn open(
        source: Box<dyn SampleSource>,
        volume: f32,
        speed: f32,
    ) -> Result<Sink, MediaError> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| {
            MediaError::Failed("the system reports no default audio output device".to_owned())
        })?;
        let supported = device
            .default_output_config()
            .map_err(|error| MediaError::Failed(format!("no usable output config: {error}")))?;
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let device_channels = usize::from(config.channels).max(1);
        let device_rate = config.sample_rate;

        let capacity = ring_capacity(device_rate, device_channels);
        let (producer, mut consumer) = ring::channel::<f32>(capacity);

        let shared = Arc::new(Shared {
            generation: AtomicU64::new(0),
            frames_written: AtomicU64::new(0),
            base_nanos: AtomicU64::new(0),
            frames_pushed: AtomicU64::new(0),
            frames_taken: AtomicU64::new(0),
            source_done: AtomicBool::new(false),
            paused: AtomicBool::new(true),
            volume: AtomicVolume::new(volume),
            speed: AtomicVolume::new(speed),
            device_rate,
        });
        let seekable = Arc::new(AtomicBool::new(true));

        let (commands, orders) = mpsc::channel();
        let decode_shared = Arc::clone(&shared);
        let decode_seekable = Arc::clone(&seekable);
        let decoder = std::thread::Builder::new()
            .name("cranpose-media-decode".to_owned())
            .spawn(move || {
                decode_loop(
                    source,
                    producer,
                    decode_shared,
                    decode_seekable,
                    orders,
                    device_rate,
                    device_channels,
                )
            })
            .map_err(|error| MediaError::Failed(format!("no decode thread: {error}")))?;

        let callback_shared = Arc::clone(&shared);
        let mut callback_generation = 0u64;
        let error_callback = |error| log::warn!("cranpose-media stream error: {error}");

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    render(
                        data,
                        &mut consumer,
                        &callback_shared,
                        &mut callback_generation,
                        device_channels,
                        |sample| sample,
                    );
                },
                error_callback,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    render(
                        data,
                        &mut consumer,
                        &callback_shared,
                        &mut callback_generation,
                        device_channels,
                        |sample| (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16,
                    );
                },
                error_callback,
                None,
            ),
            other => {
                return Err(MediaError::Failed(format!(
                    "the default output device wants {other:?} samples, which this player does not write"
                )))
            }
        }
        .map_err(|error| MediaError::Failed(format!("failed to build the output stream: {error}")))?;

        stream
            .play()
            .map_err(|error| MediaError::Failed(format!("failed to start the stream: {error}")))?;

        log::debug!("cranpose-media: cpal stream at {device_rate} Hz, {device_channels} channels");

        Ok(Sink {
            stream,
            shared,
            commands,
            decoder: Some(decoder),
            seekable,
        })
    }

    pub(crate) fn play(&self) {
        self.shared.paused.store(false, Ordering::Release);
        if let Err(error) = self.stream.play() {
            log::warn!("cranpose-media: failed to start the output stream: {error}");
        }
    }

    pub(crate) fn pause(&self) {
        self.shared.paused.store(true, Ordering::Release);
        if let Err(error) = self.stream.pause() {
            log::warn!("cranpose-media: failed to pause the output stream: {error}");
        }
    }

    pub(crate) fn set_volume(&self, volume: f32) {
        self.shared.volume.set(volume);
    }

    /// Changes the playback rate and rebases the position, because the frames
    /// written from here on convert to media time at a different ratio than the
    /// ones already counted.
    pub(crate) fn set_speed(&self, speed: f32) {
        let position = self.shared.position();
        self.shared.speed.set(speed);
        self.shared.rebase(position);
    }

    pub(crate) fn seek(&self, position: Duration) -> Result<(), MediaError> {
        if !self.seekable.load(Ordering::Acquire) {
            return Err(MediaError::Failed(SeekError::Unsupported.to_string()));
        }
        self.commands
            .send(Command::Seek(position))
            .map_err(|_| MediaError::Failed("the decode thread is gone".to_owned()))
    }

    pub(crate) fn position(&self) -> Duration {
        self.shared.position()
    }

    pub(crate) fn ended(&self) -> bool {
        self.shared.ended()
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(decoder) = self.decoder.take() {
            let _ = decoder.join();
        }
    }
}

/// Ring size: [`BUFFER_SECONDS`] of device audio, never below a floor that
/// keeps a small or oddly-configured device from underrunning every callback.
fn ring_capacity(device_rate: u32, device_channels: usize) -> usize {
    let samples = (device_rate as f32 * BUFFER_SECONDS) as usize * device_channels;
    samples.max(MIN_BUFFER_SAMPLES).next_power_of_two()
}

/// The device callback. Pops what the decode thread has ready, scales it, and
/// writes silence for whatever is missing.
fn render<T: cpal::Sample>(
    data: &mut [T],
    consumer: &mut ring::Consumer<f32>,
    shared: &Shared,
    callback_generation: &mut u64,
    device_channels: usize,
    convert: impl Fn(f32) -> T,
) {
    let generation = shared.generation.load(Ordering::Acquire);
    if generation != *callback_generation {
        // Everything still queued belongs to the position we seeked away from.
        let mut discarded = 0u64;
        while consumer.pop().is_some() {
            discarded += 1;
        }
        shared.frames_taken.fetch_add(discarded, Ordering::AcqRel);
        *callback_generation = generation;
    }

    let volume = shared.volume.get();
    let mut taken = 0u64;
    for slot in data.iter_mut() {
        match consumer.pop() {
            Some(sample) => {
                taken += 1;
                *slot = convert(sample * volume);
            }
            None => *slot = convert(0.0),
        }
    }
    if taken > 0 {
        shared.frames_taken.fetch_add(taken, Ordering::AcqRel);
        let frames = taken / device_channels.max(1) as u64;
        shared.frames_written.fetch_add(frames, Ordering::Relaxed);
    }
}

/// The decode thread: convert the item to the device's shape and keep the ring
/// as full as it will go.
fn decode_loop(
    mut source: Box<dyn SampleSource>,
    mut producer: ring::Producer<f32>,
    shared: Arc<Shared>,
    seekable: Arc<AtomicBool>,
    orders: Receiver<Command>,
    device_rate: u32,
    device_channels: usize,
) {
    let mut converter = Converter::new(&*source, device_rate, device_channels);
    let mut pending: Option<f32> = None;

    loop {
        match orders.try_recv() {
            Ok(Command::Stop) | Err(TryRecvError::Disconnected) => return,
            Ok(Command::Seek(position)) => {
                match source.try_seek(position) {
                    Ok(()) => {
                        converter.reset();
                        pending = None;
                        shared.source_done.store(false, Ordering::Release);
                        shared.rebase(position);
                        // Publishing the new generation last is what makes the
                        // callback's discard cover exactly the stale samples.
                        shared.generation.fetch_add(1, Ordering::AcqRel);
                    }
                    Err(SeekError::Unsupported) => {
                        seekable.store(false, Ordering::Release);
                        log::debug!("cranpose-media: this item cannot seek");
                    }
                    Err(SeekError::Failed(reason)) => {
                        log::warn!("cranpose-media: seek failed: {reason}");
                    }
                }
                continue;
            }
            Err(TryRecvError::Empty) => {}
        }

        if shared.paused.load(Ordering::Acquire) {
            std::thread::sleep(DECODE_IDLE_NAP);
            continue;
        }

        let sample = match pending.take() {
            Some(sample) => Some(sample),
            None => converter.next(&mut *source, shared.speed.get()),
        };
        let Some(sample) = sample else {
            shared.source_done.store(true, Ordering::Release);
            std::thread::sleep(DECODE_IDLE_NAP);
            continue;
        };
        match producer.push(sample) {
            Ok(()) => {
                shared.frames_pushed.fetch_add(1, Ordering::AcqRel);
            }
            Err(sample) => {
                // The ring is full, which is the healthy state. Hold the sample
                // so it is not lost and wait for the callback to make room.
                pending = Some(sample);
                std::thread::sleep(DECODE_IDLE_NAP);
            }
        }
    }
}

/// Turns the item's rate and channel layout into the device's.
///
/// Linear interpolation between the two frames either side of the read
/// position. Speed folds into the same step: playing twice as fast is
/// advancing the read position twice as far per output frame.
struct Converter {
    source_rate: f32,
    source_channels: usize,
    device_rate: f32,
    device_channels: usize,
    /// The two source frames the current output frame sits between.
    previous: Vec<f32>,
    next: Vec<f32>,
    /// How far between them, in source frames.
    fraction: f32,
    /// The output frame being handed out one sample at a time.
    frame: Vec<f32>,
    emitted: usize,
    primed: bool,
    done: bool,
}

impl Converter {
    fn new(source: &dyn SampleSource, device_rate: u32, device_channels: usize) -> Converter {
        let source_channels = usize::from(source.channels().get());
        Converter {
            source_rate: source.sample_rate().get() as f32,
            source_channels,
            device_rate: device_rate as f32,
            device_channels,
            previous: vec![0.0; source_channels],
            next: vec![0.0; source_channels],
            fraction: 0.0,
            frame: vec![0.0; device_channels],
            emitted: device_channels,
            primed: false,
            done: false,
        }
    }

    fn reset(&mut self) {
        self.previous.iter_mut().for_each(|sample| *sample = 0.0);
        self.next.iter_mut().for_each(|sample| *sample = 0.0);
        self.fraction = 0.0;
        self.emitted = self.device_channels;
        self.primed = false;
        self.done = false;
    }

    /// The next device sample, or `None` once the source has run out.
    fn next(&mut self, source: &mut dyn SampleSource, speed: f32) -> Option<f32> {
        if self.emitted >= self.device_channels && !self.advance(source, speed) {
            return None;
        }
        let sample = self.frame[self.emitted];
        self.emitted += 1;
        Some(sample)
    }

    /// Produces one output frame, pulling source frames until the read position
    /// is between `previous` and `next`.
    fn advance(&mut self, source: &mut dyn SampleSource, speed: f32) -> bool {
        if self.done {
            return false;
        }
        if !self.primed {
            if !read_frame(source, &mut self.previous) || !read_frame(source, &mut self.next) {
                self.done = true;
                return false;
            }
            self.primed = true;
        }

        let step = (self.source_rate * speed.max(0.01)) / self.device_rate.max(1.0);
        while self.fraction >= 1.0 {
            std::mem::swap(&mut self.previous, &mut self.next);
            if !read_frame(source, &mut self.next) {
                self.done = true;
                return false;
            }
            self.fraction -= 1.0;
        }

        for (channel, slot) in self.frame.iter_mut().enumerate() {
            *slot = map_channel(
                &self.previous,
                &self.next,
                self.fraction,
                channel,
                self.source_channels,
            );
        }
        self.fraction += step;
        self.emitted = 0;
        true
    }
}

/// Reads one interleaved source frame. `false` once the source is exhausted,
/// including a partial frame at the end, which is not a frame.
fn read_frame(source: &mut dyn SampleSource, frame: &mut [f32]) -> bool {
    for slot in frame.iter_mut() {
        match source.next() {
            Some(sample) => *slot = sample,
            None => return false,
        }
    }
    true
}

/// One device channel, interpolated between two source frames.
///
/// Mono into anything wider plays on every channel; anything wider into mono
/// is the mean of what the source has; otherwise channels line up by index and
/// a device channel the source does not reach stays silent.
fn map_channel(
    previous: &[f32],
    next: &[f32],
    fraction: f32,
    channel: usize,
    source_channels: usize,
) -> f32 {
    let at = |frame: &[f32]| -> f32 {
        if source_channels == 1 {
            frame.first().copied().unwrap_or(0.0)
        } else if channel < source_channels {
            frame[channel]
        } else {
            0.0
        }
    };
    let start = at(previous);
    let end = at(next);
    start + (end - start) * fraction.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SamplesBuffer;

    #[test]
    fn a_ring_holds_a_fifth_of_a_second_rounded_up_to_a_power_of_two() {
        let capacity = ring_capacity(48_000, 2);
        assert!(capacity.is_power_of_two());
        assert!(capacity >= (48_000.0 * BUFFER_SECONDS) as usize * 2);
    }

    #[test]
    fn a_tiny_device_rate_still_gets_a_usable_ring() {
        assert!(ring_capacity(8_000, 1) >= MIN_BUFFER_SAMPLES);
    }

    #[test]
    fn matching_rates_and_channels_pass_samples_through_unchanged() {
        let mut source: Box<dyn SampleSource> =
            Box::new(SamplesBuffer::new(1, 8_000, vec![0.1, 0.2, 0.3, 0.4, 0.5]));
        let mut converter = Converter::new(&*source, 8_000, 1);
        let mut produced = Vec::new();
        while let Some(sample) = converter.next(&mut *source, 1.0) {
            produced.push(sample);
        }
        // The converter reads one frame ahead, so the last frame is the one it
        // cannot interpolate past and is not emitted.
        assert_eq!(produced.len(), 4);
        for (index, sample) in produced.iter().enumerate() {
            assert!(
                (sample - (0.1 * (index + 1) as f32)).abs() < 1e-5,
                "{produced:?}"
            );
        }
    }

    #[test]
    fn mono_into_stereo_plays_on_both_channels() {
        let mut source: Box<dyn SampleSource> =
            Box::new(SamplesBuffer::new(1, 8_000, vec![1.0, 1.0, 1.0, 1.0]));
        let mut converter = Converter::new(&*source, 8_000, 2);
        let left = converter.next(&mut *source, 1.0).expect("left");
        let right = converter.next(&mut *source, 1.0).expect("right");
        assert_eq!(left, right);
    }

    #[test]
    fn a_device_channel_the_source_does_not_reach_is_silent() {
        assert_eq!(map_channel(&[1.0, 1.0], &[1.0, 1.0], 0.0, 5, 2), 0.0);
    }

    #[test]
    fn halving_the_rate_produces_half_as_many_frames() {
        let frames = 64;
        let mut source: Box<dyn SampleSource> =
            Box::new(SamplesBuffer::new(1, 8_000, vec![0.5; frames]));
        let mut converter = Converter::new(&*source, 4_000, 1);
        let mut produced = 0;
        while converter.next(&mut *source, 1.0).is_some() {
            produced += 1;
        }
        assert!(
            (produced - (frames as i32 / 2)).abs() <= 2,
            "produced {produced} from {frames}"
        );
    }

    #[test]
    fn double_speed_consumes_the_source_twice_as_fast() {
        let frames = 64;
        let mut source: Box<dyn SampleSource> =
            Box::new(SamplesBuffer::new(1, 8_000, vec![0.5; frames]));
        let mut converter = Converter::new(&*source, 8_000, 1);
        let mut produced = 0;
        while converter.next(&mut *source, 2.0).is_some() {
            produced += 1;
        }
        assert!(
            (produced - (frames as i32 / 2)).abs() <= 2,
            "produced {produced} from {frames}"
        );
    }

    #[test]
    fn the_position_follows_the_frames_the_callback_wrote() {
        let shared = Shared {
            generation: AtomicU64::new(0),
            frames_written: AtomicU64::new(24_000),
            base_nanos: AtomicU64::new(0),
            frames_pushed: AtomicU64::new(0),
            frames_taken: AtomicU64::new(0),
            source_done: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            volume: AtomicVolume::new(1.0),
            speed: AtomicVolume::new(1.0),
            device_rate: 48_000,
        };
        assert_eq!(shared.position(), Duration::from_millis(500));
    }

    #[test]
    fn double_speed_advances_the_position_twice_as_fast() {
        let shared = Shared {
            generation: AtomicU64::new(0),
            frames_written: AtomicU64::new(24_000),
            base_nanos: AtomicU64::new(0),
            frames_pushed: AtomicU64::new(0),
            frames_taken: AtomicU64::new(0),
            source_done: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            volume: AtomicVolume::new(1.0),
            speed: AtomicVolume::new(2.0),
            device_rate: 48_000,
        };
        assert_eq!(shared.position(), Duration::from_secs(1));
    }

    #[test]
    fn rebasing_moves_the_position_and_restarts_the_count() {
        let shared = Shared {
            generation: AtomicU64::new(0),
            frames_written: AtomicU64::new(96_000),
            base_nanos: AtomicU64::new(0),
            frames_pushed: AtomicU64::new(0),
            frames_taken: AtomicU64::new(0),
            source_done: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            volume: AtomicVolume::new(1.0),
            speed: AtomicVolume::new(1.0),
            device_rate: 48_000,
        };
        shared.rebase(Duration::from_secs(30));
        assert_eq!(shared.position(), Duration::from_secs(30));
        assert_eq!(shared.frames_written.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn an_item_has_not_ended_while_the_ring_still_holds_samples() {
        let shared = Shared {
            generation: AtomicU64::new(0),
            frames_written: AtomicU64::new(0),
            base_nanos: AtomicU64::new(0),
            frames_pushed: AtomicU64::new(1_000),
            frames_taken: AtomicU64::new(400),
            source_done: AtomicBool::new(true),
            paused: AtomicBool::new(false),
            volume: AtomicVolume::new(1.0),
            speed: AtomicVolume::new(1.0),
            device_rate: 48_000,
        };
        assert!(!shared.ended());
        shared.frames_taken.store(1_000, Ordering::Release);
        assert!(shared.ended());
    }

    #[test]
    fn a_source_that_is_still_decoding_has_not_ended_however_much_was_taken() {
        let shared = Shared {
            generation: AtomicU64::new(0),
            frames_written: AtomicU64::new(0),
            base_nanos: AtomicU64::new(0),
            frames_pushed: AtomicU64::new(10),
            frames_taken: AtomicU64::new(10),
            source_done: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            volume: AtomicVolume::new(1.0),
            speed: AtomicVolume::new(1.0),
            device_rate: 48_000,
        };
        assert!(!shared.ended());
    }

    #[test]
    fn volume_survives_the_trip_through_its_bit_pattern() {
        let volume = AtomicVolume::new(0.0);
        volume.set(0.375);
        assert_eq!(volume.get(), 0.375);
    }
}
