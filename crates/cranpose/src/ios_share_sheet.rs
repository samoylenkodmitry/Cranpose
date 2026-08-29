#![allow(unsafe_code)]

use std::rc::Rc;

use cranpose_services::{ShareContent, ShareError, ShareSheet, set_platform_share_sheet};
use objc2::{MainThreadMarker, MainThreadOnly, runtime::AnyObject};
use objc2_foundation::{NSArray, NSString, NSTemporaryDirectory, NSURL};
use objc2_ui_kit::{UIActivityViewController, UIViewController};

pub(crate) fn register() {
    set_platform_share_sheet(Rc::new(IosShareSheet));
}

struct IosShareSheet;

impl ShareSheet for IosShareSheet {
    fn share(&self, content: ShareContent) -> Result<(), ShareError> {
        let Some(mtm) = MainThreadMarker::new() else {
            return Err(ShareError::Failed(
                "the share sheet must be presented on the main thread".into(),
            ));
        };

        let tmp_dir = NSTemporaryDirectory().to_string();
        let path = std::path::Path::new(&tmp_dir).join(&content.file_name);
        std::fs::write(&path, &content.bytes)
            .map_err(|error| ShareError::Failed(format!("staging shared file: {error}")))?;
        let ns_path = NSString::from_str(&path.to_string_lossy());
        let url = NSURL::fileURLWithPath(&ns_path);

        let url_item: &AnyObject = &url;
        let items = match &content.text {
            Some(text) => {
                let ns_text = NSString::from_str(text);
                let text_item: &AnyObject = &ns_text;
                NSArray::from_slice(&[text_item, url_item])
            }
            None => NSArray::from_slice(&[url_item]),
        };

        let controller = unsafe {
            UIActivityViewController::initWithActivityItems_applicationActivities(
                UIActivityViewController::alloc(mtm),
                &items,
                None,
            )
        };

        let root = crate::ios_file_picker::root_view_controller(mtm)
            .ok_or_else(|| ShareError::Failed("no root view controller to present from".into()))?;

        let presented: &UIViewController = &controller;
        if let Some(popover) = presented.popoverPresentationController()
            && let Some(view) = root.view()
        {
            popover.setSourceView(Some(&view));
        }

        root.presentViewController_animated_completion(presented, true, None);
        Ok(())
    }

    fn is_supported(&self) -> bool {
        true
    }
}
