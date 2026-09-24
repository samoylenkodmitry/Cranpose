//! The pointer size a Windows user asked for.
//!
//! Settings' "Mouse pointer size" slider writes the size of the standard
//! cursors, 32 pixels unchanged, to `CursorBaseSize` under
//! `HKCU\Control Panel\Cursors`. Windows draws its own cursors at that size and
//! an app's custom images at theirs, so a custom cursor that follows the
//! system has to be scaled by the app.
#![expect(unsafe_code)]

use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// How much the system pointer is enlarged: 1 when the slider was never moved.
pub(crate) fn pointer_scale() -> f64 {
    let key = wide("Control Panel\\Cursors");
    let value = wide("CursorBaseSize");
    let mut size: u32 = 0;
    let mut length = std::mem::size_of::<u32>() as u32;
    // SAFETY: both names are NUL-terminated UTF-16 that outlive the call, and
    // the data pointer is a DWORD whose length is passed with it.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut size as *mut u32).cast(),
            &mut length,
        )
    };
    crate::cursor_scale::windows_cursor_scale((status == 0).then_some(size))
}
