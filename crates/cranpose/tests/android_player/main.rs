#[cfg(not(all(
    target_os = "android",
    feature = "android",
    feature = "renderer-wgpu",
    feature = "video"
)))]
fn main() {}

#[cfg(all(
    target_os = "android",
    feature = "android",
    feature = "renderer-wgpu",
    feature = "video"
))]
fn main() {
    android_e2e::run_all();
}

#[cfg(all(
    target_os = "android",
    feature = "android",
    feature = "renderer-wgpu",
    feature = "video"
))]
#[path = "../support/player_e2e.rs"]
mod support;

#[cfg(all(
    target_os = "android",
    feature = "android",
    feature = "renderer-wgpu",
    feature = "video"
))]
mod android_e2e {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::Ordering,
        thread,
        time::{Duration, Instant},
    };

    use cranpose::{
        AudioRole, Color, MediaItem, PlaybackParameters, Player, PlayerState,
        SubtitleConfiguration, TrackType, VideoCodec, VideoContainer, install_player,
        player_capabilities, uri_for_path, video_surface_supported,
    };

    use super::support::{
        HEIGHT, WIDTH, create_shutter_view, pixel_rgba, spawn_static_http_server,
    };

    const WAIT_TIMEOUT: Duration = Duration::from_secs(12);

    struct StderrLogger;

    impl log::Log for StderrLogger {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.level() <= log::Level::Warn
        }

        fn log(&self, record: &log::Record<'_>) {
            if self.enabled(record.metadata()) {
                #[expect(
                    clippy::print_stderr,
                    reason = "surfaces Android warnings in device test"
                )]
                {
                    eprintln!("[{} {}] {}", record.level(), record.target(), record.args());
                }
            }
        }

        fn flush(&self) {}
    }

    static LOGGER: StderrLogger = StderrLogger;

    pub(super) fn run_all() {
        drop(log::set_logger(&LOGGER));
        log::set_max_level(log::LevelFilter::Warn);
        capabilities_probe_reports_containers_and_codecs();
        if let Ok(dir) = std::env::var("CRANPOSE_ANDROID_FIXTURES") {
            let root = PathBuf::from(dir);
            local_mp4_renders_to_vulkan_content_frame_with_seek_and_subtitles(&root);
            http_hls_with_custom_headers_and_in_memory_bytes(&root);
            hdr_hevc_clips_render_non_black(&root);
        }
    }

    fn wait_until(mut cond: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        while Instant::now() < deadline {
            if cond() {
                return true;
            }
            #[expect(
                clippy::disallowed_methods,
                reason = "device integration test polling backend worker completion"
            )]
            thread::sleep(Duration::from_millis(8));
        }
        cond()
    }

    fn capabilities_probe_reports_containers_and_codecs() {
        install_player();
        let caps = player_capabilities();
        assert!(
            caps.zero_copy_gpu,
            "Android backend must report zero_copy_gpu=true"
        );
        assert!(
            caps.supports_container(VideoContainer::Mp4),
            "Android backend must support MP4"
        );
        assert!(
            caps.supports_container(VideoContainer::Hls),
            "Android backend must support HLS"
        );
        assert!(
            caps.supports_container(VideoContainer::Dash),
            "Android backend must support DASH"
        );
        assert!(
            caps.supports_video_codec(VideoCodec::H264),
            "Android backend must support H.264"
        );
        assert!(
            video_surface_supported(),
            "Android wgpu Vulkan video surface must be supported"
        );
        #[expect(
            clippy::print_stderr,
            reason = "reports device probe details in harness-less Android test"
        )]
        {
            eprintln!("PASS capabilities_probe: {caps:?}");
        }
    }

    fn local_mp4_renders_to_vulkan_content_frame_with_seek_and_subtitles(root: &Path) {
        install_player();
        let mp4_path = root.join("two_color.mp4");
        let srt_path = root.join("sample.srt");
        assert!(mp4_path.exists(), "missing fixture {}", mp4_path.display());
        assert!(srt_path.exists(), "missing fixture {}", srt_path.display());

        let player = Player::builder()
            .with_audio_role(AudioRole::Muted)
            .build()
            .expect("build Android player");
        let item = MediaItem::new(uri_for_path(&mp4_path)).with_subtitle(
            SubtitleConfiguration::new(uri_for_path(&srt_path))
                .with_mime_type("application/x-subrip")
                .with_language("en")
                .with_label("English"),
        );

        let mut view = create_shutter_view(player.clone(), Color::rgb(0.0, 0.0, 1.0));
        let mut pixels = Vec::new();
        view.draw(&mut pixels).expect("shutter draw");
        let shutter = pixel_rgba(&pixels, WIDTH, WIDTH / 2, HEIGHT / 2);
        assert!(
            shutter[2] > 180 && shutter[1] < 90,
            "expected blue shutter before first frame, got {shutter:?}"
        );

        let t0 = Instant::now();
        player.set_media_item(item);
        player.prepare();
        player.play();

        assert!(
            wait_until(|| player.playback_state() == PlayerState::Ready),
            "player did not reach Ready; state={:?}, error={:?}",
            player.playback_state(),
            player.player_error()
        );
        assert!(
            player
                .duration()
                .is_some_and(|d| d >= Duration::from_millis(1500)),
            "expected duration >= 1.5s, got {:?}",
            player.duration()
        );
        assert!(
            player.current_tracks().contains_type(TrackType::Video),
            "expected video track group"
        );
        assert!(
            player.current_tracks().contains_type(TrackType::Audio),
            "expected audio track group"
        );
        assert!(
            player.current_tracks().contains_type(TrackType::Text),
            "expected sidecar subtitle track group"
        );

        let mut ttff = None;
        let mut green_pixel = [0u8; 4];
        let saw_green = wait_until(|| {
            view.draw(&mut pixels).expect("video draw");
            let center = pixel_rgba(&pixels, WIDTH, WIDTH / 2, HEIGHT / 2);
            green_pixel = center;
            if player.rendered_first_frame() && center[1] > 160 && center[0] < 90 && center[2] < 90
            {
                ttff = Some(t0.elapsed());
                true
            } else {
                false
            }
        });
        assert!(
            saw_green,
            "first green video frame must render through Vulkan YcbcrConverter; last center={green_pixel:?}, rendered_first_frame={}, has_frames={}, stats={:?}, pos={:?}, state={:?}, err={:?}",
            player.rendered_first_frame(),
            player.decoded_frames().is_some(),
            player.decoded_frames().map(|f| f.stats()),
            player.current_position(),
            player.playback_state(),
            player.player_error(),
        );
        let ttff = ttff.expect("ttff captured");

        let red_pixel = verify_seek_and_subtitles(&player, &mut view, &mut pixels);

        #[expect(
            clippy::print_stderr,
            reason = "records measured TTFF and pixel output in harness-less Android test"
        )]
        {
            eprintln!(
                "PASS local_mp4_vulkan: TTFF={:.1}ms green={green_pixel:?} red={red_pixel:?} size={:?}",
                ttff.as_secs_f64() * 1000.0,
                player.video_size()
            );
        }
        player.release();
    }

    fn verify_seek_and_subtitles(
        player: &Player,
        view: &mut cranpose::embedded_view::EmbeddedView,
        pixels: &mut Vec<u8>,
    ) -> [u8; 4] {
        player.pause();
        player.seek_to(Duration::from_millis(1500));
        let mut red_pixel = [0u8; 4];
        let saw_red = wait_until(|| {
            view.draw(pixels).expect("post-seek draw");
            let center = pixel_rgba(pixels, WIDTH, WIDTH / 2, HEIGHT / 2);
            red_pixel = center;
            center[0] > 160 && center[1] < 90 && center[2] < 90
        });
        assert!(
            saw_red,
            "seek to 1.5s must render red second half; last center={red_pixel:?}"
        );
        assert!(
            wait_until(|| {
                player
                    .current_cues()
                    .cues
                    .iter()
                    .any(|c| c.text == "Red second half")
            }),
            "expected subtitle cue at 1.5s, got {:?}",
            player.current_cues()
        );
        player.set_playback_parameters(PlaybackParameters::with_speed(1.5));
        assert!(
            (player.playback_parameters().speed - 1.5).abs() < 0.01,
            "playback speed did not update"
        );
        red_pixel
    }

    fn http_hls_with_custom_headers_and_in_memory_bytes(root: &Path) {
        install_player();
        let hls_dir = root.join("hls");
        assert!(hls_dir.join("stream.m3u8").exists(), "missing HLS fixture");

        let (base_url, saw_header, stop) =
            spawn_static_http_server(hls_dir, "x-test-auth: bearer-android-2481");
        let hls_player = Player::builder()
            .with_audio_role(AudioRole::Muted)
            .build()
            .expect("build HLS player");
        let hls_item = MediaItem::new(format!("{base_url}/stream.m3u8"))
            .with_mime_type("application/vnd.apple.mpegurl")
            .with_header("X-Test-Auth", "bearer-android-2481");
        hls_player.set_media_item(hls_item);
        hls_player.prepare();
        hls_player.play();

        assert!(
            wait_until(|| hls_player.playback_state() == PlayerState::Ready),
            "HLS player did not reach Ready; state={:?}, error={:?}",
            hls_player.playback_state(),
            hls_player.player_error()
        );
        assert!(
            saw_header.load(Ordering::Acquire),
            "HTTP HLS requests must forward MediaItem::headers"
        );
        let hls_frames = hls_player
            .decoded_frames()
            .expect("HLS player must expose DecodedFrames");
        assert!(
            wait_until(|| hls_frames.next_frame().is_some()),
            "HLS playback must emit zero-copy VideoFrame"
        );
        hls_player.release();
        stop.store(true, Ordering::Release);

        let raw_mp4 = fs::read(root.join("two_color.mp4")).expect("read mp4 bytes");
        let mem_player = Player::builder()
            .with_audio_role(AudioRole::Muted)
            .build()
            .expect("build in-memory player");
        mem_player.set_media_item(MediaItem::from_bytes(raw_mp4, "video/mp4"));
        mem_player.prepare();
        mem_player.play();
        assert!(
            wait_until(|| mem_player.playback_state() == PlayerState::Ready),
            "in-memory MediaItem::from_bytes did not reach Ready; error={:?}",
            mem_player.player_error()
        );
        let mem_frames = mem_player
            .decoded_frames()
            .expect("in-memory player must expose DecodedFrames");
        assert!(
            wait_until(|| mem_frames.next_frame().is_some()),
            "in-memory MediaItem::from_bytes must emit VideoFrame"
        );
        mem_player.release();
        #[expect(
            clippy::print_stderr,
            reason = "reports device test pass in harness-less Android test"
        )]
        {
            eprintln!("PASS http_hls_and_memory_bytes");
        }
    }

    fn hdr_hevc_clips_render_non_black(root: &Path) {
        install_player();
        for (label, file_name) in [("PQ", "hdr_pq.mp4"), ("HLG", "hdr_hlg.mp4")] {
            let path = root.join(file_name);
            if !path.exists() {
                continue;
            }
            let player = Player::builder()
                .with_audio_role(AudioRole::Muted)
                .build()
                .expect("build HDR player");
            player.set_media_item(MediaItem::new(uri_for_path(&path)));
            player.prepare();
            player.play();
            assert!(
                wait_until(|| player.playback_state() == PlayerState::Ready),
                "HDR {label} player did not reach Ready; error={:?}",
                player.player_error()
            );

            let mut view = create_shutter_view(player.clone(), Color::rgb(0.0, 0.0, 0.0));
            let mut pixels = Vec::new();
            let mut rendered_px = [0u8; 4];
            let ok = wait_until(|| {
                view.draw(&mut pixels).expect("HDR draw");
                let center = pixel_rgba(&pixels, WIDTH, WIDTH / 2, HEIGHT / 2);
                rendered_px = center;
                player.rendered_first_frame()
                    && (center[0] > 80 || center[1] > 80 || center[2] > 80)
            });
            assert!(
                ok,
                "HDR {label} HEVC 10-bit clip failed to tone-map/render; last center={rendered_px:?}"
            );
            #[expect(
                clippy::print_stderr,
                reason = "reports HDR pixel verification in harness-less Android test"
            )]
            {
                eprintln!("PASS hdr_{label}: center={rendered_px:?}");
            }
            player.release();
        }
    }
}
