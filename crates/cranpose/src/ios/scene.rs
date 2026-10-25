#![expect(unsafe_code, reason = "Objective-C messages to UIKit")]

use objc2::{
    ClassType, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, rc::Retained,
    runtime::AnyClass,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSSet, NSURL, NSUserActivity};
use objc2_ui_kit::{
    UIOpenURLContext, UIScene, UISceneConnectionOptions, UISceneDelegate, UISceneSession, UIView,
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
            options: &UISceneConnectionOptions,
        ) {
            if let Some(window_scene) = scene.downcast_ref::<UIWindowScene>() {
                crate::thread_state::with(|state| {
                    *state.ios.window_scene.borrow_mut() = Some(window_scene.retain());
                });
                link();
            }
            // iOS 27 answers nil, not an empty set, for a launch with no URL
            // or activity, where the generated getters panic.
            let contexts: Option<Retained<NSSet<UIOpenURLContext>>> =
                unsafe { msg_send![options, URLContexts] };
            if let Some(contexts) = contexts {
                publish_url_contexts(&contexts);
            }
            let activities: Option<Retained<NSSet<NSUserActivity>>> =
                unsafe { msg_send![options, userActivities] };
            for activity in activities.iter().flat_map(|activities| activities.iter()) {
                publish_web_link(&activity);
            }
        }

        #[unsafe(method(scene:openURLContexts:))]
        fn scene_open_url_contexts(&self, _scene: &UIScene, contexts: &NSSet<UIOpenURLContext>) {
            publish_url_contexts(contexts);
        }

        #[unsafe(method(scene:continueUserActivity:))]
        fn scene_continue_user_activity(&self, _scene: &UIScene, activity: &NSUserActivity) {
            publish_web_link(activity);
        }
    }

    unsafe impl UIWindowSceneDelegate for SceneDelegate {}
);

/// Publishes what the scene was asked to open: a document from Files or
/// another app as incoming content, and a URL in one of the app's schemes as
/// an opened link.
fn publish_url_contexts(contexts: &NSSet<UIOpenURLContext>) {
    for context in contexts {
        let url = context.URL();
        match url.path().filter(|_| url.isFileURL()) {
            Some(path) => cranpose_services::publish_incoming_content(
                cranpose_services::IncomingContent::from_file(std::path::Path::new(
                    &path.to_string(),
                )),
            ),
            None => publish_url(&url),
        }
    }
}

/// Publishes the web page of a universal link the scene continues.
fn publish_web_link(activity: &NSUserActivity) {
    if let Some(url) = activity.webpageURL() {
        publish_url(&url);
    }
}

fn publish_url(url: &NSURL) {
    if let Some(text) = url.absoluteString() {
        cranpose_services::publish_opened_link(text.to_string());
    }
}

impl SceneDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
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
    crate::thread_state::with(|state| *state.ios.window.borrow_mut() = Some(ui_window));
    link();
}

fn link() {
    if MainThreadMarker::new().is_none() {
        return;
    }
    crate::thread_state::with(|state| {
        let scene_borrow = state.ios.window_scene.borrow();
        let Some(scene) = scene_borrow.as_ref() else {
            return;
        };
        let window_borrow = state.ios.window.borrow();
        let Some(window) = window_borrow.as_ref() else {
            return;
        };
        if window.windowScene().is_none() {
            window.setWindowScene(Some(scene));
            window.makeKeyAndVisible();
        }
    });
}
