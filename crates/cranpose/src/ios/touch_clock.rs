#![expect(
    unsafe_code,
    reason = "an Objective-C gesture recognizer class for UIKit"
)]

use std::cell::RefCell;

use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Sel},
};
use objc2_foundation::{NSObjectProtocol, NSSet};
use objc2_ui_kit::{UIEvent, UIGestureRecognizer, UIGestureRecognizerState, UITouch, UITouchPhase};
use winit::{event::FingerId, window::Window};

// The latest sample of one touch, keyed by the `UITouch` address winit
// names its finger by.
#[derive(Clone, Copy)]
pub(crate) struct Sample {
    touch: usize,
    uptime_ms: i64,
    ended: bool,
}

define_class!(
    // UIKit hands a touch to the view's gesture recognizers before the view,
    // so this recognizer holds the time of each touch sample winit is about
    // to deliver. It never recognizes, and it neither delays nor cancels the
    // view's touches.
    #[unsafe(super(UIGestureRecognizer))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeTouchClock"]
    #[ivars = RefCell<Vec<Sample>>]
    pub(crate) struct TouchClock;

    unsafe impl NSObjectProtocol for TouchClock {}

    impl TouchClock {
        #[unsafe(method(touchesBegan:withEvent:))]
        fn touches_began(&self, touches: &NSSet<UITouch>, event: &UIEvent) {
            self.ivars().borrow_mut().retain(|sample| !sample.ended);
            self.record(touches);
            unsafe { msg_send![super(self), touchesBegan: touches, withEvent: event] }
        }

        #[unsafe(method(touchesMoved:withEvent:))]
        fn touches_moved(&self, touches: &NSSet<UITouch>, event: &UIEvent) {
            self.record(touches);
            unsafe { msg_send![super(self), touchesMoved: touches, withEvent: event] }
        }

        #[unsafe(method(touchesEnded:withEvent:))]
        fn touches_ended(&self, touches: &NSSet<UITouch>, event: &UIEvent) {
            self.record(touches);
            unsafe { msg_send![super(self), touchesEnded: touches, withEvent: event] }
            self.fail_when_all_ended();
        }

        #[unsafe(method(touchesCancelled:withEvent:))]
        fn touches_cancelled(&self, touches: &NSSet<UITouch>, event: &UIEvent) {
            self.record(touches);
            unsafe { msg_send![super(self), touchesCancelled: touches, withEvent: event] }
            self.fail_when_all_ended();
        }

        #[unsafe(method(canPreventGestureRecognizer:))]
        fn can_prevent(&self, _other: &UIGestureRecognizer) -> bool {
            false
        }

        #[unsafe(method(canBePreventedByGestureRecognizer:))]
        fn can_be_prevented(&self, _other: &UIGestureRecognizer) -> bool {
            false
        }
    }
);

impl TouchClock {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RefCell::new(Vec::new()));
        unsafe { msg_send![super(this), initWithTarget: None::<&AnyObject>, action: None::<Sel>] }
    }

    fn record(&self, touches: &NSSet<UITouch>) {
        let mut samples = self.ivars().borrow_mut();
        for touch in touches {
            let phase = touch.phase();
            let sample = Sample {
                touch: Retained::as_ptr(&touch) as usize,
                uptime_ms: (touch.timestamp() * 1000.0).round() as i64,
                ended: phase == UITouchPhase::Ended || phase == UITouchPhase::Cancelled,
            };
            match samples.iter_mut().find(|known| known.touch == sample.touch) {
                Some(known) => *known = sample,
                None => samples.push(sample),
            }
        }
    }

    // A recognizer left possible keeps UIKit waiting on it; once every touch
    // it follows has ended, it steps aside for the next touch sequence. The
    // samples stay for the view, which takes the same touches after it.
    fn fail_when_all_ended(&self) {
        if self.ivars().borrow().iter().all(|sample| sample.ended) {
            self.setState(UIGestureRecognizerState::Failed);
        }
    }

    pub(crate) fn time_ms(&self, finger: FingerId) -> Option<i64> {
        let touch = finger.into_raw();
        self.ivars()
            .borrow()
            .iter()
            .find(|sample| sample.touch == touch)
            .map(|sample| sample.uptime_ms)
    }
}

pub(crate) fn install(window: &dyn Window) -> Option<Retained<TouchClock>> {
    let mtm = MainThreadMarker::new()?;
    let view = super::scene::winit_view(window)?;
    let clock = TouchClock::new(mtm);
    clock.setCancelsTouchesInView(false);
    clock.setDelaysTouchesBegan(false);
    clock.setDelaysTouchesEnded(false);
    view.addGestureRecognizer(&clock);
    Some(clock)
}
