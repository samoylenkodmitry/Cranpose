#![allow(unsafe_code)]

use std::{ffi::c_void, sync::OnceLock};

const FRAME_RATE_COMPATIBILITY_DEFAULT: i8 = 0;
const CHANGE_FRAME_RATE_ALWAYS: i8 = 1;

type SetFrameRateFn = unsafe extern "C" fn(*mut c_void, f32, i8) -> i32;
type SetFrameRateWithChangeStrategyFn = unsafe extern "C" fn(*mut c_void, f32, i8, i8) -> i32;

enum VoteSymbol {
    WithStrategy(SetFrameRateWithChangeStrategyFn),
    Plain(SetFrameRateFn),
    Absent,
}

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
    SYMBOL.get_or_init(|| unsafe {
        let with_strategy =
            resolve_native_window_symbol(c"ANativeWindow_setFrameRateWithChangeStrategy");
        if !with_strategy.is_null() {
            return VoteSymbol::WithStrategy(std::mem::transmute::<
                *mut c_void,
                SetFrameRateWithChangeStrategyFn,
            >(with_strategy));
        }
        let plain = resolve_native_window_symbol(c"ANativeWindow_setFrameRate");
        if !plain.is_null() {
            return VoteSymbol::Plain(std::mem::transmute::<*mut c_void, SetFrameRateFn>(plain));
        }
        log::info!(
            "[android-frame-rate] ANativeWindow_setFrameRate needs API 30; \
                 frame-rate votes are disabled on this device"
        );
        VoteSymbol::Absent
    })
}

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

#[derive(Default)]
pub(crate) struct FrameRateVoter {
    last: Option<(usize, u32)>,
}

impl FrameRateVoter {
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
