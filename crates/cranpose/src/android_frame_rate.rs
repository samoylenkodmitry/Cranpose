//! On-demand display frame-rate voting for the Android frame loop.
//!
//! Compose gameplay runs a 120 Hz panel at 120 not because the app asks for
//! it but because HWUI votes a frame rate on the window while animations and
//! gestures run, and clears the vote when they stop. A window that never
//! votes meets the other half of the same machinery: SurfaceFlinger infers a
//! rate from the present cadence it observes and pins the app there with a
//! `frameRateOverride`, which also throttles the app's choreographer — so the
//! inference feeds on its own output and never lets go. Measured on a Pixel
//! 9 Pro: `frameRateOverrides={uid=… frameRateHz=60.0}` for a Cranpose app on
//! a 120 Hz panel, permanently, while the Compose original bursts to 120.
//!
//! [`FrameRateVoter`] applies the rate that
//! [`cranpose_app_shell::FrameRatePreference`] asks for. The vote goes
//! through `ANativeWindow_setFrameRateWithChangeStrategy` (API 31) or
//! `ANativeWindow_setFrameRate` (API 30), both resolved with `dlsym` so a
//! minSdk 29 build loads everywhere and quietly does nothing on Android 10,
//! where displays have a single rate anyway.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::sync::OnceLock;

/// `ANATIVEWINDOW_FRAME_RATE_COMPATIBILITY_DEFAULT`: the rate is a UI hint,
/// not fixed-source video content, so SurfaceFlinger may pick any suitable
/// display mode near it.
const FRAME_RATE_COMPATIBILITY_DEFAULT: i8 = 0;
/// `ANATIVEWINDOW_CHANGE_FRAME_RATE_ALWAYS`: accept a non-seamless mode
/// switch. This is what a game bursting a 60/120 panel needs; the default
/// only-if-seamless strategy would silently keep 60 on panels whose mode
/// switch blanks a frame.
const CHANGE_FRAME_RATE_ALWAYS: i8 = 1;

type SetFrameRateFn = unsafe extern "C" fn(*mut c_void, f32, i8) -> i32;
type SetFrameRateWithChangeStrategyFn = unsafe extern "C" fn(*mut c_void, f32, i8, i8) -> i32;

enum VoteSymbol {
    WithStrategy(SetFrameRateWithChangeStrategyFn),
    Plain(SetFrameRateFn),
    Absent,
}

/// Resolves an `ANativeWindow_*` symbol. These live in `libnativewindow.so`,
/// which the Android linker's namespaces hide from `RTLD_DEFAULT` even though
/// the library is loaded (unlike `libandroid.so`'s `AChoreographer_*`
/// symbols, which resolve directly). `dlopen` of an NDK library by name is
/// always permitted for apps, so the fallback opens it explicitly; the handle
/// is deliberately never closed.
unsafe fn resolve_native_window_symbol(name: &std::ffi::CStr) -> *mut c_void {
    let direct = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    if !direct.is_null() {
        return direct;
    }
    let library = unsafe { libc::dlopen(c"libnativewindow.so".as_ptr(), libc::RTLD_LAZY) };
    if library.is_null() {
        return std::ptr::null_mut();
    }
    unsafe { libc::dlsym(library, name.as_ptr()) }
}

fn vote_symbol() -> &'static VoteSymbol {
    static SYMBOL: OnceLock<VoteSymbol> = OnceLock::new();
    SYMBOL.get_or_init(|| {
        // SAFETY: symbols come from the loaded libnativewindow.so; the
        // transmutes target the NDK-documented signatures.
        unsafe {
            let with_strategy = resolve_native_window_symbol(
                c"ANativeWindow_setFrameRateWithChangeStrategy",
            );
            if !with_strategy.is_null() {
                return VoteSymbol::WithStrategy(std::mem::transmute::<
                    *mut c_void,
                    SetFrameRateWithChangeStrategyFn,
                >(with_strategy));
            }
            let plain = resolve_native_window_symbol(c"ANativeWindow_setFrameRate");
            if !plain.is_null() {
                return VoteSymbol::Plain(std::mem::transmute::<*mut c_void, SetFrameRateFn>(
                    plain,
                ));
            }
            log::info!(
                "[android-frame-rate] ANativeWindow_setFrameRate needs API 30; \
                 frame-rate votes are disabled on this device"
            );
            VoteSymbol::Absent
        }
    })
}

/// The display's fastest supported refresh rate, queried once over JNI.
///
/// `Display.getSupportedRefreshRates` rather than the choreographer's
/// refresh-rate callback, because the callback reports the rate the app is
/// currently *given* — under a SurfaceFlinger override that is exactly the
/// pinned rate the vote exists to escape.
pub(crate) fn panel_max_refresh_rate(app: &android_activity::AndroidApp) -> Option<f32> {
    static PANEL_MAX: OnceLock<Option<f32>> = OnceLock::new();
    *PANEL_MAX.get_or_init(|| match query_panel_max_refresh_rate(app) {
        Ok(rate) => {
            log::info!("[android-frame-rate] panel max refresh rate: {rate} Hz");
            Some(rate)
        }
        Err(error) => {
            log::warn!("[android-frame-rate] could not query display refresh rates: {error}");
            None
        }
    })
}

fn query_panel_max_refresh_rate(app: &android_activity::AndroidApp) -> Result<f32, String> {
    crate::android_jni::with_android_activity_env(app, |env, activity| {
        use jni::{jni_sig, jni_str, objects::JFloatArray};

        let describe = |env: &mut jni::Env<'_>, what: &str, error: jni::errors::Error| {
            crate::android_jni::clear_pending_android_jni_exception(env);
            format!("{what} failed: {error}")
        };
        let window_manager = env
            .call_method(
                &activity,
                jni_str!("getWindowManager"),
                jni_sig!("()Landroid/view/WindowManager;"),
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|error| describe(env, "Activity.getWindowManager", error))?;
        let display = env
            .call_method(
                &window_manager,
                jni_str!("getDefaultDisplay"),
                jni_sig!("()Landroid/view/Display;"),
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|error| describe(env, "WindowManager.getDefaultDisplay", error))?;
        let rates = env
            .call_method(
                &display,
                jni_str!("getSupportedRefreshRates"),
                jni_sig!("()[F"),
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|error| describe(env, "Display.getSupportedRefreshRates", error))?;
        let rates = env
            .cast_local::<JFloatArray>(rates)
            .map_err(|error| describe(env, "float[] cast", error))?;
        let len = rates
            .len(env)
            .map_err(|error| describe(env, "float[] length", error))?;
        let mut buffer = vec![0.0f32; len];
        rates
            .get_region(env, 0, &mut buffer)
            .map_err(|error| describe(env, "float[] read", error))?;
        buffer
            .into_iter()
            .filter(|rate| rate.is_finite() && *rate > 0.0)
            .fold(None, |best: Option<f32>, rate| {
                Some(best.map_or(rate, |best| best.max(rate)))
            })
            .ok_or_else(|| "display reported no refresh rates".to_string())
    })
}

/// Applies frame-rate votes to the app's native window, deduplicating so the
/// FFI call only happens when the desired rate (or the window itself)
/// changes. A recreated window gets a fresh pointer, which re-arms the vote
/// without explicit lifecycle tracking.
#[derive(Default)]
pub(crate) struct FrameRateVoter {
    last: Option<(usize, u32)>,
}

impl FrameRateVoter {
    /// Votes `rate_hz` on the current native window, `0.0` meaning "no
    /// preference". Cheap when nothing changed; a no-op when the app has no
    /// window or the device predates the API.
    pub(crate) fn apply(&mut self, app: &android_activity::AndroidApp, rate_hz: f32) {
        let symbol = vote_symbol();
        if matches!(symbol, VoteSymbol::Absent) {
            return;
        }
        let Some(window) = app.native_window() else {
            return;
        };
        let window_ptr = window.ptr().as_ptr().cast::<c_void>();
        let key = (window_ptr as usize, rate_hz.to_bits());
        if self.last == Some(key) {
            return;
        }
        // SAFETY: `window` keeps the ANativeWindow alive across the call; the
        // symbols carry their NDK signatures.
        let status = unsafe {
            match symbol {
                VoteSymbol::WithStrategy(set) => set(
                    window_ptr,
                    rate_hz,
                    FRAME_RATE_COMPATIBILITY_DEFAULT,
                    CHANGE_FRAME_RATE_ALWAYS,
                ),
                VoteSymbol::Plain(set) => {
                    set(window_ptr, rate_hz, FRAME_RATE_COMPATIBILITY_DEFAULT)
                }
                VoteSymbol::Absent => unreachable!(),
            }
        };
        if status == 0 {
            log::info!("[android-frame-rate] voted {rate_hz} Hz on window {window_ptr:?}");
            self.last = Some(key);
        } else {
            log::warn!("[android-frame-rate] setFrameRate({rate_hz}) returned {status}");
        }
    }
}
