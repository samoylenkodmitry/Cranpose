use super::*;
use crate::source::SamplesBuffer;

fn tone(hz: f32, sample_rate: u32) -> SamplesBuffer {
    let frames = sample_rate as usize;
    let samples: Vec<f32> = (0..frames)
        .map(|frame| (2.0 * PI * hz * frame as f32 / sample_rate as f32).sin())
        .collect();
    SamplesBuffer::new(1, sample_rate, samples)
}

fn settled_peak(samples: &[f32]) -> f32 {
    samples[samples.len() / 2..]
        .iter()
        .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
}

fn run(tap: &Arc<EqualizerTap>, hz: f32, sample_rate: u32) -> Vec<f32> {
    tap.wrap(tone(hz, sample_rate)).collect()
}

#[test]
fn a_disabled_equalizer_passes_every_sample_through_unchanged() {
    let tap = EqualizerTap::new();
    tap.set(false, 0.0, &[12.0; BAND_CENTERS_HZ.len()]);
    let source: Vec<f32> = tone(1_000.0, 44_100).collect();
    let filtered = run(&tap, 1_000.0, 44_100);
    assert_eq!(source, filtered, "a disabled equalizer must be a no-op");
}

#[test]
fn an_enabled_flat_equalizer_leaves_the_level_where_it_was() {
    let tap = EqualizerTap::new();
    tap.set(true, 0.0, &[0.0; BAND_CENTERS_HZ.len()]);
    let filtered = run(&tap, 1_000.0, 44_100);
    let peak = settled_peak(&filtered);
    assert!(
        (peak - 1.0).abs() < 0.02,
        "flat bands changed the level: peak {peak}"
    );
}

#[test]
fn lifting_a_band_lifts_the_frequency_it_is_centred_on() {
    let mut gains = [0.0f32; BAND_CENTERS_HZ.len()];
    gains[5] = 12.0;
    let tap = EqualizerTap::new();
    tap.set(true, 0.0, &gains);

    let lifted = settled_peak(&run(&tap, 1_000.0, 44_100));
    assert!(lifted > 0.98, "the centred band was not lifted: {lifted}");

    let untouched = settled_peak(&run(&tap, 125.0, 44_100));
    assert!(
        untouched < 1.15,
        "a band three octaves away moved: {untouched}"
    );
}

#[test]
fn cutting_a_band_cuts_the_frequency_it_is_centred_on() {
    let mut gains = [0.0f32; BAND_CENTERS_HZ.len()];
    gains[5] = -12.0;
    let tap = EqualizerTap::new();
    tap.set(true, 0.0, &gains);

    let cut = settled_peak(&run(&tap, 1_000.0, 44_100));
    assert!(cut < 0.4, "the centred band was not cut: {cut}");
}

#[test]
fn the_preamp_scales_everything_ahead_of_the_bands() {
    let tap = EqualizerTap::new();
    tap.set(true, -6.0, &[0.0; BAND_CENTERS_HZ.len()]);
    let peak = settled_peak(&run(&tap, 1_000.0, 44_100));
    assert!(
        (peak - 0.5).abs() < 0.03,
        "the preamp did not halve the level: {peak}"
    );
}

#[test]
fn a_band_above_the_nyquist_frequency_is_left_alone() {
    let filter = Biquad::peaking(16_000.0, 22_050.0, 12.0);
    assert_eq!(
        filter,
        Biquad::PASSTHROUGH,
        "a band at or above Nyquist must pass through"
    );
}

#[test]
fn every_reported_band_can_be_lifted_and_cut_by_the_same_amount() {
    let bands = bands();
    assert_eq!(bands.len(), BAND_CENTERS_HZ.len());
    for (band, center) in bands.iter().zip(BAND_CENTERS_HZ.iter()) {
        assert_eq!(band.center_hz, *center);
        assert_eq!(band.max_gain_db, BAND_RANGE_DB);
        assert_eq!(band.min_gain_db, -BAND_RANGE_DB);
        assert_eq!(band.clamp_gain(99.0), BAND_RANGE_DB);
        assert_eq!(band.clamp_gain(-99.0), -BAND_RANGE_DB);
    }
}

#[test]
fn a_new_item_rebuilds_the_curve_for_its_own_sample_rate() {
    let tap = EqualizerTap::new();
    let mut gains = [0.0f32; BAND_CENTERS_HZ.len()];
    gains[9] = 12.0;
    tap.set(true, 0.0, &gains);

    tap.prepare(44_100);
    let wide = tap
        .current_curve()
        .expect("an enabled equalizer has a curve");
    tap.prepare(22_050);
    let narrow = tap
        .current_curve()
        .expect("an enabled equalizer has a curve");

    assert_ne!(wide.filters[9], Biquad::PASSTHROUGH);
    assert_eq!(narrow.filters[9], Biquad::PASSTHROUGH);
}
