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
        capacity >= (MAX_DEVICE_RATE as f32 * BUFFER_SECONDS) as usize * backend::NOMINAL_CHANNELS
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
