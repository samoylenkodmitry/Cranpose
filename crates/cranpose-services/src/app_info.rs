//! How the platform packaged this app: the version a user is shown, and the
//! build version a store orders releases by.
//!
//! An app that prints its own version usually knows it at compile time, which
//! is fine right up until the packaging step adds something the compiler never
//! saw — an Android `versionNameSuffix`, a CI build number, a store-assigned
//! build. Then the About screen and the artifact disagree, and the mismatch is
//! invisible until someone reads a bug report. This asks the platform what it
//! actually shipped.
//!
//! ```rust,no_run
//! use cranpose_services::app_info;
//!
//! // Prefer what the platform packaged; fall back to what was compiled in.
//! let version = app_info::version_name()
//!     .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
//! ```
//!
//! The default implementation answers `None` for both: a desktop binary run
//! straight out of `target/` was not packaged by anything and has no answer to
//! give. Platform backends install a real one with
//! [`set_platform_app_info`].

use std::cell::RefCell;
use std::rc::Rc;

/// Reports the packaged identity of the running app.
pub trait AppInfo {
    /// The version a user is shown — Android's `versionName`, Apple's
    /// `CFBundleShortVersionString`. `None` when the platform has none.
    fn version_name(&self) -> Option<String>;

    /// The build identifier a store orders releases by — Android's
    /// `versionCode`, Apple's `CFBundleVersion`. `None` when unknown.
    ///
    /// This is a string because Apple build versions may contain multiple
    /// numeric components, such as `42.3.1`. Android version codes are
    /// converted without losing their numeric value.
    fn build_version(&self) -> Option<String>;
}

pub type AppInfoRef = Rc<dyn AppInfo>;

struct DefaultAppInfo;

impl AppInfo for DefaultAppInfo {
    fn version_name(&self) -> Option<String> {
        None
    }

    fn build_version(&self) -> Option<String> {
        None
    }
}

thread_local! {
    static PLATFORM_APP_INFO: RefCell<Option<AppInfoRef>> = const { RefCell::new(None) };
}

/// Installs a platform app-info implementation, replacing any previous one.
pub fn set_platform_app_info(info: AppInfoRef) {
    PLATFORM_APP_INFO.with(|cell| *cell.borrow_mut() = Some(info));
}

/// Removes any registered platform app info (tests and teardown).
pub fn clear_platform_app_info() {
    PLATFORM_APP_INFO.with(|cell| *cell.borrow_mut() = None);
}

/// The active app info: the platform implementation if installed, otherwise
/// the built-in default.
pub fn app_info() -> AppInfoRef {
    PLATFORM_APP_INFO
        .with(|cell| cell.borrow().clone())
        .unwrap_or_else(|| Rc::new(DefaultAppInfo))
}

/// The version a user is shown, if the platform knows one.
pub fn version_name() -> Option<String> {
    app_info().version_name()
}

/// The build identifier a store orders releases by, if the platform knows one.
pub fn build_version() -> Option<String> {
    app_info().build_version()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Packaged;

    impl AppInfo for Packaged {
        fn version_name(&self) -> Option<String> {
            Some("1.4.2-debug".to_string())
        }

        fn build_version(&self) -> Option<String> {
            Some("17.2.1".to_string())
        }
    }

    #[test]
    fn an_unpackaged_binary_has_no_version_to_report() {
        clear_platform_app_info();
        assert_eq!(version_name(), None);
        assert_eq!(build_version(), None);
    }

    #[test]
    fn the_platform_answer_wins_and_carries_what_packaging_added() {
        clear_platform_app_info();
        set_platform_app_info(Rc::new(Packaged));
        // The suffix is the whole point: a compile-time constant cannot know
        // about it, because the packaging step is what adds it.
        assert_eq!(version_name().as_deref(), Some("1.4.2-debug"));
        assert_eq!(build_version().as_deref(), Some("17.2.1"));
        clear_platform_app_info();
        assert_eq!(version_name(), None);
    }
}
