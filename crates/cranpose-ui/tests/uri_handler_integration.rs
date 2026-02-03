use cranpose_core::CompositionLocalProvider;
use cranpose_ui::{
    local_uri_handler, run_test_composition, UriHandler, UriHandlerError, UriHandlerRef,
};
use std::cell::RefCell;
use std::rc::Rc;

struct TestUriHandler;

impl UriHandler for TestUriHandler {
    fn open_uri(&self, _uri: &str) -> Result<(), UriHandlerError> {
        Ok(())
    }
}

#[test]
fn integration_local_uri_handler_overrides_current() {
    let local = local_uri_handler();
    let custom_handler: UriHandlerRef = Rc::new(TestUriHandler);
    let captured = Rc::new(RefCell::new(None));

    {
        let captured = Rc::clone(&captured);
        let custom_handler = custom_handler.clone();
        let local_for_provider = local.clone();
        let local_for_read = local.clone();
        run_test_composition(move || {
            let captured = Rc::clone(&captured);
            let local_for_read = local_for_read.clone();
            let custom_handler = custom_handler.clone();
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
}
