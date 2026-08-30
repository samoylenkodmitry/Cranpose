use std::sync::{Arc, Mutex, OnceLock};

use block2::RcBlock;
use cranpose_services::{BackgroundActivity, set_platform_background_activity};
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;
use objc2_foundation::NSString;
use objc2_ui_kit::{UIApplication, UIApplicationState};

fn task_slot() -> &'static Mutex<Option<usize>> {
    static SLOT: OnceLock<Mutex<Option<usize>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

pub(crate) fn register() {
    set_platform_background_activity(Arc::new(IosBackgroundActivity));
}

struct IosBackgroundActivity;

impl BackgroundActivity for IosBackgroundActivity {
    fn set_active(&self, active: bool) {
        DispatchQueue::main().exec_async(move || {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            if active {
                begin_task(mtm);
            } else {
                end_task(mtm);
            }
        });
    }
}

fn begin_task(mtm: MainThreadMarker) {
    let mut slot = match task_slot().lock() {
        Ok(slot) => slot,
        Err(_) => return,
    };
    if slot.is_some() {
        return;
    }
    let app = UIApplication::sharedApplication(mtm);
    let name = NSString::from_str("cranpose.background-activity");
    let handler = RcBlock::new(|| {
        if let Some(mtm) = MainThreadMarker::new() {
            end_task(mtm);
        }
    });
    let id = app.beginBackgroundTaskWithName_expirationHandler(Some(&name), Some(&handler));
    *slot = Some(id);
}

fn end_task(mtm: MainThreadMarker) {
    let taken = task_slot().lock().ok().and_then(|mut slot| slot.take());
    if let Some(id) = taken {
        UIApplication::sharedApplication(mtm).endBackgroundTask(id);
    }
}

pub(crate) fn app_is_off_screen() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    UIApplication::sharedApplication(mtm).applicationState() == UIApplicationState::Background
}
