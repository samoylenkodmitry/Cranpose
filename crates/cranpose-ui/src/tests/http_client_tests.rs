use crate::{
    default_http_client, local_http_client, run_test_composition, HttpClient, HttpClientRef,
};
use cranpose_core::CompositionLocalProvider;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

struct TestHttpClient;

impl HttpClient for TestHttpClient {
    fn get_text<'a>(&'a self, _url: &'a str) -> crate::http::HttpFuture<'a, String> {
        Box::pin(async { Ok("ok".to_string()) })
    }
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
