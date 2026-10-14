//! `ImageLoader` and `rememberAsyncImage` against a local HTTP server: an
//! image decodes at the size it is drawn, the memory and disk caches answer
//! repeated requests, one download serves every request of one image, a
//! request nobody waits for any more stops, and a missing image fails.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, PoisonError},
    time::Duration,
};

use cranpose_core::{Composition, MemoryApplier, location_key};
use cranpose_services::{
    HttpClientRef, ImageCacheLimits, ImageLoadError, ImageLoadState, ImageLoader, ImageRequest,
    default_http_client, rememberAsyncImage,
};

/// Serves `/photo.png`, a 40 × 20 PNG, and a 404 for every other path,
/// counting the requests by path. While `hold` is set, a response waits.
struct Server {
    base: String,
    hits: Arc<Mutex<HashMap<String, usize>>>,
    hold: Arc<(Mutex<bool>, Condvar)>,
}

impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let base = format!("http://{}", listener.local_addr().expect("an address"));
        let hits = Arc::new(Mutex::new(HashMap::new()));
        let hold = Arc::new((Mutex::new(false), Condvar::new()));
        let png = png(40, 20);
        let (counted, held) = (Arc::clone(&hits), Arc::clone(&hold));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (counted, held, png) = (Arc::clone(&counted), Arc::clone(&held), png.clone());
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(&stream);
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() {
                        return;
                    }
                    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                    *lock(&counted).entry(path.clone()).or_default() += 1;
                    let (gate, opened) = &*held;
                    let _open = opened
                        .wait_while(lock(gate), |holding| *holding)
                        .unwrap_or_else(PoisonError::into_inner);
                    let mut stream = &stream;
                    let (status, body): (&str, &[u8]) = if path == "/photo.png" {
                        ("200 OK", &png)
                    } else {
                        ("404 Not Found", b"missing")
                    };
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(body);
                });
            }
        });
        Self { base, hits, hold }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn hits(&self, path: &str) -> usize {
        lock(&self.hits).get(path).copied().unwrap_or(0)
    }

    fn set_hold(&self, holding: bool) {
        let (gate, opened) = &*self.hold;
        *lock(gate) = holding;
        opened.notify_all();
    }
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([200, 30, 30, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("a PNG");
    bytes.into_inner()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn loader(disk: Option<PathBuf>) -> ImageLoader {
    ImageLoader::new(ImageCacheLimits::default(), disk)
}

fn client() -> HttpClientRef {
    default_http_client()
}

fn disk_dir(tag: &str) -> PathBuf {
    cranpose_core::test_scratch_dir(env!("CARGO_MANIFEST_DIR"), tag)
}

#[test]
fn an_image_decodes_at_the_size_it_is_drawn_and_the_memory_cache_keeps_it() {
    let server = Server::start();
    let loader = loader(None);
    let request = ImageRequest::new(server.url("/photo.png")).max_size(20, 20);
    let bitmap = pollster::block_on(loader.load(client(), &request)).expect("the image loads");
    assert_eq!(
        (bitmap.width(), bitmap.height()),
        (20, 10),
        "a 40 × 20 image drawn at most 20 wide keeps its aspect ratio"
    );
    assert_eq!(loader.cached(&request).as_ref(), Some(&bitmap));
    pollster::block_on(loader.load(client(), &request)).expect("the cached image");
    assert_eq!(
        server.hits("/photo.png"),
        1,
        "the memory cache answers the second request"
    );
}

#[test]
fn requests_for_one_image_share_one_download() {
    let server = Server::start();
    let loader = loader(None);
    let request = ImageRequest::new(server.url("/photo.png"));
    server.set_hold(true);
    let first = loader.load(client(), &request);
    let second = loader.load(client(), &request);
    let release = {
        let hold = Arc::clone(&server.hold);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let (gate, opened) = &*hold;
            *lock(gate) = false;
            opened.notify_all();
        })
    };
    let (first, second) = pollster::block_on(futures_util::future::join(first, second));
    let _ = release.join();
    assert_eq!(first.expect("first"), second.expect("second"));
    assert_eq!(server.hits("/photo.png"), 1);
}

#[test]
fn a_new_loader_reads_what_the_last_one_downloaded_from_disk() {
    let server = Server::start();
    let disk = disk_dir("image-disk-cache");
    let request = ImageRequest::new(server.url("/photo.png"));
    pollster::block_on(loader(Some(disk.clone())).load(client(), &request)).expect("download");
    let again = pollster::block_on(loader(Some(disk)).load(client(), &request)).expect("disk");
    assert_eq!((again.width(), again.height()), (40, 20));
    assert_eq!(
        server.hits("/photo.png"),
        1,
        "the second loader read the disk cache"
    );
}

#[test]
fn a_request_nobody_waits_for_stops_and_the_next_one_downloads_again() {
    let server = Server::start();
    let loader = loader(None);
    let request = ImageRequest::new(server.url("/photo.png"));
    server.set_hold(true);
    let mut abandoned = Box::pin(loader.load(client(), &request));
    let waker = futures_util::task::noop_waker();
    let mut context = std::task::Context::from_waker(&waker);
    for _ in 0..200 {
        assert!(abandoned.as_mut().poll(&mut context).is_pending());
        if server.hits("/photo.png") == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(server.hits("/photo.png"), 1, "the first download started");
    drop(abandoned);
    server.set_hold(false);
    pollster::block_on(loader.load(client(), &request)).expect("a new download");
    assert_eq!(
        server.hits("/photo.png"),
        2,
        "the dropped request did not keep its download for the next caller"
    );
}

#[test]
fn a_missing_image_fails_with_its_read_error() {
    let server = Server::start();
    let result = pollster::block_on(
        loader(None).load(client(), &ImageRequest::new(server.url("/missing.png"))),
    );
    assert!(
        matches!(result, Err(ImageLoadError::Read(_))),
        "a 404 is a read error: {result:?}"
    );
}

#[test]
fn bytes_the_app_holds_decode_without_the_network() {
    let request = ImageRequest::bytes("asset:icon", png(8, 8));
    let bitmap = pollster::block_on(loader(None).load(client(), &request)).expect("decoded");
    assert_eq!((bitmap.width(), bitmap.height()), (8, 8));
}

#[test]
fn a_composable_sees_loading_then_the_image() {
    let server = Server::start();
    let url = server.url("/photo.png");
    let captured = std::rc::Rc::new(std::cell::Cell::new(None));
    let installed = std::rc::Rc::clone(&captured);
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), move || {
            installed.set(Some(rememberAsyncImage(
                ImageRequest::new(url.clone()).max_size(10, 10),
            )));
        })
        .expect("the composition renders");
    let state = captured.get().expect("the composable returned its state");
    assert_eq!(state.value(), ImageLoadState::Loading);
    let runtime = composition.runtime_handle();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while state.value() == ImageLoadState::Loading && std::time::Instant::now() < deadline {
        runtime.drain_ui();
        std::thread::yield_now();
    }
    match state.value() {
        ImageLoadState::Ready(bitmap) => assert_eq!((bitmap.width(), bitmap.height()), (10, 5)),
        other => panic!("the composable's image did not load: {other:?}"),
    }
}
