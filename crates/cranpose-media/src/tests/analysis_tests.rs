use super::*;
use crate::source::SamplesBuffer;

fn tone(frames: usize, channels: u16) -> SamplesBuffer {
    SamplesBuffer::new(channels, 48_000, vec![0.25f32; frames * channels as usize])
}

#[test]
fn a_disabled_tap_lets_the_samples_through_untouched() {
    let tap = AnalysisTap::new();
    let mut source = tap.wrap(tone(ANALYSIS_FRAMES * 2, 2));

    let played: Vec<f32> = source.by_ref().collect();

    assert_eq!(played.len(), ANALYSIS_FRAMES * 4);
    assert!(played.iter().all(|sample| *sample == 0.25));
    assert!(!tap.filled.load(Ordering::Acquire));
}

#[test]
fn an_enabled_tap_publishes_one_block_at_a_time() {
    let tap = AnalysisTap::new();
    let mut source = tap.wrap(tone(ANALYSIS_FRAMES * 2, 2));
    tap.set_enabled(true);

    for _ in 0..ANALYSIS_FRAMES * 2 - 2 {
        source.next().expect("samples");
    }
    assert!(!tap.filled.load(Ordering::Acquire));

    source.next().expect("samples");
    source.next().expect("samples");
    assert!(tap.filled.load(Ordering::Acquire));
    assert_eq!(tap.ready.lock().len(), ANALYSIS_FRAMES * 2);
    assert_eq!(tap.sequence.load(Ordering::Acquire), 1);
}

#[test]
fn a_block_that_nobody_took_is_replaced_rather_than_queued() {
    let tap = AnalysisTap::new();
    let mut source = tap.wrap(tone(ANALYSIS_FRAMES * 4, 1));
    tap.set_enabled(true);

    for _ in 0..ANALYSIS_FRAMES * 3 {
        source.next().expect("samples");
    }

    assert_eq!(tap.sequence.load(Ordering::Acquire), 3);
    assert!(tap.filled.load(Ordering::Acquire));
}

#[test]
fn the_output_callback_never_grows_its_staging_buffer() {
    let tap = AnalysisTap::new();
    let mut source = tap.wrap(tone(ANALYSIS_FRAMES * 2, 2));
    tap.set_enabled(true);
    let capacity = source.staging.capacity();

    for _ in 0..ANALYSIS_FRAMES * 4 {
        source.next().expect("samples");
    }

    assert_eq!(source.staging.capacity(), capacity);
}

#[test]
fn turning_the_tap_off_mid_item_stops_it_collecting() {
    let tap = AnalysisTap::new();
    let mut source = tap.wrap(tone(ANALYSIS_FRAMES * 2, 1));
    tap.set_enabled(true);
    for _ in 0..8 {
        source.next().expect("samples");
    }
    assert_eq!(source.staging.len(), 8);

    tap.set_enabled(false);
    source.next().expect("samples");

    assert!(source.staging.is_empty());
}

#[test]
fn a_seek_throws_away_the_part_block_from_before_it() {
    let tap = AnalysisTap::new();
    let mut source = tap.wrap(tone(ANALYSIS_FRAMES * 2, 1));
    tap.set_enabled(true);
    for _ in 0..8 {
        source.next().expect("samples");
    }

    let _ = source.try_seek(Duration::ZERO);

    assert!(source.staging.is_empty());
}
