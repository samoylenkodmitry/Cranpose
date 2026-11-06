use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_core::{Composition, CompositionLocalProvider, MemoryApplier, location_key};
use cranpose_services::{
    BytesBody, HttpClient, HttpClientRef, HttpControl, HttpFuture, HttpRequest, HttpResponse,
    local_http_client,
};

#[cfg(feature = "http-native")]
#[test]
#[ignore = "requires public HTTPS and a local self-signed TLS test server"]
#[expect(
    clippy::disallowed_methods,
    reason = "This opt-in integration test receives its TLS server URLs from the test environment"
)]
fn native_https_accepts_trusted_certificates_and_rejects_untrusted_certificates() {
    let trusted = std::env::var("CRANPOSE_TEST_TRUSTED_HTTPS").expect("trusted HTTPS URL");
    let untrusted = std::env::var("CRANPOSE_TEST_UNTRUSTED_HTTPS").expect("self-signed HTTPS URL");
    let client = cranpose_services::default_http_client();
    let response = pollster::block_on(client.send(&HttpRequest::get(trusted), HttpControl::new()))
        .expect("trusted TLS connection");
    assert_eq!(response.status, 200);
    assert!(
        pollster::block_on(client.send(&HttpRequest::get(untrusted), HttpControl::new())).is_err()
    );
}

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

fn run_test_composition(build: impl FnMut()) {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), build)
        .expect("initial render succeeds");
}

#[test]
fn integration_local_http_client_overrides_current() {
    let local = local_http_client();
    let custom_client: HttpClientRef = Arc::new(TestHttpClient);
    let captured = Rc::new(RefCell::new(None));

    {
        let captured = Rc::clone(&captured);
        let custom_client = Arc::clone(&custom_client);
        let local_for_provider = local.clone();
        let local_for_read = local.clone();
        run_test_composition(move || {
            let captured = Rc::clone(&captured);
            let local_for_read = local_for_read.clone();
            let custom_client = Arc::clone(&custom_client);
            CompositionLocalProvider(
                vec![local_for_provider.provides(custom_client)],
                move || {
                    let current = local_for_read.current();
                    *captured.borrow_mut() = Some(current);
                },
            );
        });
    }

    let current = Arc::clone(captured.borrow().as_ref().expect("client captured"));
    assert!(Arc::ptr_eq(&current, &custom_client));
}

// The server holds its answer for at most this long, so a request that
// blocked the UI thread fails the test and does not hang it.
#[cfg(feature = "http-native")]
const HELD: std::time::Duration = std::time::Duration::from_secs(5);

// A request awaited in an effect. The server holds its answer while the UI
// thread drains its work. The answer then reaches the effect.
#[cfg(feature = "http-native")]
#[test]
fn an_effect_awaits_a_request_while_the_ui_thread_stays_free() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        time::{Duration, Instant},
    };

    use cranpose_core::LaunchedEffect;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a local server");
    let address = listener.local_addr().expect("local server address");
    let (arrived, request_arrived) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept the request");
        let mut request = [0_u8; 1024];
        let read = stream.read(&mut request).expect("read the request");
        assert!(read > 0, "the request came empty");
        arrived.send(()).expect("note the request");
        // The answer goes out on release, or after `HELD` once the test failed.
        let _held: Result<(), mpsc::RecvTimeoutError> = released.recv_timeout(HELD);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\npage"
        )
        .expect("write the answer");
    });

    let text = Rc::new(RefCell::new(None::<String>));
    let sink = Rc::clone(&text);
    let url = format!("http://{address}");
    let mut composition = Composition::new(MemoryApplier::new());
    let started = Instant::now();
    composition
        .render(location_key(file!(), line!(), column!()), move || {
            let (sink, url) = (Rc::clone(&sink), url.clone());
            let client = cranpose_services::default_http_client();
            LaunchedEffect((), async move |_| {
                let page = client.get_text(&url).await.expect("the page");
                *sink.borrow_mut() = Some(page);
            });
        })
        .expect("render succeeds");
    let runtime = composition.runtime_handle();

    while request_arrived.try_recv().is_err() && started.elapsed() < HELD * 2 {
        runtime.drain_ui();
        std::thread::yield_now();
    }
    for _ in 0..10 {
        runtime.drain_ui();
    }
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the UI thread waited for the network"
    );
    assert!(
        text.borrow().is_none(),
        "the effect finished before the server answered"
    );

    release.send(()).expect("release the answer");
    assert!(crate::composition_support::pump_until(
        &mut composition,
        || { text.borrow().is_some() }
    ));
    assert_eq!(text.borrow().as_deref(), Some("page"));
    server.join().expect("join the server");
}
