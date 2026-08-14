//! Platform truth about the physical display panel.
//!
//! One question today: is the display round? The renderer's round-display
//! corner cull hangs off this answer — the FRAMEWORK learns the panel shape
//! from the OS configuration and culls what a round panel physically never
//! shows, for any app and any layout. No screen-shape assumption may
//! originate from app content, which is why this module is the only source
//! of the flag.

#![allow(unsafe_code)]

use std::sync::OnceLock;

/// Whether the display this activity's configuration describes is
/// physically round: `AConfiguration`'s screenRound field is
/// `ACONFIGURATION_SCREENROUND_YES`.
///
/// Read through the raw NDK call. The safe wrappers exist but are feature-
/// gated behind `api-level-30` (`ndk` 0.9), and `android-activity` 0.6.1's
/// re-export does not even compile with the feature on (it forgets to
/// import `ScreenRound`). The C symbol itself is `__INTRODUCED_IN(30)`, so
/// it is resolved once via `dlsym` instead of linked: on an older device
/// the platform cannot report roundness and this returns `false` — the
/// cull stays structurally off rather than the process failing to load.
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
    // `copy()` snapshots the live ConfigurationRef under its lock; the
    // snapshot owns its AConfiguration for the duration of the call.
    let config = app.config().copy();
    // SAFETY: `ptr()` is the snapshot's valid AConfiguration; the getter
    // only reads it.
    let round = unsafe { getter(config.ptr().as_ptr()) };
    round == ndk_sys::ACONFIGURATION_SCREENROUND_YES as libc::c_int
}
