#![cfg(not(target_arch = "wasm32"))]

use std::{
    f32::consts::PI,
    fs,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use cranpose_audio::backend::{AudioSink, Renderer};
use cranpose_media::SoftwareMediaPlayer;
use cranpose_services::{EqualizerSettings, MediaItem, MediaPlayer, MediaSamples};

type Output = Arc<Mutex<Option<Box<dyn Renderer>>>>;
struct Capture(Output);
impl AudioSink for Capture {}
impl Drop for Capture {
    fn drop(&mut self) {
        self.0.lock().expect("capture lock").take();
    }
}

fn wave() -> Vec<u8> {
    let frames = 48_000;
    let size = frames * 4;
    let mut bytes = Vec::with_capacity(44 + size);
    bytes.extend(b"RIFF");
    bytes.extend(((36 + size) as u32).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(48_000_u32.to_le_bytes());
    bytes.extend(192_000_u32.to_le_bytes());
    bytes.extend(4_u16.to_le_bytes());
    bytes.extend(16_u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend((size as u32).to_le_bytes());
    for frame in 0..frames {
        let sample =
            (0.125 * (2.0 * PI * 1_000.0 * frame as f32 / 48_000.0).sin() * 32767.0) as i16;
        bytes.extend(sample.to_le_bytes());
        bytes.extend(sample.to_le_bytes());
    }
    bytes
}

fn output_peak(path: &std::path::Path, balance: f32, gain_db: f32) -> [f32; 2] {
    let analyzed: Arc<Mutex<Option<MediaSamples>>> = Arc::new(Mutex::new(None));
    let received = analyzed.clone();
    let _observer = cranpose_services::observe_media_samples(move |samples| {
        *received.lock().expect("analysis lock") = Some(samples);
    });
    let output: Output = Arc::new(Mutex::new(None));
    let receiver = output.clone();
    let player = SoftwareMediaPlayer::with_output(Arc::new(move |mut renderer| {
        renderer.set_device_format(48_000.0, 2);
        *receiver.lock().expect("capture lock") = Some(renderer);
        Ok(Box::new(Capture(receiver.clone())))
    }));
    assert!(player.set_balance(balance));
    assert!(!player.set_balance(f32::NAN));
    let mut settings = EqualizerSettings {
        enabled: true,
        preamp_db: 0.0,
        gains_db: vec![0.0; 10],
    };
    settings.gains_db[5] = gain_db;
    player.set_equalizer(&settings);
    player.set_analysis_enabled(true);
    player
        .prepare(&MediaItem::new(cranpose_media::uri_for_path(path)))
        .expect("prepare wave");
    player.play().expect("play wave");
    let mut peaks = [0.0_f32; 2];
    let mut heard = 0;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut block = [0.0; 512];
    while heard < 24_000 {
        assert!(Instant::now() < deadline, "no PCM arrived");
        output
            .lock()
            .expect("capture lock")
            .as_mut()
            .expect("renderer")
            .render(&mut block);
        if block.iter().any(|s| *s != 0.0) {
            for frame in block.as_chunks::<2>().0 {
                peaks[0] = peaks[0].max(frame[0].abs());
                peaks[1] = peaks[1].max(frame[1].abs());
            }
            heard += block.len();
        } else {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while analyzed.lock().expect("analysis lock").is_none() {
        assert!(Instant::now() < deadline, "analysis did not publish PCM");
        std::thread::sleep(Duration::from_millis(5));
    }
    let samples = analyzed
        .lock()
        .expect("analysis lock")
        .take()
        .expect("analysis");
    assert_eq!(samples.sample_rate, 48_000);
    assert_eq!(samples.channels, 2);
    assert!(samples.samples.iter().any(|value| value.abs() > 0.05));
    assert!(player.set_balance(-balance));
    output
        .lock()
        .expect("capture lock")
        .as_mut()
        .expect("renderer")
        .render(&mut block);
    if balance != 0.0 {
        let muted_channel = usize::from(balance > 0.0);
        assert!(
            block
                .as_chunks::<2>()
                .0
                .iter()
                .all(|frame| frame[muted_channel] == 0.0)
        );
        assert!(
            block
                .as_chunks::<2>()
                .0
                .iter()
                .any(|frame| frame[1 - muted_channel].abs() > 0.05)
        );
    }
    player.pause();
    player
        .seek_to(Duration::from_millis(200))
        .expect("seek wave");
    player.stop();
    assert!(
        output.lock().expect("capture lock").is_none(),
        "stop must release the device"
    );
    peaks
}

#[test]
fn playback_outputs_real_balance_equalizer_and_analysis_samples() {
    let path = std::env::temp_dir().join(format!("cranpose-processing-{}.wav", std::process::id()));
    fs::write(&path, wave()).expect("write generated test tone");
    let center = output_peak(&path, 0.0, 0.0);
    let left = output_peak(&path, -1.0, 0.0);
    let right = output_peak(&path, 1.0, 0.0);
    let boosted = output_peak(&path, 0.0, 6.0);
    fs::remove_file(path).expect("remove generated test tone");
    assert!((center[0] - center[1]).abs() < 0.001);
    assert!(left[0] > 0.12 && left[1] == 0.0, "left {left:?}");
    assert!(right[1] > 0.12 && right[0] == 0.0, "right {right:?}");
    assert!(
        boosted[0] > center[0] * 1.8 && boosted[0] < center[0] * 2.2,
        "EQ {boosted:?} vs {center:?}"
    );
}
