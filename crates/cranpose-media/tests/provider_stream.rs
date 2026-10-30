#![cfg(all(unix, not(target_arch = "wasm32"), feature = "all-codecs"))]

use std::{
    fs::File,
    io::{self, Write},
    os::{fd::OwnedFd, unix::net::UnixStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use cranpose_audio::backend::{AudioSink, Renderer};
use cranpose_media::SoftwareMediaPlayer;
use cranpose_services::{
    MediaItem, MediaPlayer, MediaSourceHandle, MediaSourceOpener,
    clear_platform_media_source_opener, set_platform_media_source_opener,
};

struct Output {
    _renderer: Mutex<Box<dyn Renderer>>,
}
impl AudioSink for Output {}
struct Provider(Arc<AtomicUsize>);
impl MediaSourceOpener for Provider {
    fn open(&self, _: &str) -> io::Result<MediaSourceHandle> {
        let (reader, mut writer) = UnixStream::pair()?;
        let transferred = Arc::clone(&self.0);
        thread::spawn(move || {
            let data_len = 32_u32 * 1024 * 1024;
            let mut header = Vec::from(b"RIFF".as_slice());
            header.extend_from_slice(&(36 + data_len).to_le_bytes());
            header.extend_from_slice(b"WAVEfmt ");
            header.extend_from_slice(&16_u32.to_le_bytes());
            header.extend_from_slice(&1_u16.to_le_bytes());
            header.extend_from_slice(&2_u16.to_le_bytes());
            header.extend_from_slice(&48_000_u32.to_le_bytes());
            header.extend_from_slice(&192_000_u32.to_le_bytes());
            header.extend_from_slice(&4_u16.to_le_bytes());
            header.extend_from_slice(&16_u16.to_le_bytes());
            header.extend_from_slice(b"data");
            header.extend_from_slice(&data_len.to_le_bytes());
            if writer.write_all(&header).is_err() {
                return;
            }
            let block = [1_u8; 16 * 1024];
            let data_len = usize::try_from(data_len).expect("fixture size fits usize");
            for _ in 0..data_len / block.len() {
                if writer.write_all(&block).is_err() {
                    break;
                }
                transferred.fetch_add(block.len(), Ordering::Relaxed);
            }
        });
        Ok(MediaSourceHandle::new(
            File::from(OwnedFd::from(reader)),
            Some(32 * 1024 * 1024 + 44),
        ))
    }
}

struct Registration;
impl Drop for Registration {
    fn drop(&mut self) {
        clear_platform_media_source_opener();
    }
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test observes a paused worker outside the frame loop"
)]
fn a_paused_provider_stream_does_not_download_the_whole_track() {
    cranpose_services::set_application_id("org.cranpose.provider-stream-test")
        .expect("application id");
    let transferred = Arc::new(AtomicUsize::new(0));
    set_platform_media_source_opener(Arc::new(Provider(Arc::clone(&transferred))));
    let _registration = Registration;
    let player = SoftwareMediaPlayer::with_output(Arc::new(|mut renderer| {
        renderer.set_device_format(48_000.0, 2);
        Ok(Box::new(Output {
            _renderer: Mutex::new(renderer),
        }))
    }));
    player
        .prepare(&MediaItem::new("content://test/audio.wav"))
        .expect("prepare a provider pipe");
    assert!(
        !player.capabilities().seeking,
        "a provider pipe must not advertise seeking"
    );
    thread::sleep(Duration::from_millis(250));
    let read = transferred.load(Ordering::Relaxed);
    player.stop();
    assert!(
        read < 1024 * 1024,
        "paused playback downloaded {read} bytes"
    );
}
