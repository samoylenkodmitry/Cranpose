#![cfg(all(not(target_arch = "wasm32"), feature = "all-codecs"))]

use std::{
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use cranpose_audio::backend::{AudioSink, Renderer};
use cranpose_media::SoftwareMediaPlayer;
use cranpose_services::{MediaItem, MediaPlayer};

type Output = Arc<Mutex<Option<Box<dyn Renderer>>>>;
struct Capture(Output);
impl AudioSink for Capture {}
impl Drop for Capture {
    fn drop(&mut self) {
        self.0.lock().expect("output lock").take();
    }
}

#[test]
fn opus_plays_through_the_media_api_without_conversion() {
    play_fixture("tone.opus");
}

#[test]
fn common_audio_containers_play_from_the_original_files() {
    for name in [
        "tone.wav",
        "tone.flac",
        "tone.m4a",
        "tone.aiff",
        "tone.mp2",
        "video.webm",
    ] {
        play_fixture(name);
    }
}

fn play_fixture(name: &str) {
    let output: Output = Arc::new(Mutex::new(None));
    let receiver = Arc::clone(&output);
    let player = SoftwareMediaPlayer::with_output(Arc::new(move |mut renderer| {
        renderer.set_device_format(48_000.0, 2);
        *receiver.lock().expect("output lock") = Some(renderer);
        Ok(Box::new(Capture(Arc::clone(&receiver))))
    }));
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let item = MediaItem::new(cranpose_media::uri_for_path(&fixture));
    player
        .prepare(&item)
        .unwrap_or_else(|error| panic!("open original {name}: {error}"));
    player.play().expect("start Opus playback");
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut block = [0.0_f32; 512];
    loop {
        output
            .lock()
            .expect("output lock")
            .as_mut()
            .expect("output opened")
            .render(&mut block);
        if block.iter().any(|sample| sample.abs() > 0.01) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no decoded audio reached the output"
        );
        thread::sleep(Duration::from_millis(1));
    }
    player.stop();
}
