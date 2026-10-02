//! Shared runtime and native-language bindings for embedding Cranpose in native views.
mod content;
mod session;
mod worker;

use content::NativeContext;
pub use content::{NativeContent, NativeView, SendToHost, WebView, WebViewEvent};
pub use session::{
    FrameListener, NativeError, NativeEvent, NativeFrame, NativeSession, NativeSlot, TouchPhase,
};

uniffi::setup_scaffolding!();
