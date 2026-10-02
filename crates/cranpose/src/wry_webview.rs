use std::rc::Rc;

use cranpose_ui::Rect;
use raw_window_handle::HasWindowHandle;

use crate::{
    native_view::NativeViewHost,
    webview_host::{BrowserView, WebViews},
};

pub(crate) fn wake_callback(proxy: &winit::event_loop::EventLoopProxy) -> Rc<dyn Fn()> {
    let proxy = proxy.clone();
    Rc::new(move || proxy.wake_up())
}

impl BrowserView for wry::WebView {
    fn navigate(&mut self, url: &str) -> Result<(), String> {
        self.load_url(url).map_err(|error| error.to_string())
    }

    fn place(&mut self, bounds: Rect) -> Result<(), String> {
        #[cfg(not(target_os = "ios"))]
        self.set_bounds(wry_bounds(bounds))
            .map_err(|error| error.to_string())?;
        #[cfg(target_os = "ios")]
        {
            use objc2_core_foundation::{CGPoint, CGRect, CGSize};
            use wry::WebViewExtIOS;
            self.webview().setFrame(CGRect::new(
                CGPoint::new(f64::from(bounds.x), f64::from(bounds.y)),
                CGSize::new(
                    f64::from(bounds.width.max(0.0)),
                    f64::from(bounds.height.max(0.0)),
                ),
            ));
        }
        self.set_visible(bounds.width > 0.0 && bounds.height > 0.0)
            .map_err(|error| error.to_string())
    }
}

fn wry_bounds(bounds: Rect) -> wry::Rect {
    wry::Rect {
        position: wry::dpi::LogicalPosition::new(bounds.x, bounds.y).into(),
        size: wry::dpi::LogicalSize::new(bounds.width.max(0.0), bounds.height.max(0.0)).into(),
    }
}

pub(crate) fn sync<R: cranpose_render_common::Renderer>(
    views: &mut WebViews<wry::WebView>,
    host: &NativeViewHost,
    shell: Option<&mut cranpose_app_shell::AppShell<R>>,
    window: Option<&impl HasWindowHandle>,
    wake: &Rc<dyn Fn()>,
) where
    R::Error: std::fmt::Debug,
{
    if host.is_empty() && views.is_empty() {
        return;
    }
    let (Some(shell), Some(window)) = (shell, window) else {
        return;
    };
    views.sync(host, shell.layout_tree(), |id, url, bounds, events| {
        #[cfg(target_os = "linux")]
        gtk::init().map_err(|error| error.to_string())?;
        let events = events.clone();
        let wake = Rc::clone(wake);
        let builder = wry::WebViewBuilder::new()
            .with_bounds(wry_bounds(bounds))
            .with_url(url)
            .with_on_page_load_handler(move |event, url| {
                if matches!(event, wry::PageLoadEvent::Finished) {
                    let _ = events.send((id, format!("loaded:{url}")));
                    wake();
                }
            });
        #[cfg(not(target_os = "ios"))]
        let mut view = builder
            .build_as_child(window)
            .map_err(|error| error.to_string())?;
        #[cfg(target_os = "ios")]
        let mut view = builder.build(window).map_err(|error| error.to_string())?;
        view.place(bounds)?;
        Ok(view)
    });
}

#[cfg(target_os = "linux")]
pub(crate) fn pump(views: &WebViews<wry::WebView>) -> Option<web_time::Instant> {
    views.views().next()?;
    if gtk::is_initialized() {
        for _ in 0..32 {
            if !gtk::events_pending() {
                break;
            }
            gtk::main_iteration_do(false);
        }
    }
    Some(web_time::Instant::now() + std::time::Duration::from_millis(16))
}
