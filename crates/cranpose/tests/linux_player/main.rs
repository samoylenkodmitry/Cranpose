#[macro_use]
#[path = "../../../coroflow/tests/support/main_thread_test.rs"]
mod main_thread_test;

#[cfg(not(target_os = "linux"))]
fn linux_player_e2e() {}

#[cfg(target_os = "linux")]
fn linux_player_e2e() {
    linux::run_all();
}

main_thread_test!(linux_player_e2e);

#[cfg(target_os = "linux")]
#[path = "../support/player_e2e.rs"]
mod support;

#[cfg(target_os = "linux")]
#[expect(
    clippy::print_stderr,
    reason = "records measured TTFF and pixel readbacks in harness-less test"
)]
mod linux {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::Ordering,
        thread,
        time::{Duration, Instant},
    };

    use super::support::{
        AudioRole, Color, HEIGHT, MediaItem, PlaybackParameters, Player, PlayerState,
        SubtitleConfiguration, TrackType, VideoCodec, VideoContainer, WIDTH,
        create_shutter_view as make_shutter_view, ffmpeg_binary, generate_hdr_hevc_mp4,
        generate_two_color_mp4, generate_vod_hls, install_player, pixel_rgba, player_capabilities,
        spawn_static_http_server, temp_test_dir as make_temp_test_dir, uri_for_path,
        video_surface_supported, wait_polling as wait_polling_timeout,
    };

    const WAIT_TIMEOUT: Duration = Duration::from_secs(12);

    pub(super) fn run_all() {
        capabilities_probe_reports_containers_and_codecs();
        if let Some(ffmpeg) = ffmpeg_binary() {
            local_mp4_renders_to_vulkan_content_frame_with_seek_and_subtitles(&ffmpeg);
            http_hls_with_custom_headers_and_in_memory_bytes(&ffmpeg);
            hdr_hevc_pq_and_hlg_render_to_vulkan(&ffmpeg);
        }
    }

    fn wait_polling(cond: impl FnMut() -> bool) -> bool {
        wait_polling_timeout(WAIT_TIMEOUT, cond)
    }

    fn temp_test_dir(tag: &str) -> PathBuf {
        make_temp_test_dir("cranpose-linux-player", tag)
    }

    fn render_until(
        view: &mut cranpose::embedded_view::EmbeddedView,
        mut pred: impl FnMut(&[u8]) -> bool,
    ) -> Option<Vec<u8>> {
        let mut pixels = vec![0_u8; (WIDTH * HEIGHT * 4) as usize];
        let deadline = Instant::now() + WAIT_TIMEOUT;
        while Instant::now() < deadline {
            if view.draw(&mut pixels).is_ok() && pred(&pixels) {
                return Some(pixels);
            }
            #[expect(
                clippy::disallowed_methods,
                reason = "integration test render poll off the frame loop"
            )]
            thread::sleep(Duration::from_millis(16));
        }
        None
    }

    fn capabilities_probe_reports_containers_and_codecs() {
        install_player();
        let caps = player_capabilities();
        assert!(
            caps.supported_containers.contains(&VideoContainer::Mp4),
            "Linux player should report MP4 support: {caps:?}"
        );
        assert!(
            caps.supported_containers.contains(&VideoContainer::Hls),
            "Linux player should report HLS support: {caps:?}"
        );
        assert!(
            caps.supported_containers.contains(&VideoContainer::Dash),
            "Linux player should report DASH support: {caps:?}"
        );
        assert!(
            caps.supported_video_codecs.contains(&VideoCodec::H264),
            "Linux player should report H.264 decode support: {caps:?}"
        );
        eprintln!("linux_player capabilities: {caps:?}");
    }

    fn local_mp4_renders_to_vulkan_content_frame_with_seek_and_subtitles(ffmpeg: &Path) {
        install_player();
        assert!(
            video_surface_supported(),
            "Linux renderer-wgpu + video must report video_surface_supported"
        );
        let dir = temp_test_dir("mp4");
        let mp4 = dir.join("two_color.mp4");
        generate_two_color_mp4(ffmpeg, &mp4);

        let srt = dir.join("sample.srt");
        fs::write(
            &srt,
            "1\n00:00:00,000 --> 00:00:00,900\nFirst Green Caption\n\n\
             2\n00:00:01,000 --> 00:00:02,000\nSecond Red Caption\n",
        )
        .expect("write srt");

        let item = MediaItem::new(uri_for_path(&mp4)).with_subtitle(
            SubtitleConfiguration::new(uri_for_path(&srt))
                .with_mime_type("application/x-subrip")
                .with_language("en")
                .with_label("English"),
        );

        let player = Player::builder()
            .with_audio_role(AudioRole::Muted)
            .build()
            .expect("build Linux player");
        player.set_media_item(item);

        let mut view = make_shutter_view(player.clone(), Color(0.0, 0.0, 1.0, 1.0));
        let mut shutter = vec![0_u8; (WIDTH * HEIGHT * 4) as usize];
        view.draw(&mut shutter).expect("render shutter");
        let shutter_px = pixel_rgba(&shutter, WIDTH, WIDTH / 2, HEIGHT / 2);
        assert!(
            shutter_px[2] > 200 && shutter_px[0] < 30 && shutter_px[1] < 30,
            "expected blue shutter before prepare, got {shutter_px:?}"
        );

        let start = Instant::now();
        player.set_playback_parameters(PlaybackParameters::with_speed(1.0));
        player.prepare();
        player.play();

        let first_frame = render_until(&mut view, |px| {
            let c = pixel_rgba(px, WIDTH, WIDTH / 2, HEIGHT / 2);
            c[1] > 180 && c[0] < 80 && c[2] < 80
        })
        .expect("expected green MP4 first frame on Linux Vulkan PlayerSurface");
        let ttff = start.elapsed();
        let green = pixel_rgba(&first_frame, WIDTH, WIDTH / 2, HEIGHT / 2);
        eprintln!(
            "linux_player MP4 TTFF: {:.1} ms, green={green:?}",
            ttff.as_secs_f64() * 1000.0
        );

        assert!(
            wait_polling(|| player.playback_state() == PlayerState::Ready),
            "expected PlayerState::Ready, got {:?}",
            player.playback_state()
        );
        let tracks = player.current_tracks();
        assert!(
            tracks.contains_type(TrackType::Video),
            "expected video track in Tracks"
        );
        assert!(
            tracks.contains_type(TrackType::Text),
            "expected sidecar subtitle track in Tracks"
        );

        player.seek_to(Duration::from_millis(1300));
        let after_seek = render_until(&mut view, |px| {
            let c = pixel_rgba(px, WIDTH, WIDTH / 2, HEIGHT / 2);
            c[0] > 180 && c[1] < 80 && c[2] < 80
        })
        .expect("expected red frame after seeking to 1.3s");
        let red = pixel_rgba(&after_seek, WIDTH, WIDTH / 2, HEIGHT / 2);
        eprintln!("linux_player seek red={red:?}");

        assert!(
            wait_polling(|| {
                player
                    .current_cues()
                    .cues
                    .iter()
                    .any(|c| c.text.contains("Second Red Caption"))
            }),
            "expected sidecar SRT cue after seek, got {:?}",
            player.current_cues()
        );

        player.release();
        drop(fs::remove_dir_all(&dir));
    }

    fn http_hls_with_custom_headers_and_in_memory_bytes(ffmpeg: &Path) {
        install_player();
        let dir = temp_test_dir("hls");
        let _playlist = generate_vod_hls(ffmpeg, &dir);
        let (base_url, saw_header, stop) =
            spawn_static_http_server(dir.clone(), "x-test-auth: bearer-linux-2481");

        let player = Player::builder()
            .with_audio_role(AudioRole::Muted)
            .build()
            .expect("build HLS player");
        let item = MediaItem::new(format!("{base_url}/stream.m3u8"))
            .with_header("X-Test-Auth", "bearer-linux-2481");
        player.set_media_item(item);
        let mut view = make_shutter_view(player.clone(), Color(1.0, 0.0, 0.0, 1.0));
        player.prepare();
        player.play();

        let hls_frame = render_until(&mut view, |px| {
            let c = pixel_rgba(px, WIDTH, WIDTH / 2, HEIGHT / 2);
            c[2] > 170 && c[0] < 90
        })
        .unwrap_or_else(|| {
            panic!(
                "expected blue HTTP HLS frame: state={:?}, error={:?}, size={:?}, first_frame={}, saw_header={}",
                player.playback_state(),
                player.player_error(),
                player.video_size(),
                player.rendered_first_frame(),
                saw_header.load(Ordering::Acquire),
            )
        });
        let blue = pixel_rgba(&hls_frame, WIDTH, WIDTH / 2, HEIGHT / 2);
        eprintln!("linux_player HTTP HLS blue={blue:?}");
        assert!(
            saw_header.load(Ordering::Acquire),
            "HTTP HLS requests must forward custom MediaItem::headers"
        );
        player.release();
        stop.store(true, Ordering::Release);

        // In-memory bytes playback (`MediaItem::from_bytes`, zero temp files on disk).
        let mp4 = dir.join("mem.mp4");
        generate_two_color_mp4(ffmpeg, &mp4);
        let raw = fs::read(&mp4).expect("read mem mp4");
        let mem_player = Player::builder()
            .with_audio_role(AudioRole::Muted)
            .build()
            .expect("build memory player");
        mem_player.set_media_item(MediaItem::from_bytes(raw, "video/mp4"));
        let mut mem_view = make_shutter_view(mem_player.clone(), Color(0.0, 0.0, 1.0, 1.0));
        mem_player.prepare();
        mem_player.play();
        let mem_frame = render_until(&mut mem_view, |px| {
            let c = pixel_rgba(px, WIDTH, WIDTH / 2, HEIGHT / 2);
            c[1] > 180 && c[0] < 80 && c[2] < 80
        })
        .expect("expected green frame from MediaItem::from_bytes");
        let mem_green = pixel_rgba(&mem_frame, WIDTH, WIDTH / 2, HEIGHT / 2);
        eprintln!("linux_player MediaItem::from_bytes green={mem_green:?}");
        mem_player.release();
        drop(fs::remove_dir_all(&dir));
    }

    fn hdr_hevc_pq_and_hlg_render_to_vulkan(ffmpeg: &Path) {
        install_player();
        let caps = player_capabilities();
        if !caps.supported_video_codecs.contains(&VideoCodec::Hevc) {
            return;
        }
        let dir = temp_test_dir("hdr");
        for (tag, hex, transfer, check) in [
            (
                "pq",
                "0xff0000",
                "smpte2084",
                (|c: [u8; 4]| c[0] > 180 && c[1] < 90 && c[2] < 90) as fn([u8; 4]) -> bool,
            ),
            (
                "hlg",
                "0x00ffff",
                "arib-std-b67",
                (|c: [u8; 4]| c[1] > 160 && c[2] > 160 && c[0] < 100) as fn([u8; 4]) -> bool,
            ),
        ] {
            let path = dir.join(format!("hdr_{tag}.mp4"));
            assert!(
                generate_hdr_hevc_mp4(ffmpeg, &path, hex, transfer),
                "ffmpeg HDR ({transfer}) generation failed"
            );
            let player = Player::builder()
                .with_audio_role(AudioRole::Muted)
                .build()
                .expect("build HDR player");
            player.set_media_item(MediaItem::new(uri_for_path(&path)));
            let mut view = make_shutter_view(player.clone(), Color(0.0, 0.0, 0.0, 1.0));
            player.prepare();
            player.play();
            let pixels = render_until(&mut view, |px| {
                check(pixel_rgba(px, WIDTH, WIDTH / 2, HEIGHT / 2))
            })
            .unwrap_or_else(|| panic!("expected HDR {tag} frame on Linux Vulkan PlayerSurface"));
            let center = pixel_rgba(&pixels, WIDTH, WIDTH / 2, HEIGHT / 2);
            eprintln!("linux_player HDR {tag} center={center:?}");
            player.release();
        }
        drop(fs::remove_dir_all(&dir));
    }
}
