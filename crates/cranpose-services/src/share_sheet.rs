//! System share sheet: hand a file (and optional text) to the OS share UI so
//! the user can send it to another app.
//!
//! Like [`crate::uri_handler`], the compiled-in default is a no-op that reports
//! [`ShareError::Unsupported`]; platform backends install a real implementation
//! through [`set_platform_share_sheet`] (iOS `UIActivityViewController`, Android
//! `ACTION_SEND`, the Web Share API). Desktop has no system share sheet — apps
//! export through a save dialog instead — so `is_supported` is `false` there and
//! the UI can offer "Save…" rather than "Share".

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{compositionLocalOfWithPolicy, CompositionLocal, CompositionLocalProvider};
use cranpose_macros::composable;

#[derive(thiserror::Error, Debug)]
pub enum ShareError {
    #[error("sharing is not supported on this platform")]
    Unsupported,
    #[error("failed to present the share sheet: {0}")]
    Failed(String),
}

/// A single file to share, with an optional accompanying message/subject.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShareContent {
    /// Suggested file name shown to the user and used by the receiving app.
    pub file_name: String,
    /// MIME type of `bytes` (e.g. `application/pdf`, `image/jpeg`).
    pub mime_type: String,
    /// File payload. Backends stage it to a temporary URL to share.
    pub bytes: Vec<u8>,
    /// Optional text/subject shared alongside the file.
    pub text: Option<String>,
}

impl ShareContent {
    /// A file to share, identified by name and MIME type.
    pub fn file(
        file_name: impl Into<String>,
        mime_type: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            file_name: file_name.into(),
            mime_type: mime_type.into(),
            bytes,
            text: None,
        }
    }

    /// Attach accompanying text (subject / message) to the shared file.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }
}

/// The OS share sheet. Installed by the platform backend; the default is a
/// no-op that reports [`ShareError::Unsupported`].
pub trait ShareSheet {
    /// Present the system share sheet for `content`. Fire-and-forget: it
    /// resolves once presented and does not report the user's choice.
    fn share(&self, content: ShareContent) -> Result<(), ShareError>;

    /// Whether a real system share sheet is available (so UI can choose between
    /// "Share" and "Save…").
    fn is_supported(&self) -> bool;
}

pub type ShareSheetRef = Rc<dyn ShareSheet>;

thread_local! {
    static PLATFORM_SHARE_SHEET: RefCell<Option<ShareSheetRef>> = const { RefCell::new(None) };
}

/// Installs a platform share sheet, replacing any previously installed one.
/// Backends with main-thread UIKit / Activity access register here.
pub fn set_platform_share_sheet(share_sheet: ShareSheetRef) {
    PLATFORM_SHARE_SHEET.with(|cell| *cell.borrow_mut() = Some(share_sheet));
}

/// Removes any registered platform share sheet (tests and teardown).
pub fn clear_platform_share_sheet() {
    PLATFORM_SHARE_SHEET.with(|cell| *cell.borrow_mut() = None);
}

fn registered_platform_share_sheet() -> Option<ShareSheetRef> {
    PLATFORM_SHARE_SHEET.with(|cell| cell.borrow().clone())
}

struct PlatformShareSheet;

impl ShareSheet for PlatformShareSheet {
    fn share(&self, content: ShareContent) -> Result<(), ShareError> {
        match registered_platform_share_sheet() {
            Some(handler) => handler.share(content),
            None => Err(ShareError::Unsupported),
        }
    }

    fn is_supported(&self) -> bool {
        registered_platform_share_sheet().is_some_and(|handler| handler.is_supported())
    }
}

pub fn default_share_sheet() -> ShareSheetRef {
    Rc::new(PlatformShareSheet)
}

pub fn local_share_sheet() -> CompositionLocal<ShareSheetRef> {
    thread_local! {
        static LOCAL_SHARE_SHEET: RefCell<Option<CompositionLocal<ShareSheetRef>>> = const { RefCell::new(None) };
    }

    LOCAL_SHARE_SHEET.with(|cell| {
        let mut local = cell.borrow_mut();
        local
            .get_or_insert_with(|| compositionLocalOfWithPolicy(default_share_sheet, Rc::ptr_eq))
            .clone()
    })
}

#[allow(non_snake_case)]
#[composable]
pub fn ProvideShareSheet(content: impl FnOnce()) {
    let share_sheet = cranpose_core::remember(default_share_sheet).with(|state| state.clone());
    let local = local_share_sheet();

    CompositionLocalProvider(vec![local.provides(share_sheet)], move || {
        content();
    });
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct RecordingShareSheet {
        shared: RefCell<Option<ShareContent>>,
        supported: bool,
    }

    impl ShareSheet for RecordingShareSheet {
        fn share(&self, content: ShareContent) -> Result<(), ShareError> {
            *self.shared.borrow_mut() = Some(content);
            Ok(())
        }
        fn is_supported(&self) -> bool {
            self.supported
        }
    }

    #[test]
    fn default_share_sheet_is_unsupported_without_a_backend() {
        clear_platform_share_sheet();
        let sheet = default_share_sheet();
        assert!(!sheet.is_supported());
        let error = sheet
            .share(ShareContent::file(
                "a.pdf",
                "application/pdf",
                vec![1, 2, 3],
            ))
            .expect_err("no backend installed");
        assert!(matches!(error, ShareError::Unsupported));
    }

    #[test]
    fn registered_share_sheet_takes_precedence() {
        let recorder = Rc::new(RecordingShareSheet {
            shared: RefCell::new(None),
            supported: true,
        });
        set_platform_share_sheet(recorder.clone());

        let sheet = default_share_sheet();
        assert!(sheet.is_supported());
        sheet
            .share(ShareContent::file("r.pdf", "application/pdf", vec![9]).with_text("hi"))
            .expect("registered backend shares");

        let shared = recorder.shared.borrow();
        let shared = shared.as_ref().expect("content recorded");
        assert_eq!(shared.file_name, "r.pdf");
        assert_eq!(shared.text.as_deref(), Some("hi"));

        clear_platform_share_sheet();
        assert!(!default_share_sheet().is_supported());
    }
}
