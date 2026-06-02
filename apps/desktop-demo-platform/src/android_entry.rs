#![allow(unsafe_code)]

#[doc(hidden)]
#[no_mangle]
pub fn android_main(app: android_activity::AndroidApp) {
    super::create_app().run(app, desktop_demo::app::combined_app);
}
