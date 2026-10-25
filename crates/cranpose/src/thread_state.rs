use std::cell::{OnceCell, RefCell};

use cranpose_core::CompositionLocal;

use crate::{
    android_host_window::AndroidHostWindowRegistry,
    native_view::NativeViewHost,
    native_window::{NativeWindowDispatchContext, WindowState},
    scoped_weak_stack::ScopedWeakStack,
};

pub(crate) struct CranposeThreadState {
    pub(crate) window_state_local: OnceCell<CompositionLocal<Option<WindowState>>>,
    pub(crate) native_view_host: OnceCell<CompositionLocal<NativeViewHost>>,
    pub(crate) native_window_dispatch: RefCell<Vec<NativeWindowDispatchContext>>,
    #[cfg(all(
        feature = "native-windows",
        feature = "renderer-wgpu",
        not(target_arch = "wasm32")
    ))]
    pub(crate) native_window_registry: ScopedWeakStack<crate::native_window::NativeWindowRegistry>,
    #[cfg_attr(
        not(any(test, all(feature = "android", target_os = "android"))),
        expect(dead_code, reason = "only the Android backend registers host windows")
    )]
    pub(crate) android_host_window_registry: ScopedWeakStack<AndroidHostWindowRegistry>,
    #[cfg(feature = "localization")]
    pub(crate) system_languages_local:
        OnceCell<CompositionLocal<std::rc::Rc<[cranpose_ui::Locale]>>>,
    #[cfg(feature = "renderer-wgpu")]
    pub(crate) gpu_context_local:
        OnceCell<cranpose_core::StaticCompositionLocal<Option<crate::GpuContext>>>,
    #[cfg(debug_assertions)]
    #[cfg_attr(
        not(any(
            test,
            all(feature = "desktop-shell", feature = "renderer-wgpu"),
            all(feature = "android", feature = "renderer-wgpu", target_os = "android"),
            all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
            all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32")
        )),
        expect(
            dead_code,
            reason = "only the accessibility bridges warn about unlabeled controls"
        )
    )]
    pub(crate) unlabeled_controls_warned: RefCell<std::collections::HashSet<cranpose_core::NodeId>>,
    #[cfg(all(feature = "android", target_os = "android"))]
    pub(crate) android_font_scale: std::cell::Cell<cranpose_ui::FontScaleCurve>,
    #[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
    pub(crate) android_platform_env:
        OnceCell<std::rc::Rc<crate::platform_env::PlatformEnvironment>>,
    #[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
    pub(crate) android_ime_density: std::cell::Cell<f32>,
    #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
    pub(crate) pacing_diag: RefCell<crate::desktop::PacingDiag>,
    /// The tray and menu bar the composition asks for.
    #[cfg(all(
        feature = "desktop-menus",
        feature = "renderer-wgpu",
        any(
            target_os = "macos",
            target_os = "windows",
            all(target_os = "linux", not(target_arch = "wasm32"))
        )
    ))]
    pub(crate) native_menus: RefCell<crate::desktop::menus::MenuRequests>,
    #[cfg(all(
        feature = "desktop-menus",
        feature = "renderer-wgpu",
        any(
            target_os = "macos",
            target_os = "windows",
            all(target_os = "linux", not(target_arch = "wasm32"))
        )
    ))]
    pub(crate) native_menus_next_owner: std::cell::Cell<u64>,
    #[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
    pub(crate) ios: IosThreadState,
    #[cfg(all(feature = "watchos", feature = "heart-rate", target_os = "watchos"))]
    pub(crate) heart_rate: crate::watchos_heart_rate::HeartRateThreadState,
    #[cfg(all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"))]
    pub(crate) web: WebThreadState,
}

impl CranposeThreadState {
    fn new() -> Self {
        Self {
            window_state_local: OnceCell::new(),
            native_view_host: OnceCell::new(),
            native_window_dispatch: RefCell::new(Vec::new()),
            #[cfg(all(
                feature = "native-windows",
                feature = "renderer-wgpu",
                not(target_arch = "wasm32")
            ))]
            native_window_registry: ScopedWeakStack::new(),
            android_host_window_registry: ScopedWeakStack::new(),
            #[cfg(feature = "localization")]
            system_languages_local: OnceCell::new(),
            #[cfg(feature = "renderer-wgpu")]
            gpu_context_local: OnceCell::new(),
            #[cfg(debug_assertions)]
            unlabeled_controls_warned: RefCell::new(std::collections::HashSet::new()),
            #[cfg(all(feature = "android", target_os = "android"))]
            android_font_scale: std::cell::Cell::new(cranpose_ui::FontScaleCurve::linear(1.0)),
            #[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
            android_platform_env: OnceCell::new(),
            #[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
            android_ime_density: std::cell::Cell::new(1.0),
            #[cfg(all(feature = "desktop-shell", feature = "renderer-wgpu"))]
            pacing_diag: RefCell::new(crate::desktop::PacingDiag::default()),
            #[cfg(all(
                feature = "desktop-menus",
                feature = "renderer-wgpu",
                any(
                    target_os = "macos",
                    target_os = "windows",
                    all(target_os = "linux", not(target_arch = "wasm32"))
                )
            ))]
            native_menus: RefCell::new(crate::desktop::menus::MenuRequests::default()),
            #[cfg(all(
                feature = "desktop-menus",
                feature = "renderer-wgpu",
                any(
                    target_os = "macos",
                    target_os = "windows",
                    all(target_os = "linux", not(target_arch = "wasm32"))
                )
            ))]
            native_menus_next_owner: std::cell::Cell::new(0),
            #[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
            ios: IosThreadState::new(),
            #[cfg(all(feature = "watchos", feature = "heart-rate", target_os = "watchos"))]
            heart_rate: crate::watchos_heart_rate::HeartRateThreadState::new(),
            #[cfg(all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"))]
            web: WebThreadState::new(),
        }
    }
}

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
pub(crate) struct IosThreadState {
    pub(crate) back_gesture_target:
        RefCell<Option<objc2::rc::Retained<crate::ios::back_gesture::BackGestureTarget>>>,
    pub(crate) text_field_traits: OnceCell<objc2_ui_kit::UIAccessibilityTraits>,
    pub(crate) notification_delegate:
        RefCell<Option<objc2::rc::Retained<crate::ios::notifier::NotificationDelegate>>>,
    pub(crate) window_scene: RefCell<Option<objc2::rc::Retained<objc2_ui_kit::UIWindowScene>>>,
    pub(crate) window: RefCell<Option<objc2::rc::Retained<objc2_ui_kit::UIWindow>>>,
    pub(crate) key_input_view:
        RefCell<Option<objc2::rc::Retained<crate::ios::keyboard::KeyInputView>>>,
}

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
impl IosThreadState {
    const fn new() -> Self {
        Self {
            back_gesture_target: RefCell::new(None),
            text_field_traits: OnceCell::new(),
            notification_delegate: RefCell::new(None),
            window_scene: RefCell::new(None),
            window: RefCell::new(None),
            key_input_view: RefCell::new(None),
        }
    }
}

#[cfg(all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"))]
pub(crate) struct WebThreadState {
    pub(crate) wheel_epoch: OnceCell<web_time::Instant>,
    pub(crate) frame_canvas: RefCell<Option<web_sys::HtmlCanvasElement>>,
    pub(crate) frame_asked_of: RefCell<Option<web_sys::Window>>,
    pub(crate) host_surface_wake: RefCell<Option<std::rc::Rc<dyn Fn()>>>,
    pub(crate) media_browser: RefCell<Option<crate::web::media::Browser>>,
    pub(crate) active_notifications:
        RefCell<std::collections::HashMap<String, crate::web::services::ActiveNotification>>,
}

#[cfg(all(feature = "web", feature = "renderer-wgpu", target_arch = "wasm32"))]
impl WebThreadState {
    fn new() -> Self {
        Self {
            wheel_epoch: OnceCell::new(),
            frame_canvas: RefCell::new(None),
            frame_asked_of: RefCell::new(None),
            host_surface_wake: RefCell::new(None),
            media_browser: RefCell::new(None),
            active_notifications: RefCell::new(std::collections::HashMap::new()),
        }
    }
}

thread_local! {
    static STATE: CranposeThreadState = CranposeThreadState::new();
}

pub(crate) fn with<R>(f: impl FnOnce(&CranposeThreadState) -> R) -> R {
    cranpose_core::with_or_fresh(&STATE, CranposeThreadState::new, f)
}
