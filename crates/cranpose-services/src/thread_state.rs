#[cfg(target_arch = "wasm32")]
use std::rc::Rc;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

use crate::{
    accessibility_state::AccessibilityState,
    app_info::AppInfoRef,
    browser_history::BrowserHistoryRef,
    composition_locals::ServiceLocals,
    content::ContentResolverRef,
    device_info::DeviceInfoRef,
    durable_save::LocalSaves,
    file_picker::{FilePickerRef, RecoveredPick},
    share_sheet::ShareSheetRef,
    theme::SystemTheme,
    uri_handler::UriHandlerRef,
};

pub(crate) struct ServiceThreadState {
    pub(crate) locals: ServiceLocals,
    pub(crate) app_info: RefCell<Option<AppInfoRef>>,
    pub(crate) device_info: RefCell<Option<DeviceInfoRef>>,
    pub(crate) content_resolver: RefCell<Option<ContentResolverRef>>,
    pub(crate) file_picker: RefCell<Option<FilePickerRef>>,
    pub(crate) share_sheet: RefCell<Option<ShareSheetRef>>,
    pub(crate) browser_history: RefCell<Option<BrowserHistoryRef>>,
    pub(crate) uri_handler: RefCell<Option<UriHandlerRef>>,
    pub(crate) accessibility_state: Cell<AccessibilityState>,
    pub(crate) platform_system_theme: Cell<Option<SystemTheme>>,
    pub(crate) detected_system_theme: Cell<Option<SystemTheme>>,
    pub(crate) local_saves: RefCell<LocalSaves>,
    pub(crate) recovered_picks: RefCell<HashMap<String, RecoveredPick>>,
    pub(crate) in_flight_request: RefCell<Option<String>>,
    pub(crate) registered_request_keys: RefCell<HashMap<String, usize>>,
    /// The browser's shared image loader, made on first use.
    #[cfg(all(target_arch = "wasm32", feature = "image-loading"))]
    pub(crate) image_loader: RefCell<Option<crate::image_loading::ImageLoader>>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) lifecycle_observers: Listeners<dyn Fn(crate::host::LifecycleEvent)>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) app_update_observers: Listeners<dyn Fn(crate::app_update::AppUpdateStatus)>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) back_listeners: Listeners<dyn Fn(crate::navigation::BackSignal)>,
    #[cfg(target_arch = "wasm32")]
    pub(crate) store_listeners: Listeners<dyn Fn()>,
}

impl ServiceThreadState {
    fn new() -> Self {
        Self {
            locals: ServiceLocals::default(),
            app_info: RefCell::new(None),
            device_info: RefCell::new(None),
            content_resolver: RefCell::new(None),
            file_picker: RefCell::new(None),
            share_sheet: RefCell::new(None),
            browser_history: RefCell::new(None),
            uri_handler: RefCell::new(None),
            accessibility_state: Cell::new(AccessibilityState {
                screen_reader_on: false,
            }),
            platform_system_theme: Cell::new(None),
            detected_system_theme: Cell::new(None),
            local_saves: RefCell::default(),
            recovered_picks: RefCell::new(HashMap::new()),
            in_flight_request: RefCell::new(None),
            registered_request_keys: RefCell::new(HashMap::new()),
            #[cfg(all(target_arch = "wasm32", feature = "image-loading"))]
            image_loader: RefCell::new(None),
            #[cfg(target_arch = "wasm32")]
            lifecycle_observers: Listeners::new(),
            #[cfg(target_arch = "wasm32")]
            app_update_observers: Listeners::new(),
            #[cfg(target_arch = "wasm32")]
            back_listeners: Listeners::new(),
            #[cfg(target_arch = "wasm32")]
            store_listeners: Listeners::new(),
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) struct Listeners<F: ?Sized>(RefCell<Vec<(u64, Rc<F>)>>);

#[cfg(target_arch = "wasm32")]
impl<F: ?Sized> Listeners<F> {
    fn new() -> Self {
        Self(RefCell::new(Vec::new()))
    }

    pub(crate) fn add(&self, id: u64, listener: Rc<F>) {
        self.0.borrow_mut().push((id, listener));
    }

    pub(crate) fn remove(&self, id: u64) {
        self.0.borrow_mut().retain(|(held, _)| *held != id);
    }

    pub(crate) fn all(&self) -> Vec<Rc<F>> {
        self.0
            .borrow()
            .iter()
            .map(|(_, listener)| Rc::clone(listener))
            .collect()
    }

    pub(crate) fn last(&self) -> Option<Rc<F>> {
        self.0
            .borrow()
            .last()
            .map(|(_, listener)| Rc::clone(listener))
    }
}

thread_local! {
    static STATE: ServiceThreadState = ServiceThreadState::new();
}

pub(crate) fn with<R>(f: impl FnOnce(&ServiceThreadState) -> R) -> R {
    cranpose_core::with_or_fresh(&STATE, ServiceThreadState::new, f)
}

pub(crate) fn try_with<R>(f: impl FnOnce(&ServiceThreadState) -> R) -> Option<R> {
    STATE.try_with(f).ok()
}
