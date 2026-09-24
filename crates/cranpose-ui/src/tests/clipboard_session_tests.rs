use std::cell::RefCell;

use super::*;

struct RecordingClipboard {
    value: RefCell<Option<String>>,
}

struct UnavailableClipboard;

struct AsyncOnlyClipboard {
    requests: Cell<usize>,
}

impl PlatformClipboard for RecordingClipboard {
    fn write_text(&self, text: &str) {
        *self.value.borrow_mut() = Some(text.to_string());
    }
    fn read_text(&self) -> Option<String> {
        self.value.borrow().clone()
    }
}

impl PlatformClipboard for UnavailableClipboard {
    fn write_text(&self, _text: &str) {}

    fn read_text(&self) -> Option<String> {
        None
    }
}

impl PlatformClipboard for AsyncOnlyClipboard {
    fn write_text(&self, _text: &str) {}

    fn read_text(&self) -> Option<String> {
        None
    }

    fn can_request_paste(&self) -> bool {
        true
    }

    fn request_paste(&self) -> bool {
        self.requests.set(self.requests.get() + 1);
        true
    }
}

#[test]
fn manager_uses_in_process_fallback_without_a_platform_clipboard() {
    let context = crate::render_state::AppContext::new();
    context.enter(|| {
        let clipboard = ClipboardManager;
        assert!(!clipboard.has_system_clipboard());
        clipboard.set_text("hello");
        assert_eq!(clipboard.text().as_deref(), Some("hello"));
    });
}

#[test]
fn manager_routes_through_the_installed_platform_clipboard() {
    let context = crate::render_state::AppContext::new();
    context.enter(|| {
        let recorder = Rc::new(RecordingClipboard {
            value: RefCell::new(None),
        });
        set_platform_clipboard(recorder.clone());

        let clipboard = ClipboardManager;
        assert!(clipboard.has_system_clipboard());
        clipboard.set_text("world");
        assert_eq!(recorder.value.borrow().as_deref(), Some("world"));
        assert_eq!(clipboard.text().as_deref(), Some("world"));

        clear_platform_clipboard();
        assert!(!clipboard.has_system_clipboard());
    });
}

#[test]
fn a_paste_goes_to_the_platform_when_it_cannot_be_read_on_the_spot() {
    let context = crate::render_state::AppContext::new();
    context.enter(|| {
        let clipboard = Rc::new(AsyncOnlyClipboard {
            requests: Cell::new(0),
        });
        set_platform_clipboard(clipboard.clone());

        assert!(clipboard_can_paste());
        clipboard_paste_into_focus();
        assert_eq!(clipboard.requests.get(), 1);
    });
}

#[test]
fn a_readable_clipboard_pastes_without_asking_the_platform() {
    let context = crate::render_state::AppContext::new();
    context.enter(|| {
        assert!(!clipboard_can_paste());
        clipboard_write_text("pasted");
        assert!(clipboard_can_paste());
        clipboard_paste_into_focus();
    });
}

#[test]
fn manager_falls_back_when_the_installed_platform_is_unavailable() {
    let context = crate::render_state::AppContext::new();
    context.enter(|| {
        set_platform_clipboard(Rc::new(UnavailableClipboard));

        let clipboard = ClipboardManager;
        clipboard.set_text("headless copy");
        assert_eq!(clipboard.text().as_deref(), Some("headless copy"));
    });
}
