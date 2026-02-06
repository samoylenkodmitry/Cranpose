use cranpose_core::{location_key, Composition, CompositionLocalProvider, MemoryApplier};
use cranpose_services::{local_http_client, HttpClient, HttpClientRef, HttpFuture};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

struct TestHttpClient;

impl HttpClient for TestHttpClient {
    fn get_text<'a>(&'a self, _url: &'a str) -> HttpFuture<'a, String> {
        Box::pin(async { Ok("ok".to_string()) })
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
        let custom_client = custom_client.clone();
        let local_for_provider = local.clone();
        let local_for_read = local.clone();
        run_test_composition(move || {
            let captured = Rc::clone(&captured);
            let local_for_read = local_for_read.clone();
            let custom_client = custom_client.clone();
            CompositionLocalProvider(
                vec![local_for_provider.provides(custom_client)],
                move || {
                    let current = local_for_read.current();
                    *captured.borrow_mut() = Some(current);
                },
            );
        });
    }

    let current = captured.borrow().as_ref().expect("client captured").clone();
    assert!(Arc::ptr_eq(&current, &custom_client));
}
