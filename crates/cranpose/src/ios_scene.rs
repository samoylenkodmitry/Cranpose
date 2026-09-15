#![allow(unsafe_code)]

use std::cell::RefCell;

use objc2::{
    ClassType, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, rc::Retained,
    runtime::AnyClass,
};
use objc2_foundation::{NSObject, NSObjectProtocol};
use objc2_ui_kit::{
    UIScene, UISceneConnectionOptions, UISceneDelegate, UISceneSession, UIView, UIWindow,
    UIWindowScene, UIWindowSceneDelegate,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeSceneDelegate"]
    #[ivars = ()]
    struct SceneDelegate;

    unsafe impl NSObjectProtocol for SceneDelegate {}

    unsafe impl UISceneDelegate for SceneDelegate {
        #[unsafe(method(scene:willConnectToSession:options:))]
        fn scene_will_connect(
            &self,
            scene: &UIScene,
            _session: &UISceneSession,
            _options: &UISceneConnectionOptions,
        ) {
            if let Some(window_scene) = scene.downcast_ref::<UIWindowScene>() {
                SCENE.with(|cell| *cell.borrow_mut() = Some(window_scene.retain()));
                link();
            }
        }
    }

    unsafe impl UIWindowSceneDelegate for SceneDelegate {}
);

impl SceneDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    static SCENE: RefCell<Option<Retained<UIWindowScene>>> = const { RefCell::new(None) };
    static WINDOW: RefCell<Option<Retained<UIWindow>>> = const { RefCell::new(None) };
}

pub(crate) fn register_class() {
    let _: &AnyClass = SceneDelegate::class();
    if let Some(mtm) = MainThreadMarker::new() {
        let _ = SceneDelegate::new(mtm);
    }
}

pub(crate) fn hold_window(window: &dyn Window) {
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::UiKit(uikit) = handle.as_raw() else {
        return;
    };
    let view: *mut UIView = uikit.ui_view.as_ptr().cast();
    let ui_window = unsafe { (*view).window() };
    let Some(ui_window) = ui_window else {
        return;
    };
    WINDOW.with(|cell| *cell.borrow_mut() = Some(ui_window));
    link();
}

fn link() {
    if MainThreadMarker::new().is_none() {
        return;
    }
    SCENE.with(|scene_cell| {
        let scene_borrow = scene_cell.borrow();
        let Some(scene) = scene_borrow.as_ref() else {
            return;
        };
        WINDOW.with(|window_cell| {
            let window_borrow = window_cell.borrow();
            let Some(window) = window_borrow.as_ref() else {
                return;
            };
            if window.windowScene().is_none() {
                window.setWindowScene(Some(scene));
                window.makeKeyAndVisible();
            }
        });
    });
}
