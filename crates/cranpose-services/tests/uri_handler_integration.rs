use cranpose_core::{location_key, Composition, CompositionLocalProvider, MemoryApplier};
use cranpose_services::{local_uri_handler, UriHandler, UriHandlerError, UriHandlerRef};
use std::cell::RefCell;
use std::rc::Rc;

struct TestUriHandler;

impl UriHandler for TestUriHandler {
    fn open_uri(&self, _uri: &str) -> Result<(), UriHandlerError> {
        Ok(())
    }
}

fn run_test_composition(build: impl FnMut()) {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), build)
        .expect("initial render succeeds");
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
