use cranpose_ui::{Modifier, composable};

/// An event from an embedded browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebViewEvent {
    /// A page finished loading. In browsers this reports the requested URL:
    /// cross-origin navigation and HTTP status are not exposed by iframe events.
    Loaded(String),
    /// Creating the view or requesting navigation failed.
    Failed(String),
}

/// Embeds a website inside the component's layout bounds.
///
/// Enable Cranpose's `webview` feature for application hosts, or register a
/// `WebViewFactory` when embedding a component in Android/UIKit. The URL loads
/// only when it changes; unrelated recompositions preserve the document.
/// Desktop uses the system WebView (WebKit, WebView2 or WebKitGTK), Android uses
/// WebView, iOS uses WKWebView, and browsers use an iframe. Browser sites must
/// permit framing. Linux requires WebKitGTK 4.1 and X11 or XWayland.
///
/// Native views appear above Cranpose drawing. Use axis-aligned bounds without
/// ancestor transforms or rounded clips, and remove the view before displaying
/// an overlapping Cranpose popup. See [`crate::native_view::NativeViewLayout`].
#[composable]
pub fn WebView(url: &str, modifier: Modifier, on_event: impl Fn(WebViewEvent) + 'static) {
    crate::native_view::NativeView(
        crate::native_view::current_host(),
        "web",
        url,
        modifier,
        move |event| {
            if let Some(url) = event.strip_prefix("loaded:") {
                on_event(WebViewEvent::Loaded(url.to_owned()));
            } else if let Some(message) = event.strip_prefix("error:") {
                on_event(WebViewEvent::Failed(message.to_owned()));
            }
        },
    );
}
