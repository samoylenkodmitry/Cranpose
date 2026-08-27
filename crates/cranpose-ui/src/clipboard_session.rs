//! Platform clipboard session: an OS clipboard read/write bridge for in-tree UI.
//!
//! The text selection contextual menu (Copy / Cut / Paste) lives in the widget
//! tree (`cranpose-ui`), which cannot reach the OS clipboard directly — that
//! machinery lives one layer up in `cranpose-app-shell` / platform glue
//! (`arboard` on desktop, DOM clipboard on web). Platforms install a
//! [`PlatformClipboard`] so the menu can copy/paste through the system
//! clipboard; when none is installed an in-process fallback keeps copy/paste
//! working within the app (and in headless tests).
//!
//! The session is stored per [`AppContext`](crate::render_state::AppContext),
//! like the text-input and focus sessions, so multiple app instances in one
//! process do not share a clipboard.

#[cfg(test)]
use std::cell::Cell;
use std::{cell::RefCell, rc::Rc};

/// A platform-provided OS clipboard. Installed by the platform runtime so that
/// in-tree UI (the selection menu) can read/write the system clipboard.
pub trait PlatformClipboard {
    /// Writes `text` to the OS clipboard.
    fn write_text(&self, text: &str);
    /// Reads the OS clipboard's text, or `None` when empty/unavailable.
    ///
    /// A platform whose clipboard cannot be read synchronously answers `None`
    /// here and takes the paste through [`request_paste`](Self::request_paste)
    /// instead.
    fn read_text(&self) -> Option<String>;

    /// Whether this platform can complete a paste it cannot answer
    /// synchronously, so a Paste action stays offered even though
    /// [`read_text`](Self::read_text) has nothing to show.
    fn can_request_paste(&self) -> bool {
        false
    }

    /// Asks the platform to paste the clipboard into whatever holds focus, for
    /// platforms that cannot answer [`read_text`](Self::read_text).
    ///
    /// Returns `true` when the platform has taken the request — the text lands
    /// in the focused field through the platform's own paste path, possibly
    /// after this returns. `false` means the caller should paste
    /// [`read_text`](Self::read_text) itself, which is what every clipboard
    /// with a synchronous read does.
    ///
    /// This exists because the browser's clipboard is a promise: reading it is
    /// asynchronous and permissioned, so `Some(text)` is not something a web
    /// bridge can produce on the call stack of the tap that asked for it.
    fn request_paste(&self) -> bool {
        false
    }
}

/// Per-app-context clipboard state: an optionally-installed platform clipboard
/// plus an in-process fallback used when none is installed.
pub(crate) struct ClipboardSessionState {
    platform: RefCell<Option<Rc<dyn PlatformClipboard>>>,
    fallback: RefCell<Option<String>>,
}

impl ClipboardSessionState {
    pub(crate) fn new() -> Self {
        Self {
            platform: RefCell::new(None),
            fallback: RefCell::new(None),
        }
    }

    fn set_platform(&self, clipboard: Option<Rc<dyn PlatformClipboard>>) {
        *self.platform.borrow_mut() = clipboard;
    }

    fn write(&self, text: &str) {
        // Keep a local copy even when a platform bridge is installed: native
        // clipboards may be temporarily unavailable (notably headless desktop
        // sessions), and copy/paste must still round-trip within this app.
        *self.fallback.borrow_mut() = Some(text.to_string());
        if let Some(platform) = self.platform.borrow().clone() {
            platform.write_text(text);
        }
    }

    fn read(&self) -> Option<String> {
        if let Some(platform) = self.platform.borrow().clone()
            && let Some(text) = platform.read_text()
        {
            return Some(text);
        }
        self.fallback.borrow().clone()
    }

    fn has_platform(&self) -> bool {
        self.platform.borrow().is_some()
    }

    fn can_request_paste(&self) -> bool {
        self.platform
            .borrow()
            .as_ref()
            .is_some_and(|platform| platform.can_request_paste())
    }

    fn request_paste(&self) -> bool {
        self.platform
            .borrow()
            .clone()
            .is_some_and(|platform| platform.request_paste())
    }
}

/// Installs the platform OS clipboard for the current app context, replacing any
/// previously installed one. Platform runtimes call this
/// (`AppShell::set_platform_clipboard`).
pub fn set_platform_clipboard(clipboard: Rc<dyn PlatformClipboard>) {
    crate::render_state::with_clipboard_session(|state| state.set_platform(Some(clipboard)));
}

/// Removes the installed platform clipboard, falling back to the in-process one.
pub fn clear_platform_clipboard() {
    crate::render_state::with_clipboard_session(|state| state.set_platform(None));
}

/// Writes `text` to the clipboard (OS clipboard when a platform is installed,
/// otherwise the in-process fallback).
pub fn clipboard_write_text(text: &str) {
    crate::render_state::with_clipboard_session(|state| state.write(text));
}

/// Reads the clipboard's text, or `None` when empty/unavailable.
pub fn clipboard_read_text() -> Option<String> {
    crate::render_state::with_clipboard_session(|state| state.read())
}

/// Whether a real OS clipboard is installed for the current app context (as
/// opposed to the in-process fallback used in headless tests or on platforms
/// with no clipboard backend registered).
pub fn has_platform_clipboard() -> bool {
    crate::render_state::with_clipboard_session(|state| state.has_platform())
}

/// Whether a Paste action should be offered.
///
/// True when the clipboard has readable text, and also when the platform can
/// only answer a paste asynchronously (the browser): a clipboard nobody can
/// read on the spot is not the same as an empty one, and hiding Paste there
/// would be wrong every time the user actually has something to paste.
pub fn clipboard_can_paste() -> bool {
    crate::render_state::with_clipboard_session(|state| {
        state.read().is_some() || state.can_request_paste()
    })
}

/// Pastes the clipboard into the focused text field — the in-tree Paste action.
///
/// Every native clipboard reads synchronously and the paste lands before this
/// returns. The browser's does not: there the platform takes the request and
/// completes it through its own paste path once the clipboard promise resolves,
/// which is why this is a command rather than a read.
pub fn clipboard_paste_into_focus() {
    if crate::render_state::with_clipboard_session(|state| state.request_paste()) {
        return;
    }
    if let Some(text) = clipboard_read_text() {
        crate::text_field_focus::dispatch_paste(&text);
    }
}

/// A Compose-style handle to the system clipboard — the framework analogue of
/// Jetpack Compose's `LocalClipboardManager`. Obtain it from
/// [`local_clipboard`] during composition, then read/write it (typically from an
/// event handler):
///
/// ```ignore
/// let clipboard = local_clipboard().current();
/// Button(Modifier::empty(), move || clipboard.set_text("copied!"), || Text("Copy"));
/// ```
///
/// It reads and writes through the app's clipboard session, so it targets the
/// installed platform clipboard (UIPasteboard, arboard, …) when present and an
/// in-process fallback otherwise.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct ClipboardManager;

impl ClipboardManager {
    /// Writes `text` to the clipboard.
    pub fn set_text(&self, text: &str) {
        clipboard_write_text(text);
    }

    /// Reads the clipboard's text, or `None` when empty/unavailable.
    pub fn text(&self) -> Option<String> {
        clipboard_read_text()
    }

    /// Whether writes reach a real OS clipboard (vs the in-process fallback).
    pub fn has_system_clipboard(&self) -> bool {
        has_platform_clipboard()
    }
}

/// CompositionLocal carrying the [`ClipboardManager`]. The same instance is
/// returned on every call (cached per thread), matching `local_uri_handler` and
/// the insets locals.
pub fn local_clipboard() -> cranpose_core::CompositionLocal<ClipboardManager> {
    thread_local! {
        static LOCAL_CLIPBOARD: RefCell<Option<cranpose_core::CompositionLocal<ClipboardManager>>> =
            const { RefCell::new(None) };
    }

    LOCAL_CLIPBOARD.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| cranpose_core::compositionLocalOf(ClipboardManager::default))
            .clone()
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct RecordingClipboard {
        value: RefCell<Option<String>>,
    }

    struct UnavailableClipboard;

    /// A clipboard shaped like the browser's: nothing to read on the call
    /// stack, but able to complete a paste of its own accord.
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

            // A clipboard nobody can read here and now still offers Paste --
            // hiding it would hide every real paste the browser can serve.
            assert!(clipboard_can_paste());
            clipboard_paste_into_focus();
            assert_eq!(clipboard.requests.get(), 1);
        });
    }

    #[test]
    fn a_readable_clipboard_pastes_without_asking_the_platform() {
        let context = crate::render_state::AppContext::new();
        context.enter(|| {
            // No platform at all: the in-process fallback is readable, so the
            // paste must take the synchronous path rather than vanish.
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
}
