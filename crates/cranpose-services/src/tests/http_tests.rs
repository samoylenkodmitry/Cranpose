#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicU64, Ordering};
use std::{cell::RefCell, rc::Rc};

use cranpose_core::CompositionLocalProvider;

use super::*;
use crate::run_test_composition;

#[test]
fn a_response_carries_what_a_resumed_download_needs_to_know() {
    let response = HttpResponse::empty("https://host/big.bin", 206)
        .with_content_length(Some(4_096))
        .with_resumed(true);
    assert_eq!(response.content_length, Some(4_096));
    assert!(response.resumed);
    assert!(response.is_success());

    let restarted = HttpResponse::empty("https://host/big.bin", 200)
        .with_content_length(None)
        .with_resumed(false);
    assert_eq!(restarted.content_length, None);
    assert!(!restarted.resumed);
    assert!(restarted.is_success());
}

#[test]
fn the_stub_client_answers_every_url_with_the_same_body() {
    let client = StubHttpClient::with_body(b"pong".to_vec());
    for url in ["https://host/a", "https://host/b?query=1"] {
        let request = HttpRequest::get(url);
        let response = pollster::block_on(client.send(&request, HttpControl::new()))
            .expect("the stub answers every request");
        assert_eq!(response.status, 200);
        assert!(response.is_success());
        assert_eq!(response.url, url);
        let body = pollster::block_on(response.read_all()).expect("body");
        assert_eq!(body.as_slice(), b"pong");
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
use std::thread;

struct TestHttpClient;

impl HttpClient for TestHttpClient {
    fn send<'a>(
        &'a self,
        request: &'a HttpRequest,
        _control: HttpControl,
    ) -> HttpFuture<'a, HttpResponse> {
        Box::pin(async move {
            Ok(HttpResponse::new(
                request.url.clone(),
                200,
                Arc::new(BytesBody::new("ok")),
            ))
        })
    }
}

#[test]
fn a_control_shares_cancellation_and_reports_progress() {
    let reported = Arc::new(AtomicU64::new(0));
    let recorder = Arc::clone(&reported);
    let control = HttpControl::new().with_progress(move |progress| {
        assert_eq!(progress.total, Some(20));
        recorder.store(progress.transferred, Ordering::Release);
    });
    let clone = control.clone();
    control.report(HttpProgress {
        transferred: 12,
        total: Some(20),
    });
    assert_eq!(reported.load(Ordering::Acquire), 12);
    assert!(!clone.is_cancelled());
    control.cancel();
    assert!(
        clone.is_cancelled(),
        "a control handed to a transfer must see the cancellation the caller made"
    );
}

#[test]
fn progress_reports_a_fraction_only_when_the_server_said_how_large_it_is() {
    assert_eq!(
        HttpProgress {
            transferred: 5,
            total: Some(20)
        }
        .fraction(),
        Some(0.25)
    );
    assert_eq!(
        HttpProgress {
            transferred: 5,
            total: None
        }
        .fraction(),
        None,
        "a chunked response has no total, and inventing one is lying about it"
    );
    assert_eq!(
        HttpProgress {
            transferred: 5,
            total: Some(0)
        }
        .fraction(),
        None
    );
    assert!(
        HttpProgress {
            transferred: 20,
            total: Some(20)
        }
        .is_complete()
    );
}

#[test]
fn a_request_carries_what_it_asks_for() {
    let request = HttpRequest::get("https://example.test/thing")
        .method(HttpMethod::Post)
        .header("Accept", "application/json")
        .body(b"payload".to_vec())
        .resume_from(4096);
    assert_eq!(request.method.name(), "POST");
    assert_eq!(
        request.headers,
        vec![("Accept".to_string(), "application/json".to_string())]
    );
    assert_eq!(request.body.as_deref(), Some(b"payload".as_slice()));
    assert_eq!(request.resume_from, Some(4096));
    assert_eq!(
        HttpRequest::get("https://example.test/thing")
            .resume_from(0)
            .resume_from,
        None,
        "resuming from the beginning is not resuming"
    );
}

#[test]
fn a_response_reads_its_headers_without_regard_to_case() {
    let response = HttpResponse::empty("https://example.test", 200).with_headers(vec![
        ("content-type".to_string(), "text/plain".to_string()),
        ("etag".to_string(), "\"abc\"".to_string()),
    ]);
    assert_eq!(response.header("Content-Type"), Some("text/plain"));
    assert_eq!(response.header("ETAG"), Some("\"abc\""));
    assert_eq!(response.header("missing"), None);
    assert!(response.is_success());
}

#[test]
fn a_failing_status_is_an_error_the_caller_can_stop_on() {
    let response = HttpResponse::empty("https://example.test", 404);
    assert!(!response.is_success());
    let error = response
        .error_for_status()
        .err()
        .expect("404 is not success");
    assert!(matches!(error, HttpError::HttpStatus { status: 404, .. }));
}

#[test]
fn a_body_read_in_chunks_reassembles_to_what_was_sent() {
    let response = HttpResponse::new(
        "https://example.test",
        200,
        Arc::new(BytesBody::chunked("cranpose streams bodies", 4)),
    );
    let mut chunks = Vec::new();
    while let Some(chunk) = pollster::block_on(response.read_chunk()).expect("a chunk") {
        assert!(chunk.len() <= 4, "a chunked body honours its chunk size");
        chunks.push(chunk);
    }
    assert!(chunks.len() > 1, "the body arrived in pieces");
    let joined = chunks.concat();
    assert_eq!(
        String::from_utf8(joined).expect("text"),
        "cranpose streams bodies"
    );
}

#[test]
fn reading_a_body_as_text_rejects_bytes_that_are_not_text() {
    let response = HttpResponse::new(
        "https://example.test",
        200,
        Arc::new(BytesBody::new(vec![0xff, 0xfe])),
    );
    assert!(matches!(
        pollster::block_on(response.read_text()),
        Err(HttpError::InvalidResponse { .. })
    ));
}

#[test]
fn an_empty_body_ends_immediately() {
    let response = HttpResponse::empty("https://example.test", 204);
    assert_eq!(
        pollster::block_on(response.read_all()).expect("an empty body reads"),
        Vec::<u8>::new()
    );
}

#[test]
fn default_http_client_is_available() {
    let client = default_http_client();
    let cloned = client.clone();
    assert_eq!(Arc::strong_count(&client), 2);
    drop(cloned);
    assert_eq!(Arc::strong_count(&client), 1);
}

#[test]
fn the_native_transfer_awaits_its_response_rather_than_blocking_on_it() {
    let source = include_str!("../http.rs");
    let send = source
        .split("async fn send_native(")
        .nth(1)
        .expect("the native send");
    let body = send.split("\nasync fn ").next().unwrap_or(send);
    assert!(
        body.contains("head_signal\n        .wait()\n        .await"),
        "the native send must await the response head"
    );
    let blocking = ["pollster", "::", "block_on"].concat();
    assert!(
        !body.contains(&blocking),
        "the native send must not block the thread that polled it"
    );
}

#[test]
fn default_http_client_has_no_process_global_native_client_cache() {
    let source = include_str!("../http.rs");
    let once_lock = ["Once", "Lock"].concat();
    let static_client = ["static ", "CLIENT"].concat();
    let native_client_fn = ["fn ", "native_client()"].concat();

    assert!(
        !source.contains(&static_client)
            && !source.contains(&native_client_fn)
            && !source.contains(&once_lock),
        "native HTTP client state must be owned by DefaultHttpClient instead of a process-global cache"
    );
}

#[test]
fn every_convenience_reads_the_body_the_backend_produced() {
    let client = TestHttpClient;
    assert_eq!(
        pollster::block_on(client.get_bytes("https://example.com")).expect("bytes"),
        b"ok".to_vec()
    );
    assert_eq!(
        pollster::block_on(client.get_text("https://example.com")).expect("text"),
        "ok"
    );
}

#[test]
fn map_ordered_concurrent_preserves_input_order() {
    let inputs = [3usize, 1, 4, 1, 5];
    let outputs = pollster::block_on(map_ordered_concurrent(&inputs, 2, |value| async move {
        value * 10
    }))
    .expect("ordered concurrent mapping");

    assert_eq!(outputs, vec![30, 10, 40, 10, 50]);
}

#[test]
fn native_ordered_concurrency_is_not_tied_to_native_http_client_feature() {
    let source = include_str!("../http.rs");
    assert!(
        source
            .contains("#[cfg(not(target_arch = \"wasm32\"))]\npub async fn map_ordered_concurrent"),
        "native ordered concurrency must stay available without the HTTP client feature"
    );
    assert!(
        !source.contains(
            "all(not(target_arch = \"wasm32\"), feature = \"http-native\")]\npub async fn map_ordered_concurrent"
        ) && !source.contains(
            "all(not(target_arch = \"wasm32\"), not(feature = \"http-native\"))]\npub async fn map_ordered_concurrent"
        ),
        "native ordered concurrency must not split into HTTP-enabled and HTTP-disabled behavior"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn map_ordered_concurrent_reports_worker_panic() {
    let inputs = [1usize];
    let should_panic = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let should_panic_for_task = Arc::clone(&should_panic);

    let error = pollster::block_on(map_ordered_concurrent(&inputs, 1, move |_| {
        let should_panic = Arc::clone(&should_panic_for_task);
        async move {
            assert!(
                !should_panic.load(std::sync::atomic::Ordering::SeqCst),
                "test worker panic"
            );
            1usize
        }
    }))
    .expect_err("worker panic should be reported");

    assert!(matches!(error, HttpError::WorkerPanicked { .. }));
}

#[test]
fn local_http_client_can_be_overridden() {
    let local = local_http_client();
    let default_client = default_http_client();
    let custom_client: HttpClientRef = Arc::new(TestHttpClient);
    let captured = Rc::new(RefCell::new(None));

    {
        let captured_for_closure = Rc::clone(&captured);
        let custom_client = custom_client.clone();
        let local_for_provider = local.clone();
        let local_for_read = local.clone();
        run_test_composition(move || {
            let captured = Rc::clone(&captured_for_closure);
            let local_for_read = local_for_read.clone();
            CompositionLocalProvider(
                vec![local_for_provider.provides(custom_client.clone())],
                move || {
                    let current = local_for_read.current();
                    *captured.borrow_mut() = Some(current);
                },
            );
        });
    }

    let current = captured.borrow().as_ref().expect("client captured").clone();
    assert!(Arc::ptr_eq(&current, &custom_client));
    assert!(!Arc::ptr_eq(&current, &default_client));
}

#[cfg(all(not(target_arch = "wasm32"), not(feature = "http-native")))]
#[test]
fn default_http_client_reports_disabled_native_http_feature() {
    let error = pollster::block_on(default_http_client().get_text("https://example.com"))
        .expect_err("native HTTP should be feature-gated");

    assert!(matches!(
        error,
        HttpError::UnsupportedFeature {
            feature: "http-native",
            ..
        }
    ));
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
#[test]
fn native_http_client_builds() {
    build_native_client().expect("native HTTP client should initialize");
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
#[test]
fn certificates_from_der_chain_accepts_valid_roots() {
    let certificates = certificates_from_der_chain(
        webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .take(3)
            .map(AsRef::as_ref),
    )
    .expect("root certificates should parse");

    assert_eq!(certificates.len(), 3);
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
fn local_server(
    body: Vec<u8>,
    chunk: usize,
    delay: std::time::Duration,
    supports_range: bool,
) -> Option<(String, thread::JoinHandle<()>)> {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipping local HTTP server bind in restricted environment: {error}");
            return None;
        }
        Err(error) => panic!("bind local test server: {error}"),
    };
    let address = listener.local_addr().expect("local test server address");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept local test request");
        let mut request = [0u8; 2048];
        let read = stream.read(&mut request).expect("read local test request");
        let request = String::from_utf8_lossy(&request[..read]).to_string();

        let range_from = supports_range
            .then(|| {
                request
                    .lines()
                    .find(|line| line.to_ascii_lowercase().starts_with("range:"))
                    .and_then(|line| line.split("bytes=").nth(1))
                    .and_then(|value| value.trim_end_matches('-').trim().parse::<u64>().ok())
            })
            .flatten();

        let payload = match range_from {
            Some(offset) if (offset as usize) < body.len() => &body[offset as usize..],
            Some(_) => &body[body.len()..],
            None => &body[..],
        };
        let status = if range_from.is_some() {
            "206 Partial Content"
        } else {
            "200 OK"
        };
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        )
        .expect("write local test response head");
        for piece in payload.chunks(chunk.max(1)) {
            if stream.write_all(piece).is_err() {
                return;
            }
            let _ = stream.flush();
            if !delay.is_zero() {
                thread::sleep(delay);
            }
        }
    });
    Some((format!("http://{address}"), handle))
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
#[test]
fn a_native_body_arrives_in_pieces() {
    let body = vec![b'x'; 200 * 1024];
    let Some((url, server)) = local_server(
        body.clone(),
        16 * 1024,
        std::time::Duration::from_millis(5),
        false,
    ) else {
        return;
    };

    let client = default_http_client();
    let response = pollster::block_on(client.send(&HttpRequest::get(&url), HttpControl::new()))
        .expect("a response");
    assert!(response.is_success());
    assert_eq!(response.content_length, Some(body.len() as u64));

    let mut received = Vec::new();
    let mut chunks = 0usize;
    while let Some(chunk) = pollster::block_on(response.read_chunk()).expect("a chunk") {
        chunks += 1;
        received.extend_from_slice(&chunk);
    }
    server.join().expect("the local server finishes");
    assert_eq!(received.len(), body.len());
    assert!(
        chunks > 1,
        "a 200 KiB body must arrive in more than one piece, saw {chunks}"
    );
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
#[test]
fn a_native_transfer_reports_progress_as_it_runs() {
    let body = vec![b'y'; 200 * 1024];
    let Some((url, server)) = local_server(
        body.clone(),
        16 * 1024,
        std::time::Duration::from_millis(2),
        false,
    ) else {
        return;
    };

    let reports = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = Arc::clone(&reports);
    let control = HttpControl::new().with_progress(move |progress| {
        recorder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(progress);
    });

    let client = default_http_client();
    let response =
        pollster::block_on(client.send(&HttpRequest::get(&url), control)).expect("a response");
    let received = pollster::block_on(response.read_all()).expect("the body");
    server.join().expect("the local server finishes");

    assert_eq!(received.len(), body.len());
    let reports = reports
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert!(
        reports.len() > 1,
        "progress must move more than once over a 200 KiB transfer, saw {}",
        reports.len()
    );
    assert!(
        reports
            .windows(2)
            .all(|pair| pair[1].transferred >= pair[0].transferred),
        "progress must not go backwards: {reports:?}"
    );
    assert_eq!(
        reports.last().map(|progress| progress.transferred),
        Some(body.len() as u64)
    );
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
#[test]
fn a_cancelled_native_transfer_stops() {
    let body = vec![b'z'; 512 * 1024];
    let Some((url, server)) =
        local_server(body, 8 * 1024, std::time::Duration::from_millis(5), false)
    else {
        return;
    };

    let control = HttpControl::new();
    let client = default_http_client();
    let response = pollster::block_on(client.send(&HttpRequest::get(&url), control.clone()))
        .expect("a response");

    let first = pollster::block_on(response.read_chunk()).expect("a chunk");
    assert!(first.is_some());
    control.cancel();

    let mut ended = false;
    for _ in 0..64 {
        match pollster::block_on(response.read_chunk()) {
            Ok(Some(_)) => continue,
            Ok(None) => {
                ended = true;
                break;
            }
            Err(HttpError::Cancelled) => {
                ended = true;
                break;
            }
            Err(other) => panic!("unexpected error after cancelling: {other}"),
        }
    }
    drop(response);
    let _ = server.join();
    assert!(ended, "a cancelled transfer must end rather than run on");
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
#[test]
fn a_download_resumes_from_what_is_already_on_disk() {
    let body: Vec<u8> = (0..4096u32).map(|value| value as u8).collect();
    let directory = crate::test_scratch_dir("http-resume");
    let target = directory.join("payload.bin");
    let _ = std::fs::remove_file(&target);
    std::fs::write(&target, &body[..1024]).expect("a partial file");

    let Some((url, server)) = local_server(body.clone(), 1024, std::time::Duration::ZERO, true)
    else {
        return;
    };
    let client = default_http_client();
    let written = pollster::block_on(client.download_to(&url, &target, HttpControl::new()))
        .expect("the download finishes");
    server.join().expect("the local server finishes");

    assert_eq!(written, body.len() as u64);
    assert_eq!(std::fs::read(&target).expect("the file"), body);
    let _ = std::fs::remove_file(&target);
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
#[test]
fn a_server_that_ignores_a_range_restarts_the_file_rather_than_appending() {
    let body: Vec<u8> = (0..4096u32).map(|value| value as u8).collect();
    let directory = crate::test_scratch_dir("http-restart");
    let target = directory.join("payload.bin");
    let _ = std::fs::remove_file(&target);
    std::fs::write(&target, &body[..1024]).expect("a partial file");

    let Some((url, server)) = local_server(body.clone(), 1024, std::time::Duration::ZERO, false)
    else {
        return;
    };
    let client = default_http_client();
    let written = pollster::block_on(client.download_to(&url, &target, HttpControl::new()))
        .expect("the download finishes");
    server.join().expect("the local server finishes");

    assert_eq!(written, body.len() as u64);
    assert_eq!(
        std::fs::read(&target).expect("the file"),
        body,
        "the partial file must be replaced, not appended to"
    );
    let _ = std::fs::remove_file(&target);
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
#[test]
fn default_http_client_fetches_text_from_local_server() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipping local HTTP server bind in restricted test environment: {err}");
            return;
        }
        Err(err) => panic!("bind local test server: {err}"),
    };
    let address = listener
        .local_addr()
        .expect("read local test server address");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept local test request");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request).expect("read local test request");
        let body = "cranpose-http-test";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .expect("write local test response");
    });

    let url = format!("http://{address}");
    let text = pollster::block_on(default_http_client().get_text(&url))
        .expect("fetch text from local test server");
    server.join().expect("join local test server");

    assert_eq!(text, "cranpose-http-test");
}
