use cranpose_render_common::Renderer;

use crate::AppShell;

#[cfg(all(
    feature = "clipboard-native",
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
#[derive(Default)]
struct SystemClipboard {
    // Opened at the first copy or paste: opened at launch, it held the main
    // thread ~0.8 ms on an Apple M5, for apps that never touch it too.
    inner: std::cell::OnceCell<std::cell::RefCell<Option<arboard::Clipboard>>>,
}

#[cfg(all(
    feature = "clipboard-native",
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
impl SystemClipboard {
    fn with<T>(
        &self,
        use_clipboard: impl FnOnce(&mut arboard::Clipboard) -> Option<T>,
    ) -> Option<T> {
        self.inner
            .get_or_init(|| std::cell::RefCell::new(arboard::Clipboard::new().ok()))
            .borrow_mut()
            .as_mut()
            .and_then(use_clipboard)
    }
}

#[cfg(all(
    feature = "clipboard-native",
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
impl cranpose_ui::clipboard_session::PlatformClipboard for SystemClipboard {
    fn write_text(&self, text: &str) {
        self.with(|clipboard| clipboard.set_text(text).ok());
    }

    fn read_text(&self) -> Option<String> {
        self.with(|clipboard| clipboard.get_text().ok())
    }
}

impl<R: Renderer> AppShell<R> {
    /// Copies and pastes through the system clipboard from now on, as a shell
    /// shown in a window does. A shell that never calls it, such as a test's,
    /// keeps what it copies to itself.
    #[cfg(all(
        feature = "clipboard-native",
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    pub fn use_system_clipboard(&self) {
        self.app.app_context.enter(|| {
            cranpose_ui::clipboard_session::set_platform_clipboard(std::rc::Rc::new(
                SystemClipboard::default(),
            ));
        });
    }

    /// Does nothing: this build reaches the system clipboard through its runner.
    #[cfg(not(all(
        feature = "clipboard-native",
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    )))]
    pub fn use_system_clipboard(&self) {}
}
