use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc,
        mpsc::{Receiver, Sender, TryRecvError},
    },
    time::Duration,
};

use cranpose_audio::{
    RenderStatus,
    backend::{self, AudioSink, Renderer},
    ring,
};
use cranpose_services::MediaError;

use crate::{
    source::{SampleSource, SeekError},
    spool::SpoolCancel,
};

const BUFFER_SECONDS: f32 = 0.2;

const MIN_BUFFER_SAMPLES: usize = 4096;

const MAX_DEVICE_RATE: u32 = 96_000;

const DECODE_IDLE_NAP: Duration = Duration::from_millis(5);

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

struct Shared {
    generation: AtomicU64,
    frames_written: AtomicU64,
    base_nanos: AtomicU64,
    frames_pushed: AtomicU64,
    frames_taken: AtomicU64,
    source_done: AtomicBool,
    paused: AtomicBool,
    volume: AtomicVolume,
    speed: AtomicVolume,
    device_rate: AtomicU32,
    device_channels: AtomicU32,
}

impl Shared {
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

    fn device_format(&self) -> Option<(u32, usize)> {
        let rate = self.device_rate.load(Ordering::Acquire);
        let channels = self.device_channels.load(Ordering::Acquire);
        if rate == 0 || channels == 0 {
            return None;
        }
        Some((rate, channels as usize))
    }

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

enum Command {
    Seek(Duration),
    Stop,
}

pub(crate) struct Sink {
    device: Box<dyn AudioSink>,
    shared: Arc<Shared>,
    commands: Sender<Command>,
    decoder: Option<std::thread::JoinHandle<()>>,
    seekable: Arc<AtomicBool>,
    spool: SpoolCancel,
}

impl Sink {
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
        self.spool.cancel();
        if let Some(decoder) = self.decoder.take() {
            let _ = decoder.join();
        }
    }
}

struct MediaRenderer {
    consumer: ring::Consumer<f32>,
    shared: Arc<Shared>,
    generation: u64,
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
        self.shared
            .device_channels
            .store(channels as u32, Ordering::Release);
        self.shared.device_rate.store(rate, Ordering::Release);
    }

    fn render(&mut self, out: &mut [f32]) -> RenderStatus {
        let generation = self.shared.generation.load(Ordering::Acquire);
        if generation != self.generation {
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
        RenderStatus::Continue
    }
}

fn ring_capacity() -> usize {
    let samples = (MAX_DEVICE_RATE as f32 * BUFFER_SECONDS) as usize * backend::NOMINAL_CHANNELS;
    samples.max(MIN_BUFFER_SAMPLES).next_power_of_two()
}

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
                pending = Some(sample);
                std::thread::sleep(DECODE_IDLE_NAP);
            }
        }
    }
}

struct Converter {
    source_rate: f32,
    source_channels: usize,
    device_rate: f32,
    device_channels: usize,
    previous: Vec<f32>,
    next: Vec<f32>,
    fraction: f32,
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

    fn next(&mut self, source: &mut dyn SampleSource, speed: f32) -> Option<f32> {
        if self.emitted >= self.device_channels && !self.advance(source, speed) {
            return None;
        }
        let sample = self.frame[self.emitted];
        self.emitted += 1;
        Some(sample)
    }

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

fn read_frame(source: &mut dyn SampleSource, frame: &mut [f32]) -> bool {
    for slot in frame.iter_mut() {
        match source.next() {
            Some(sample) => *slot = sample,
            None => return false,
        }
    }
    true
}

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
