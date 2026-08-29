#![allow(unsafe_code)]

use std::sync::OnceLock;

pub(crate) fn display_is_round(app: &android_activity::AndroidApp) -> bool {
    type GetScreenRound = unsafe extern "C" fn(*mut ndk_sys::AConfiguration) -> libc::c_int;
    static GETTER: OnceLock<Option<GetScreenRound>> = OnceLock::new();
    let getter = GETTER.get_or_init(|| {
        let symbol = unsafe {
            libc::dlsym(
                libc::RTLD_DEFAULT,
                c"AConfiguration_getScreenRound".as_ptr(),
            )
        };
        if symbol.is_null() {
            return None;
        }
        // SAFETY: the symbol, when present, is libandroid's
        // `AConfiguration_getScreenRound`, whose C signature is exactly
        // `int32_t (AConfiguration*)`.
        Some(unsafe { std::mem::transmute::<*mut libc::c_void, GetScreenRound>(symbol) })
    });
    let Some(getter) = getter else {
        return false;
    };
    let config = app.config().copy();
    // SAFETY: `ptr()` is the snapshot's valid AConfiguration; the getter
    // only reads it.
    let round = unsafe { getter(config.ptr().as_ptr()) };
    round == ndk_sys::ACONFIGURATION_SCREENROUND_YES as libc::c_int
}
