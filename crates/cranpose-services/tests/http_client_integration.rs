use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_core::{Composition, CompositionLocalProvider, MemoryApplier, location_key};
use cranpose_services::{
    BytesBody, HttpClient, HttpClientRef, HttpControl, HttpFuture, HttpRequest, HttpResponse,
    local_http_client,
};

#[cfg(feature = "http-native")]
#[test]
#[ignore = "requires public HTTPS and a local self-signed TLS test server"]
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
