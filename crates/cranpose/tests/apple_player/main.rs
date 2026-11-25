#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() {
    macos::run_all();
}

#[cfg(target_os = "macos")]
#[path = "../support/player_e2e.rs"]
mod support;

#[cfg(target_os = "macos")]
mod macos {
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
        sync::atomic::Ordering,
        time::{Duration, Instant},
    };

    use cranpose::{
        AudioRole, Color, MediaItem, PlaybackParameters, Player, PlayerState,
        SubtitleConfiguration, TrackType, VideoCodec, VideoContainer, install_player,
        player_capabilities, uri_for_path, video_surface_supported,
    };
    use objc2_foundation::{NSDate, NSRunLoop};

    use super::support::{
        HEIGHT, WIDTH, create_shutter_view as make_shutter_view, pixel_rgba,
        spawn_static_http_server,
    };

    const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

    pub(super) fn run_all() {
        capabilities_probe_reports_containers_and_codecs();
        if let Some(ffmpeg) = ffmpeg_binary() {
            local_mp4_renders_to_metal_content_frame_with_seek_and_subtitles(&ffmpeg);
            http_hls_with_custom_headers_and_in_memory_bytes(&ffmpeg);
            hdr_hevc_pq_and_hlg_render_to_metal(&ffmpeg);
        }
    }

    fn pump_main_runloop(duration: Duration) {
        let until = NSDate::dateWithTimeIntervalSinceNow(duration.as_secs_f64());
        NSRunLoop::mainRunLoop().runUntilDate(&until);
    }

    fn wait_pumping(mut cond: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        while Instant::now() < deadline {
            if cond() {
                return true;
            }
            pump_main_runloop(Duration::from_millis(8));
        }
        cond()
    }

    fn ffmpeg_binary() -> Option<PathBuf> {
        for candidate in [
            "/opt/homebrew/bin/ffmpeg",
            "/usr/local/bin/ffmpeg",
            "ffmpeg",
        ] {
            if Command::new(candidate)
                .arg("-version")
                .output()
                .is_ok_and(|out| out.status.success())
            {
                return Some(PathBuf::from(candidate));
            }
        }
        None
    }

    fn temp_test_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cranpose-apple-player-{tag}-{}",
            std::process::id()
        ));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(&dir).expect("create test media dir");
        dir
    }

    fn generate_two_color_mp4(ffmpeg: &Path, out_path: &Path) {
        let status = Command::new(ffmpeg)
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=0x00ff00:s=320x180:d=1:r=30",
                "-f",
                "lavfi",
                "-i",
                "color=c=0xff0000:s=320x180:d=1:r=30",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-filter_complex",
                "[0:v][1:v]concat=n=2:v=1:a=0[v]",
                "-map",
                "[v]",
                "-map",
                "2:a",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-g",
                "15",
                "-c:a",
                "aac",
                "-movflags",
                "+faststart",
            ])
            .arg(out_path)
            .status()
            .expect("run ffmpeg mp4");
        assert!(status.success(), "ffmpeg mp4 generation failed");
    }

    fn generate_vod_hls(ffmpeg: &Path, dir: &Path) -> PathBuf {
        let playlist = dir.join("stream.m3u8");
        let seg_pattern = dir.join("seg%03d.ts");
        let status = Command::new(ffmpeg)
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=0x0066ff:s=320x180:d=2:r=30",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-g",
                "15",
                "-c:a",
                "aac",
                "-hls_time",
                "1",
                "-hls_playlist_type",
                "vod",
                "-hls_segment_filename",
            ])
            .arg(&seg_pattern)
            .arg(&playlist)
            .status()
            .expect("run ffmpeg hls");
        assert!(status.success(), "ffmpeg hls generation failed");
        playlist
    }

    fn capabilities_probe_reports_containers_and_codecs() {
        install_player();
        let caps = player_capabilities();
        assert!(
            caps.zero_copy_gpu,
            "Apple backend must report zero-copy GPU"
        );
        assert!(caps.hdr_10bit, "Apple backend must report 10-bit HDR");
        assert!(
            caps.supports_container(VideoContainer::Mp4),
            "Apple backend must support MP4"
        );
        assert!(
            caps.supports_container(VideoContainer::FragmentedMp4),
            "Apple backend must support fragmented MP4"
        );
        assert!(
            caps.supports_container(VideoContainer::Hls),
            "Apple backend must support HLS"
        );
        assert!(
            caps.supports_video_codec(VideoCodec::H264),
            "Apple backend must support H.264"
        );
        assert!(
            caps.supports_video_codec(VideoCodec::Hevc),
            "Apple backend must support HEVC"
        );
        assert!(
            caps.supports_hardware_video_codec(VideoCodec::H264),
            "Apple hardware must decode H.264"
        );
    }

    fn create_shutter_view(player: Player) -> cranpose::embedded_view::EmbeddedView {
        make_shutter_view(player, Color::rgb(0.2, 0.2, 0.9))
    }

    fn local_mp4_renders_to_metal_content_frame_with_seek_and_subtitles(ffmpeg: &Path) {
        assert!(
            video_surface_supported(),
            "macOS Metal build must support PlayerSurface"
        );
        install_player();

        let dir = temp_test_dir("mp4");
        let mp4_path = dir.join("clip.mp4");
        let vtt_path = dir.join("subs.vtt");
        generate_two_color_mp4(ffmpeg, &mp4_path);
        fs::write(
            &vtt_path,
            "WEBVTT\n\n00:00:00.000 --> 00:00:00.900\nFirst second green\n\n00:00:01.000 --> 00:00:02.000\nSecond second red\n",
        )
        .expect("write vtt");

        let player = Player::builder()
            .with_audio_role(AudioRole::Muted)
            .build()
            .expect("apple player");
        let item = MediaItem::new(uri_for_path(&mp4_path)).with_subtitle(
            SubtitleConfiguration::new(uri_for_path(&vtt_path))
                .with_mime_type("text/vtt")
                .with_language("en")
                .with_label("English"),
        );
        player.set_media_item(item);

        let mut view = create_shutter_view(player.clone());
        let mut pixels = Vec::new();
        view.draw(&mut pixels).expect("shutter draw");
        let shutter_px = pixel_rgba(&pixels, WIDTH, WIDTH / 2, HEIGHT / 2);
        assert!(
            shutter_px[2] > 180 && shutter_px[1] < 100,
            "expected blue shutter before first video frame, got {shutter_px:?}"
        );

        let started = Instant::now();
        player.prepare();
        player.play();

        let saw_green = wait_pumping(|| {
            view.draw(&mut pixels).expect("video draw");
            let center = pixel_rgba(&pixels, WIDTH, WIDTH / 2, HEIGHT / 2);
            player.rendered_first_frame() && center[1] > 160 && center[0] < 90 && center[2] < 90
        });
        let ttff = started.elapsed();
        assert!(saw_green, "expected Metal zero-copy green frame");
        #[expect(
            clippy::print_stderr,
            reason = "records measured TTFF in harness-less test"
        )]
        {
            eprintln!(
                "apple_player MP4 TTFF: {:.1} ms",
                ttff.as_secs_f64() * 1000.0
            );
        }
        assert!(ttff < Duration::from_secs(3), "TTFF exceeded 3s: {ttff:?}");
        assert_eq!(
            (player.video_size().width, player.video_size().height),
            (WIDTH, HEIGHT),
            "reported video size must match MP4 dimensions"
        );
        let dur = player.duration().expect("mp4 duration");
        assert!(
            dur >= Duration::from_millis(1800) && dur <= Duration::from_millis(2500),
            "expected ~2s MP4 duration, got {dur:?}"
        );
        let tracks = player.current_tracks();
        assert!(
            tracks.contains_type(TrackType::Video),
            "expected video track"
        );
        assert!(
            tracks.contains_type(TrackType::Audio),
            "expected audio track"
        );
        assert!(
            tracks.contains_type(TrackType::Text),
            "expected sidecar text track"
        );

        player.pause();
        player.set_playback_parameters(PlaybackParameters::with_speed(1.5));
        player.seek_to(Duration::from_millis(1400));
        let saw_red = wait_pumping(|| {
            view.draw(&mut pixels).expect("seek draw");
            let center = pixel_rgba(&pixels, WIDTH, WIDTH / 2, HEIGHT / 2);
            center[0] > 160 && center[1] < 90 && center[2] < 90
        });
        assert!(saw_red, "expected red GPU frame after seek to 1.4s");
        assert!(
            wait_pumping(|| {
                player
                    .current_cues()
                    .cues
                    .iter()
                    .any(|cue| cue.text.contains("Second second red"))
            }),
            "expected WebVTT cue for second half after paused seek"
        );

        player.release();
        drop(fs::remove_dir_all(&dir));
    }

    fn http_hls_with_custom_headers_and_in_memory_bytes(ffmpeg: &Path) {
        install_player();
        let dir = temp_test_dir("hls-mem");
        let _playlist = generate_vod_hls(ffmpeg, &dir);
        let (base_url, saw_header, stop) =
            spawn_static_http_server(dir.clone(), "x-cranpose-token: secret-42");

        let hls_player = Player::builder()
            .with_audio_role(AudioRole::Muted)
            .build()
            .expect("hls player");
        hls_player.set_media_item(
            MediaItem::new(format!("{base_url}/stream.m3u8"))
                .with_header("X-Cranpose-Token", "secret-42"),
        );
        hls_player.prepare();
        hls_player.play();

        assert!(
            wait_pumping(|| {
                hls_player.playback_state() == PlayerState::Ready
                    && hls_player
                        .decoded_frames()
                        .and_then(|f| f.latest_frame())
                        .is_some()
            }),
            "expected AVPlayer HTTP HLS playback to decode a zero-copy VideoFrame (state={:?}, err={:?})",
            hls_player.playback_state(),
            hls_player.player_error(),
        );
        assert!(
            saw_header.load(Ordering::Acquire),
            "expected custom HTTP header X-Cranpose-Token on AVURLAsset HLS request"
        );
        hls_player.release();
        stop.store(true, Ordering::Release);

        let mp4_path = dir.join("mem.mp4");
        generate_two_color_mp4(ffmpeg, &mp4_path);
        let bytes = fs::read(&mp4_path).expect("read mp4 bytes");
        let mem_player = Player::builder()
            .with_audio_role(AudioRole::Muted)
            .build()
            .expect("mem player");
        mem_player.set_media_item(MediaItem::from_bytes(bytes, "video/mp4"));
        mem_player.prepare();
        mem_player.play();

        assert!(
            wait_pumping(|| {
                mem_player.playback_state() == PlayerState::Ready
                    && mem_player
                        .decoded_frames()
                        .and_then(|f| f.latest_frame())
                        .is_some()
            }),
            "expected in-memory MediaItem::from_bytes playback to decode a zero-copy VideoFrame (state={:?}, err={:?})",
            mem_player.playback_state(),
            mem_player.player_error(),
        );
        mem_player.release();
        drop(fs::remove_dir_all(&dir));
    }

    fn hdr_hevc_pq_and_hlg_render_to_metal(ffmpeg: &Path) {
        install_player();
        let dir = temp_test_dir("hdr");
        for (label, color_hex, trc, expect_r, expect_b) in [
            ("PQ", "0xff8800", "smpte2084", true, false),
            ("HLG", "0x00ccff", "arib-std-b67", false, true),
        ] {
            let clip_path = dir.join(format!("hdr_{label}.mp4"));
            let status = Command::new(ffmpeg)
                .args([
                    "-y",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    &format!("color=c={color_hex}:s={WIDTH}x{HEIGHT}:d=1:r=30"),
                    "-c:v",
                    "libx265",
                    "-tag:v",
                    "hvc1",
                    "-pix_fmt",
                    "yuv420p10le",
                    "-x265-params",
                    &format!("colorprim=bt2020:transfer={trc}:colormatrix=bt2020nc"),
                    "-color_primaries",
                    "bt2020",
                    "-color_trc",
                    trc,
                    "-colorspace",
                    "bt2020nc",
                    "-movflags",
                    "+faststart",
                ])
                .arg(&clip_path)
                .status()
                .expect("run ffmpeg HDR clip");
            if !status.success() {
                continue;
            }
            let player = Player::builder()
                .with_audio_role(AudioRole::Muted)
                .build()
                .expect("hdr player");
            player.set_media_item(MediaItem::new(uri_for_path(&clip_path)));
            let mut view = create_shutter_view(player.clone());
            let mut pixels = Vec::new();
            player.prepare();
            player.play();
            let mut rendered = [0_u8; 4];
            let ok = wait_pumping(|| {
                view.draw(&mut pixels).expect("hdr draw");
                let center = pixel_rgba(&pixels, WIDTH, WIDTH / 2, HEIGHT / 2);
                rendered = center;
                let matches = if expect_r {
                    center[0] > 140 && center[2] < 120
                } else if expect_b {
                    center[2] > 140 && center[0] < 120
                } else {
                    false
                };
                player.rendered_first_frame() && matches
            });
            assert!(
                ok,
                "Apple Metal HDR {label} 10-bit clip failed to tone-map/render; last center={rendered:?}"
            );
            #[expect(
                clippy::print_stderr,
                reason = "records measured Metal HDR pixel output in harness-less test"
            )]
            {
                eprintln!("apple_player HDR {label} center={rendered:?}");
            }
            player.release();
        }
        drop(fs::remove_dir_all(&dir));
    }
}
