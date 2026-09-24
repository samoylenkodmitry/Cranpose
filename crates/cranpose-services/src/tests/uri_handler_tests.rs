use std::cell::RefCell;

use cranpose_core::CompositionLocalProvider;

use super::*;
use crate::run_test_composition;

struct TestUriHandler;

impl UriHandler for TestUriHandler {
    fn open_uri(&self, _uri: &str) -> Result<(), UriHandlerError> {
        Ok(())
    }
}

#[test]
fn default_uri_handler_can_be_created() {
    let handler = default_uri_handler();
    assert_eq!(Rc::strong_count(&handler), 1);
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(feature = "uri-native")
))]
#[test]
fn default_uri_handler_reports_disabled_native_uri_feature() {
    let error = default_uri_handler()
        .open_uri("https://example.com")
        .expect_err("native URI opening should be feature-gated");

    assert!(matches!(
        error,
        UriHandlerError::UnsupportedFeature {
            feature: "uri-native",
            ..
        }
    ));
}

#[test]
fn local_uri_handler_can_be_overridden() {
    let local = local_uri_handler();
    let default_handler = default_uri_handler();
    let custom_handler: UriHandlerRef = Rc::new(TestUriHandler);
    let captured = Rc::new(RefCell::new(None));

    {
        let captured = Rc::clone(&captured);
        let custom_handler = custom_handler.clone();
        let local_for_provider = local.clone();
        let local_for_read = local.clone();
        run_test_composition(move || {
            let captured = Rc::clone(&captured);
            let custom_handler = custom_handler.clone();
            let local_for_read = local_for_read.clone();
            CompositionLocalProvider(
                vec![local_for_provider.provides(custom_handler)],
                move || {
                    let current = local_for_read.current();
                    *captured.borrow_mut() = Some(current);
                },
            );
        });
    }

    let current = captured
        .borrow()
        .as_ref()
        .expect("handler captured")
        .clone();
    assert!(Rc::ptr_eq(&current, &custom_handler));
    assert!(!Rc::ptr_eq(&current, &default_handler));
}

#[test]
fn provide_uri_handler_sets_current_handler() {
    let local = local_uri_handler();
    let captured = Rc::new(RefCell::new(None));

    {
        let captured = Rc::clone(&captured);
        let local = local.clone();
        run_test_composition(move || {
            let captured = Rc::clone(&captured);
            let local = local.clone();
            ProvideUriHandler(move || {
                let current = local.current();
                *captured.borrow_mut() = Some(current);
            });
        });
    }

    assert!(captured.borrow().is_some());
}
