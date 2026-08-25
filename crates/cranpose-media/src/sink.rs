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
//!   never blocks — an underrun is silence, not a stall. The device it runs on
//!   is [`cranpose_audio::backend`], the same AAudio-or-cpal stream the audio
//!   engine uses, so there is one output device per platform in the workspace.
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

use cranpose_audio::{
    backend::{self, AudioSink, Renderer},
    ring, RenderStatus,
};
use cranpose_services::MediaError;

use crate::{
    source::{SampleSource, SeekError},
    spool::SpoolCancel,
};

/// How much decoded audio the ring holds, as a fraction of a second.
///
/// Long enough that a decode thread descheduled for an ordinary time slice does
/// not underrun, short enough that discarding it after a seek costs a fraction
/// of a callback's budget.
const BUFFER_SECONDS: f32 = 0.2;

/// The smallest ring worth allocating, for a device that reports an
/// implausibly low rate.
const MIN_BUFFER_SAMPLES: usize = 4096;

/// The highest rate the ring is sized to hold [`BUFFER_SECONDS`] of. Above it a
/// device gets a proportionally shorter buffer, which is still many times the
/// decode thread's nap.
const MAX_DEVICE_RATE: u32 = 96_000;

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
    /// The format the device actually negotiated, published by the callback
    /// the first time it runs and re-published if the device ever changes it.
    ///
    /// Zero until then. AAudio only reports the rate once the stream is open,
    /// so the callback is the one participant that always knows, and the decode
    /// thread waits for it rather than converting to a guess.
    device_rate: AtomicU32,
    device_channels: AtomicU32,
}

impl Shared {
    /// A sink's shared state before anything has been decoded or played.
    fn new(volume: f32, speed: f32) -> Shared {
        Shared {
            generation: AtomicU64::new(0),
            frames_written: AtomicU64::new(0),
            base_nanos: AtomicU64::new(0),
            frames_pushed: AtomicU64::new(0),
            frames_taken: AtomicU64::new(0),
            source_done: AtomicBool::new(false),
            paused: AtomicBool::new(true),
            volume: AtomicVolume::new(volume),
            speed: AtomicVolume::new(speed),
            device_rate: AtomicU32::new(0),
            device_channels: AtomicU32::new(0),
        }
    }

    /// Media position: where the last rebase put us, plus the device frames
    /// written since, converted through the speed those frames were produced at.
    fn position(&self) -> Duration {
        let base = Duration::from_nanos(self.base_nanos.load(Ordering::Relaxed));
        let frames = self.frames_written.load(Ordering::Relaxed);
        let rate = self.device_rate.load(Ordering::Relaxed);
        if rate == 0 {
            return base;
        }
        let elapsed = frames as f64 / f64::from(rate) * f64::from(self.speed.get());
        base.saturating_add(Duration::from_secs_f64(elapsed.max(0.0)))
    }

    /// The negotiated format, or `None` until the callback has published it.
    fn device_format(&self) -> Option<(u32, usize)> {
        let rate = self.device_rate.load(Ordering::Acquire);
        let channels = self.device_channels.load(Ordering::Acquire);
        if rate == 0 || channels == 0 {
            return None;
        }
        Some((rate, channels as usize))
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
    /// Held for its lifetime: dropping it stops the stream and drops the
    /// renderer, and with it the consuming half of the ring.
    device: Box<dyn AudioSink>,
    shared: Arc<Shared>,
    commands: Sender<Command>,
    /// Joined on drop so the decode thread never outlives the sink that owns
    /// its ring.
    decoder: Option<std::thread::JoinHandle<()>>,
    seekable: Arc<AtomicBool>,
    /// Stops the decode thread waiting on a stream that has gone quiet, so
    /// ending an item never waits on a provider.
    spool: SpoolCancel,
}

impl Sink {
    /// Opens the platform output device and starts decoding `source` into it,
    /// paused at its start.
    pub(crate) fn open(
        source: Box<dyn SampleSource>,
        spool: SpoolCancel,
        volume: f32,
        speed: f32,
    ) -> Result<Sink, MediaError> {
        let (producer, consumer) = ring::channel::<f32>(ring_capacity());

        let shared = Arc::new(Shared::new(volume, speed));
        let seekable = Arc::new(AtomicBool::new(true));

        let device = backend::open(Box::new(MediaRenderer {
            consumer,
            shared: Arc::clone(&shared),
            generation: 0,
            channels: backend::NOMINAL_CHANNELS,
        }))
        .map_err(|error| MediaError::Failed(format!("no output device: {error}")))?;

        let (commands, orders) = mpsc::channel();
        let decode_shared = Arc::clone(&shared);
        let decode_seekable = Arc::clone(&seekable);
        let decoder = std::thread::Builder::new()
            .name("cranpose-media-decode".to_owned())
            .spawn(move || decode_loop(source, producer, decode_shared, decode_seekable, orders))
            .map_err(|error| MediaError::Failed(format!("no decode thread: {error}")))?;

        Ok(Sink {
            device,
            shared,
            commands,
            decoder: Some(decoder),
            seekable,
            spool,
        })
    }

    pub(crate) fn play(&self) {
        self.shared.paused.store(false, Ordering::Release);
        self.device.resume();
    }

    pub(crate) fn pause(&self) {
        self.shared.paused.store(true, Ordering::Release);
        self.device.suspend();
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
        // The order matters: a decode thread waiting for bytes from a provider
        // that stopped talking would not reach the command until the wait timed
        // out, and this runs on whichever thread ended the item -- usually the
        // one drawing the screen.
        self.spool.cancel();
        if let Some(decoder) = self.decoder.take() {
            let _ = decoder.join();
        }
    }
}

/// What the device callback runs: pop what the decode thread has ready, scale
/// it, and write silence for whatever is missing.
struct MediaRenderer {
    consumer: ring::Consumer<f32>,
    shared: Arc<Shared>,
    /// The seek generation this callback has already flushed to.
    generation: u64,
    /// The device's channel count, kept here so the callback does not read an
    /// atomic per block to convert samples into frames.
    channels: usize,
}

impl Renderer for MediaRenderer {
    fn set_device_format(&mut self, sample_rate: f32, channels: usize) {
        let rate = sample_rate.max(1.0) as u32;
        let channels = channels.max(1);
        if self.channels == channels && self.shared.device_rate.load(Ordering::Relaxed) == rate {
            return;
        }
        self.channels = channels;
        // Channels first, rate last: the decode thread reads the rate first and
        // takes a non-zero one as the promise that both are published.
        self.shared
            .device_channels
            .store(channels as u32, Ordering::Release);
        self.shared.device_rate.store(rate, Ordering::Release);
    }

    fn render(&mut self, out: &mut [f32]) -> RenderStatus {
        let generation = self.shared.generation.load(Ordering::Acquire);
        if generation != self.generation {
            // Everything still queued belongs to the position we seeked away
            // from.
            let mut discarded = 0u64;
            while self.consumer.pop().is_some() {
                discarded += 1;
            }
            self.shared
                .frames_taken
                .fetch_add(discarded, Ordering::AcqRel);
            self.generation = generation;
        }

        let volume = self.shared.volume.get();
        let mut taken = 0u64;
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
            self.shared.frames_taken.fetch_add(taken, Ordering::AcqRel);
            let frames = taken / self.channels.max(1) as u64;
            self.shared
                .frames_written
                .fetch_add(frames, Ordering::Relaxed);
        }
        // Never idle: a media sink lives exactly as long as the item it plays,
        // so the stream it holds is released by dropping the sink rather than
        // by the callback giving it up mid-track.
        RenderStatus::Continue
    }
}

/// Ring size: [`BUFFER_SECONDS`] of device audio, never below a floor that
/// keeps a small or oddly-configured device from underrunning every callback.
///
/// The ring is allocated before the device says what it negotiated — AAudio
/// only reports its rate once the stream is open — so it is sized for
/// [`MAX_DEVICE_RATE`]. A slower device gets a longer buffer than
/// [`BUFFER_SECONDS`], which costs nothing but the memory.
fn ring_capacity() -> usize {
    let samples = (MAX_DEVICE_RATE as f32 * BUFFER_SECONDS) as usize * backend::NOMINAL_CHANNELS;
    samples.max(MIN_BUFFER_SAMPLES).next_power_of_two()
}

/// The decode thread: convert the item to the device's shape and keep the ring
/// as full as it will go.
///
/// It does not know the device's shape when it starts. The callback publishes
/// the negotiated format the first time it runs, and the converter is built
/// then — and rebuilt if the device ever reports a different one, which is what
/// keeps a track playing at the right pitch across a route change.
fn decode_loop(
    mut source: Box<dyn SampleSource>,
    mut producer: ring::Producer<f32>,
    shared: Arc<Shared>,
    seekable: Arc<AtomicBool>,
    orders: Receiver<Command>,
) {
    let mut converter: Option<Converter> = None;
    let mut format = (0u32, 0usize);
    let mut pending: Option<f32> = None;

    loop {
        match orders.try_recv() {
            Ok(Command::Stop) | Err(TryRecvError::Disconnected) => return,
            Ok(Command::Seek(position)) => {
                match source.try_seek(position) {
                    Ok(()) => {
                        if let Some(converter) = converter.as_mut() {
                            converter.reset();
                        }
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

        let Some(negotiated) = shared.device_format() else {
            // The device has not run its first callback yet, so there is
            // nothing to convert to.
            std::thread::sleep(DECODE_IDLE_NAP);
            continue;
        };
        if negotiated != format {
            log::debug!(
                "cranpose-media: device at {} Hz, {} channels",
                negotiated.0,
                negotiated.1
            );
            format = negotiated;
            converter = Some(Converter::new(&*source, format.0, format.1));
            pending = None;
        }
        let Some(converter) = converter.as_mut() else {
            continue;
        };

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

    /// A sink's shared state as it is once the device has published a format,
    /// which is what every position and end-of-item question is asked about.
    fn playing_at(device_rate: u32) -> Shared {
        let shared = Shared::new(1.0, 1.0);
        shared.paused.store(false, Ordering::Release);
        shared
            .device_channels
            .store(backend::NOMINAL_CHANNELS as u32, Ordering::Release);
        shared.device_rate.store(device_rate, Ordering::Release);
        shared
    }

    #[test]
    fn a_ring_holds_a_fifth_of_a_second_at_the_highest_rate_it_is_sized_for() {
        let capacity = ring_capacity();
        assert!(capacity.is_power_of_two());
        assert!(
            capacity
                >= (MAX_DEVICE_RATE as f32 * BUFFER_SECONDS) as usize * backend::NOMINAL_CHANNELS
        );
        assert!(capacity >= MIN_BUFFER_SAMPLES);
    }

    #[test]
    fn the_format_is_unknown_until_the_callback_publishes_it() {
        let shared = Shared::new(1.0, 1.0);
        assert_eq!(shared.device_format(), None);
        // A position asked for before the first callback is the base, not a
        // division by a rate of zero.
        shared.frames_written.store(24_000, Ordering::Relaxed);
        assert_eq!(shared.position(), Duration::ZERO);

        shared.device_channels.store(2, Ordering::Release);
        shared.device_rate.store(48_000, Ordering::Release);
        assert_eq!(shared.device_format(), Some((48_000, 2)));
        assert_eq!(shared.position(), Duration::from_millis(500));
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
        let shared = playing_at(48_000);
        shared.frames_written.store(24_000, Ordering::Relaxed);
        assert_eq!(shared.position(), Duration::from_millis(500));
    }

    #[test]
    fn double_speed_advances_the_position_twice_as_fast() {
        let shared = playing_at(48_000);
        shared.frames_written.store(24_000, Ordering::Relaxed);
        shared.speed.set(2.0);
        assert_eq!(shared.position(), Duration::from_secs(1));
    }

    #[test]
    fn rebasing_moves_the_position_and_restarts_the_count() {
        let shared = playing_at(48_000);
        shared.frames_written.store(96_000, Ordering::Relaxed);
        shared.rebase(Duration::from_secs(30));
        assert_eq!(shared.position(), Duration::from_secs(30));
        assert_eq!(shared.frames_written.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn an_item_has_not_ended_while_the_ring_still_holds_samples() {
        let shared = playing_at(48_000);
        shared.frames_pushed.store(1_000, Ordering::Release);
        shared.frames_taken.store(400, Ordering::Release);
        shared.source_done.store(true, Ordering::Release);
        assert!(!shared.ended());
        shared.frames_taken.store(1_000, Ordering::Release);
        assert!(shared.ended());
    }

    #[test]
    fn a_source_that_is_still_decoding_has_not_ended_however_much_was_taken() {
        let shared = playing_at(48_000);
        shared.frames_pushed.store(10, Ordering::Release);
        shared.frames_taken.store(10, Ordering::Release);
        assert!(!shared.ended());
    }

    #[test]
    fn volume_survives_the_trip_through_its_bit_pattern() {
        let volume = AtomicVolume::new(0.0);
        volume.set(0.375);
        assert_eq!(volume.get(), 0.375);
    }
}
