//! iOS system "back": a left-edge swipe recognized on the root view feeds
//! [`cranpose_services::push_back_request`], so apps handle back the same way
//! across Android and iOS.
#![allow(unsafe_code)]

use cranpose_services::push_back_request;
use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSObject, NSObjectProtocol};
use objc2_ui_kit::{UIGestureRecognizerState, UIRectEdge, UIScreenEdgePanGestureRecognizer};
use std::cell::RefCell;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeBackGestureTarget"]
    #[ivars = ()]
    struct BackGestureTarget;

    unsafe impl NSObjectProtocol for BackGestureTarget {}

    impl BackGestureTarget {
        #[unsafe(method(handleEdgePan:))]
        fn handle_edge_pan(&self, recognizer: &UIScreenEdgePanGestureRecognizer) {
            // One back per completed edge swipe.
            if recognizer.state() == UIGestureRecognizerState::Ended {
                push_back_request();
            }
        }
    }
);

impl BackGestureTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    /// Keeps the gesture target alive (the recognizer holds it weakly).
    static TARGET: RefCell<Option<Retained<BackGestureTarget>>> = const { RefCell::new(None) };
}

/// Installs the left-edge swipe-back recognizer on the app's root view.
pub(crate) fn register() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(root) = crate::ios_file_picker::root_view_controller(mtm) else {
        return;
    };
    let Some(root_view) = root.view() else {
        return;
    };
    let target = BackGestureTarget::new(mtm);
    let recognizer = unsafe {
        UIScreenEdgePanGestureRecognizer::initWithTarget_action(
            UIScreenEdgePanGestureRecognizer::alloc(mtm),
            Some(&*target),
            Some(sel!(handleEdgePan:)),
        )
    };
    recognizer.setEdges(UIRectEdge::Left);
    root_view.addGestureRecognizer(&recognizer);
    TARGET.with(|cell| *cell.borrow_mut() = Some(target));
}
