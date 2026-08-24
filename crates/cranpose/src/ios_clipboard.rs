//! iOS system clipboard built on `UIPasteboard`.
//!
//! Registered as the platform clipboard (see
//! [`cranpose_ui::clipboard_session::set_platform_clipboard`]) by the iOS
//! backend, so the text-selection Copy/Cut/Paste menu reads and writes the
//! general pasteboard instead of the in-process fallback.
#![allow(unsafe_code)]

use std::rc::Rc;

use cranpose_ui::clipboard_session::{set_platform_clipboard, PlatformClipboard};
use objc2_foundation::NSString;
use objc2_ui_kit::UIPasteboard;

/// Installs the iOS pasteboard as the platform clipboard for the current
/// app context. Must be called with the context entered.
pub(crate) fn register() {
    set_platform_clipboard(Rc::new(IosClipboard));
}

struct IosClipboard;

impl PlatformClipboard for IosClipboard {
    fn write_text(&self, text: &str) {
        let string = NSString::from_str(text);
        let pasteboard = UIPasteboard::generalPasteboard();
        // SAFETY: writing a string to the process-wide general pasteboard.
        unsafe { pasteboard.setString(Some(&string)) };
    }

    fn read_text(&self) -> Option<String> {
        let pasteboard = UIPasteboard::generalPasteboard();
        // SAFETY: reading the general pasteboard's string property.
        let string = unsafe { pasteboard.string() }?;
        Some(string.to_string())
    }
}
