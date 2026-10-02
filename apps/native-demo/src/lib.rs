mod screen;

use std::sync::Arc;

use cranpose_native::{NativeContent, NativeSession};

uniffi::setup_scaffolding!();

/// Creates the demo's shared screen and application events.
#[uniffi::export]
pub fn create_demo_component(native_children: bool, website: String) -> Arc<NativeSession> {
    NativeSession::new(move || {
        let website: std::rc::Rc<str> = website.into();
        let state = screen::DemoState::default();
        let content_state = state.clone();
        NativeContent::new(move || {
            screen::content(
                content_state.clone(),
                native_children,
                std::rc::Rc::clone(&website),
            );
        })
        .on_event(move |event| {
            if event.name == "increment" {
                state.increment();
            }
        })
    })
}
