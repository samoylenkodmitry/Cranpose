//! The strings that the backend hands to the GL debug functions.
//!
//! The debug functions take a length with each string, so GL lets the string end without a NUL.
//! Some drivers still read the string up to a NUL: the Adreno driver of the Quest 3 copies the
//! message of `glPushDebugGroup` with `strlcpy`, which reads on past the length until it meets a
//! NUL. A label that fills its heap block up to a page that cannot be read then crashes the app
//! with SIGSEGV. So every string that reaches these functions keeps a NUL right after its text.

use alloc::{ffi::CString, vec::Vec};
use core::{ffi::CStr, ops::Range};

use glow::HasContext;

const DEBUG_ID: u32 = 0;

// The text before the first NUL: the driver reads no further than that NUL either.
fn before_nul(text: &str) -> &str {
    text.find('\0').map_or(text, |end| &text[..end])
}

/// The label of a command buffer, with a NUL after its text.
pub(super) fn label(text: &str) -> CString {
    CString::new(before_nul(text)).expect("the text ends before its first NUL")
}

/// Adds `marker` and a NUL to `data`. The range covers the marker's text.
pub(super) fn add_marker(data: &mut Vec<u8>, marker: &str) -> Range<u32> {
    let start = data.len() as u32;
    data.extend(before_nul(marker).as_bytes());
    let end = data.len() as u32;
    data.push(0);
    start..end
}

/// The marker that `add_marker` put at `range` of `data`.
pub(super) fn marker<'a>(data: &'a [u8], range: &Range<u32>) -> &'a CStr {
    CStr::from_bytes_with_nul(&data[range.start as usize..=range.end as usize])
        .expect("a NUL follows each marker")
}

/// The text handed to a debug function: the bytes of `message` before its NUL. Some drivers read
/// the string even when its length is 0, so an empty message becomes `<empty>`.
pub(super) fn text(message: &CStr) -> &str {
    let message = if message.is_empty() {
        c"<empty>"
    } else {
        message
    };
    message
        .to_str()
        .expect("the message was a `str` cut at a NUL")
}

pub(super) unsafe fn push_debug_group(gl: &glow::Context, message: &CStr) {
    unsafe { gl.push_debug_group(glow::DEBUG_SOURCE_APPLICATION, DEBUG_ID, text(message)) };
}

pub(super) unsafe fn insert_debug_marker(gl: &glow::Context, message: &CStr) {
    unsafe {
        gl.debug_message_insert(
            glow::DEBUG_SOURCE_APPLICATION,
            glow::DEBUG_TYPE_MARKER,
            DEBUG_ID,
            glow::DEBUG_SEVERITY_NOTIFICATION,
            text(message),
        )
    };
}

#[cfg(test)]
#[path = "tests/debug_text.rs"]
mod tests;
