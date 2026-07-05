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

use std::cell::RefCell;
use std::rc::Rc;

/// A platform-provided OS clipboard. Installed by the platform runtime so that
/// in-tree UI (the selection menu) can read/write the system clipboard.
pub trait PlatformClipboard {
    /// Writes `text` to the OS clipboard.
    fn write_text(&self, text: &str);
    /// Reads the OS clipboard's text, or `None` when empty/unavailable.
    fn read_text(&self) -> Option<String>;
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
        if let Some(platform) = self.platform.borrow().clone() {
            platform.write_text(text);
        } else {
            *self.fallback.borrow_mut() = Some(text.to_string());
        }
    }

    fn read(&self) -> Option<String> {
        if let Some(platform) = self.platform.borrow().clone() {
            platform.read_text()
        } else {
            self.fallback.borrow().clone()
        }
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
