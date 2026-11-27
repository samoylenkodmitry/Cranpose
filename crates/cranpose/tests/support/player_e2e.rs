use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use cranpose::{AppLauncher, Box as ComposeBox, BoxSpec, ContentFrame, ContentScale, Modifier};
pub(crate) use cranpose::{
    AudioRole, Color, MediaItem, PlaybackParameters, Player, PlayerState, SubtitleConfiguration,
    TrackType, VideoCodec, VideoContainer, install_player, player_capabilities, uri_for_path,
    video_surface_supported,
};

pub(crate) const WIDTH: u32 = 320;
pub(crate) const HEIGHT: u32 = 180;

pub(crate) fn pixel_rgba(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * width + x) * 4) as usize;
    [
        pixels[idx],
        pixels[idx + 1],
        pixels[idx + 2],
        pixels[idx + 3],
    ]
}

pub(crate) fn create_shutter_view(
    player: Player,
    shutter_color: Color,
) -> cranpose::embedded_view::EmbeddedView {
    AppLauncher::new()
        .create_embedded_view(WIDTH, HEIGHT, 1.0, move || {
            ContentFrame(
                player.clone(),
                Modifier::empty().fill_max_size(),
                ContentScale::FillBounds,
                move || {
                    ComposeBox(
                        Modifier::empty().fill_max_size().background(shutter_color),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        })
        .expect("embedded GPU view")
}

pub(crate) fn spawn_static_http_server(
    root: PathBuf,
    expected_header: &'static str,
) -> (String, Arc<AtomicBool>, Arc<AtomicBool>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local http");
    listener
        .set_nonblocking(true)
        .expect("set nonblocking listener");
    let addr = listener.local_addr().expect("local addr");
    let saw_header = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let saw_clone = Arc::clone(&saw_header);
    let stop_clone = Arc::clone(&stop);
    thread::spawn(move || {
        while !stop_clone.load(Ordering::Acquire) {
            let Ok((mut stream, _)) = listener.accept() else {
                #[expect(
                    clippy::disallowed_methods,
                    reason = "test fixture HTTP server accept poll loop"
                )]
                thread::sleep(Duration::from_millis(5));
                continue;
            };
            serve_http_request(&mut stream, &root, expected_header, &saw_clone);
        }
    });
    (format!("http://{addr}"), saw_header, stop)
}

fn serve_http_request(
    stream: &mut std::net::TcpStream,
    root: &Path,
    expected_header: &str,
    saw_header: &AtomicBool,
) {
    drop(stream.set_read_timeout(Some(Duration::from_secs(2))));
    let mut req_buf = [0_u8; 4096];
    let Ok(read) = stream.read(&mut req_buf) else {
        return;
    };
    let req = String::from_utf8_lossy(&req_buf[..read]);
    if req
        .to_ascii_lowercase()
        .contains(&expected_header.to_ascii_lowercase())
    {
        saw_header.store(true, Ordering::Release);
    }
    let rel = req
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .trim_start_matches('/');
    let Ok(body) = fs::read(root.join(rel)) else {
        drop(stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n"));
        return;
    };
    let mime = if rel.ends_with(".m3u8") {
        "application/vnd.apple.mpegurl"
    } else if rel.ends_with(".ts") {
        "video/mp2t"
    } else if rel.ends_with(".mp4") {
        "video/mp4"
    } else {
        "application/octet-stream"
    };
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    drop(stream.write_all(header.as_bytes()));
    drop(stream.write_all(&body));
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn wait_polling(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if cond() {
            return true;
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "integration test polling off the frame loop"
        )]
        thread::sleep(Duration::from_millis(8));
    }
    cond()
}

#[cfg(not(target_os = "android"))]
pub(crate) fn ffmpeg_binary() -> Option<PathBuf> {
    for candidate in [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
        "ffmpeg",
    ] {
        if std::process::Command::new(candidate)
            .arg("-version")
            .output()
            .is_ok_and(|out| out.status.success())
        {
            return Some(PathBuf::from(candidate));
        }
    }
    None
}

#[cfg(not(target_os = "android"))]
pub(crate) fn temp_test_dir(prefix: &str, tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-output")
        .join(format!("{prefix}-{tag}-{}", std::process::id()));
    drop(fs::remove_dir_all(&dir));
    fs::create_dir_all(&dir).expect("create test media dir");
    dir
}

#[cfg(not(target_os = "android"))]
pub(crate) fn generate_two_color_mp4(ffmpeg: &Path, out_path: &Path) {
    let status = std::process::Command::new(ffmpeg)
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

#[cfg(not(target_os = "android"))]
pub(crate) fn generate_vod_hls(ffmpeg: &Path, dir: &Path) -> PathBuf {
    let playlist = dir.join("stream.m3u8");
    let seg_pattern = dir.join("seg%03d.ts");
    let status = std::process::Command::new(ffmpeg)
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

#[cfg(not(target_os = "android"))]
pub(crate) fn generate_hdr_hevc_mp4(
    ffmpeg: &Path,
    out_path: &Path,
    hex_color: &str,
    transfer: &str,
) -> bool {
    std::process::Command::new(ffmpeg)
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("color=c={hex_color}:s={WIDTH}x{HEIGHT}:d=1:r=30"),
            "-c:v",
            "libx265",
            "-tag:v",
            "hvc1",
            "-pix_fmt",
            "yuv420p10le",
            "-x265-params",
            &format!("colorprim=bt2020:transfer={transfer}:colormatrix=bt2020nc"),
            "-color_primaries",
            "bt2020",
            "-color_trc",
            transfer,
            "-colorspace",
            "bt2020nc",
            "-movflags",
            "+faststart",
        ])
        .arg(out_path)
        .status()
        .is_ok_and(|s| s.success())
}
