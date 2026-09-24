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
