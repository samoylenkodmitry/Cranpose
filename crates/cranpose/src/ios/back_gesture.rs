#![expect(unsafe_code)]

use cranpose_services::{BackEdge, BackEvent, BackGesture, push_back_gesture, push_back_request};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained, sel};
use objc2_foundation::{NSObject, NSObjectProtocol};
use objc2_ui_kit::{UIGestureRecognizerState, UIRectEdge, UIScreenEdgePanGestureRecognizer};

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeBackGestureTarget"]
    #[ivars = ()]
    pub(crate) struct BackGestureTarget;

    unsafe impl NSObjectProtocol for BackGestureTarget {}

    impl BackGestureTarget {
        #[unsafe(method(handleEdgePan:))]
        fn handle_edge_pan(&self, recognizer: &UIScreenEdgePanGestureRecognizer) {
            edge_pan_moved(recognizer);
        }
    }
);

/// The share of the view's width past which letting go goes back.
const COMMIT_PROGRESS: f64 = 0.3;
/// The speed, in points per second, of a flick that goes back however short.
const COMMIT_VELOCITY: f64 = 300.0;

/// Hands the app each step of a left-edge pan, with its progress across the
/// view, and ends it in a back request past the commit point or on a flick.
fn edge_pan_moved(recognizer: &UIScreenEdgePanGestureRecognizer) {
    let view = recognizer.view();
    let width = view.as_ref().map_or(0.0, |view| view.bounds().size.width);
    let travel = recognizer.translationInView(view.as_deref()).x;
    let progress = if width > 0.0 {
        (travel / width).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let event = BackEvent {
        progress: progress as f32,
        edge: BackEdge::Left,
    };
    let state = recognizer.state();
    if state == UIGestureRecognizerState::Began {
        push_back_gesture(BackGesture::Started(event));
    } else if state == UIGestureRecognizerState::Changed {
        push_back_gesture(BackGesture::Progressed(event));
    } else if state == UIGestureRecognizerState::Ended {
        let speed = recognizer.velocityInView(view.as_deref()).x;
        if progress >= COMMIT_PROGRESS || speed >= COMMIT_VELOCITY {
            push_back_request();
        } else {
            push_back_gesture(BackGesture::Cancelled);
        }
    } else if state == UIGestureRecognizerState::Cancelled
        || state == UIGestureRecognizerState::Failed
    {
        push_back_gesture(BackGesture::Cancelled);
    }
}

impl BackGestureTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

pub(crate) fn register() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(root) = super::file_picker::root_view_controller(mtm) else {
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
    crate::thread_state::with(|state| *state.ios.back_gesture_target.borrow_mut() = Some(target));
}
