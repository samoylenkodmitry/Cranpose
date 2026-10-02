use std::{cell::RefCell, rc::Rc};

use cranpose::{
    native_view::{NativeView as Slot, NativeViewHost},
    prelude::Modifier,
};
use cranpose_core::{CompositionLocal, CompositionLocalProvider, SideEffect, compositionLocalOf};

use crate::NativeEvent;

thread_local! {
    static CONTEXT: CompositionLocal<Option<NativeContext>> = compositionLocalOf(|| None);
}

#[derive(Clone)]
pub(crate) struct NativeContext {
    pub(crate) views: NativeViewHost,
    events: Rc<RefCell<Vec<NativeEvent>>>,
}

impl PartialEq for NativeContext {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.events, &other.events)
    }
}

impl NativeContext {
    pub(crate) fn new() -> Self {
        Self {
            views: NativeViewHost::default(),
            events: Rc::default(),
        }
    }

    pub(crate) fn provide(&self, content: impl FnOnce()) {
        CONTEXT
            .with(|local| CompositionLocalProvider([local.provides(Some(self.clone()))], content));
    }

    pub(crate) fn take_events(&self) -> Vec<NativeEvent> {
        std::mem::take(&mut *self.events.borrow_mut())
    }

    fn current() -> Self {
        CONTEXT
            .with(CompositionLocal::current)
            .expect("native content requires a NativeSession")
    }
}

/// Application content and commands, created on the component's owning thread.
pub struct NativeContent {
    pub(crate) draw: Box<dyn FnMut()>,
    pub(crate) event: Box<dyn FnMut(NativeEvent)>,
}

impl NativeContent {
    /// Supplies a composition. The native runtime provides its host context.
    pub fn new(content: impl FnMut() + 'static) -> Self {
        Self {
            draw: Box::new(content),
            event: Box::new(|_| {}),
        }
    }

    /// Handles application events sent through the platform view.
    pub fn on_event(mut self, event: impl FnMut(NativeEvent) + 'static) -> Self {
        self.event = Box::new(event);
        self
    }
}

/// Sends an application event to the native host after this composition commits.
#[cranpose::prelude::composable]
pub fn SendToHost(name: impl Into<String>, value: impl Into<String>) {
    let context = NativeContext::current();
    let event = NativeEvent {
        name: name.into(),
        value: value.into(),
    };
    SideEffect(move || context.events.borrow_mut().push(event));
}

/// Reserves a native child with an application-defined factory and configuration.
///
/// Factories own creation, updates and disposal. The host preserves the child
/// until its slot is removed or its factory kind changes.
#[cranpose::prelude::composable]
pub fn NativeView(kind: &str, value: &str, modifier: Modifier, on_event: impl Fn(&str) + 'static) {
    Slot(
        NativeContext::current().views,
        kind,
        value,
        modifier,
        on_event,
    );
}

/// An event emitted by the optional platform WebView factory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebViewEvent {
    /// A page finished loading.
    Loaded(String),
    /// A main-frame request failed.
    Failed(String),
}

/// Embeds a website using the registered `web` factory.
///
/// The supplied URL is loaded only when it changes. Navigation and scroll state
/// survive unrelated recompositions. Install `WebViewFactory` in the native host.
/// Both adapters accept HTTPS URLs and report loading failures through this callback.
#[cranpose::prelude::composable]
pub fn WebView(url: &str, modifier: Modifier, on_event: impl Fn(WebViewEvent) + 'static) {
    NativeView("web", url, modifier, move |event| {
        if let Some(url) = event.strip_prefix("loaded:") {
            on_event(WebViewEvent::Loaded(url.to_owned()));
        } else if let Some(message) = event.strip_prefix("error:") {
            on_event(WebViewEvent::Failed(message.to_owned()));
        }
    });
}
