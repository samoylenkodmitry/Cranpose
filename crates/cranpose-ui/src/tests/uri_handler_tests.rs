use crate::{
    default_uri_handler, local_uri_handler, run_test_composition, UriHandler, UriHandlerRef,
};
use cranpose_core::CompositionLocalProvider;
use std::cell::RefCell;
use std::rc::Rc;

struct TestUriHandler;

impl UriHandler for TestUriHandler {
    fn open_uri(&self, _uri: &str) -> Result<(), crate::UriHandlerError> {
        Ok(())
    }
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
