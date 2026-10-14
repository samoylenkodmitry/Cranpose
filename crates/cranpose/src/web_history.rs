//! The page's session history: `window.history` entries that keep a depth in
//! their state, the URL fragment, and `popstate` for the user's moves.

use std::rc::Rc;

use cranpose_services::{BrowserHistory, BrowserHistoryObserver, set_platform_browser_history};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};

pub(crate) fn register() {
    set_platform_browser_history(Rc::new(WebHistory));
}

struct WebHistory;

impl WebHistory {
    fn write(&self, depth: usize, fragment: &str, push: bool) {
        let Some(history) = web_sys::window().and_then(|window| window.history().ok()) else {
            return;
        };
        let state = JsValue::from_f64(depth as f64);
        let url = format!("#{fragment}");
        let written = if push {
            history.push_state_with_url(&state, "", Some(&url))
        } else {
            history.replace_state_with_url(&state, "", Some(&url))
        };
        if let Err(error) = written {
            log::warn!("cranpose: could not write the browser history: {error:?}");
        }
    }
}

impl BrowserHistory for WebHistory {
    fn fragment(&self) -> String {
        web_sys::window()
            .and_then(|window| window.location().hash().ok())
            .map(|hash| hash.strip_prefix('#').unwrap_or(&hash).to_owned())
            .unwrap_or_default()
    }

    fn depth(&self) -> Option<usize> {
        let state = web_sys::window()?.history().ok()?.state().ok()?;
        let depth = state.as_f64()?;
        (depth >= 1.0 && depth.fract() == 0.0).then_some(depth as usize)
    }

    fn push(&self, depth: usize, fragment: &str) {
        self.write(depth, fragment, true);
    }

    fn replace(&self, depth: usize, fragment: &str) {
        self.write(depth, fragment, false);
    }

    fn go(&self, delta: isize) {
        let Some(history) = web_sys::window().and_then(|window| window.history().ok()) else {
            return;
        };
        let delta = i32::try_from(delta).unwrap_or(if delta < 0 { i32::MIN } else { i32::MAX });
        if let Err(error) = history.go_with_delta(delta) {
            log::warn!("cranpose: could not move through the browser history: {error:?}");
        }
    }

    fn observe(&self, on_move: Rc<dyn Fn()>) -> BrowserHistoryObserver {
        let Some(window) = web_sys::window() else {
            return BrowserHistoryObserver::new(());
        };
        let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| on_move());
        if let Err(error) =
            window.add_event_listener_with_callback("popstate", listener.as_ref().unchecked_ref())
        {
            log::warn!("cranpose: could not follow the browser history: {error:?}");
        }
        BrowserHistoryObserver::new(PopStateListener { window, listener })
    }
}

/// Removes its `popstate` listener when dropped.
struct PopStateListener {
    window: web_sys::Window,
    listener: Closure<dyn FnMut(web_sys::Event)>,
}

impl Drop for PopStateListener {
    fn drop(&mut self) {
        let _ = self.window.remove_event_listener_with_callback(
            "popstate",
            self.listener.as_ref().unchecked_ref(),
        );
    }
}
