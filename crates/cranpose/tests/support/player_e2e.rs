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

use cranpose::{
    AppLauncher, Box as ComposeBox, BoxSpec, Color, ContentFrame, ContentScale, Modifier, Player,
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
