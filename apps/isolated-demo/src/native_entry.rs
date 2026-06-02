#![allow(unsafe_code)]

#[cfg(target_os = "android")]
#[doc(hidden)]
#[no_mangle]
pub fn android_main(app_handle: android_activity::AndroidApp) {
    crate::app::create_app().run(app_handle, crate::app::IsolatedDemoApp);
}
