//! Code a desktop app runs on the shell's main thread. It sees each window's
//! raw handles as the window opens and runs before the event loop waits, so
//! a tray icon, a native menu or another platform integration the shell does
//! not provide attaches without forking the shell.

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};
use winit::event_loop::EventLoopProxy;

/// Names a desktop window while it is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DesktopWindowId {
    /// The app's primary window.
    Primary,
    /// A [`NativeWindow`](crate::NativeWindow), by the raw value of its key.
    Native(u64),
}

/// A window of a desktop app, as a [`DesktopIntegration`] call sees it. Its
/// raw handles are valid for that call.
pub struct DesktopWindow<'w> {
    id: DesktopWindowId,
    window: &'w dyn winit::window::Window,
}

impl DesktopWindow<'_> {
    /// Which window this is.
    pub fn id(&self) -> DesktopWindowId {
        self.id
    }
}

impl HasWindowHandle for DesktopWindow<'_> {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.window.window_handle()
    }
}

impl HasDisplayHandle for DesktopWindow<'_> {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.window.display_handle()
    }
}

/// Wakes the desktop event loop from any thread, so that it calls
/// [`DesktopIntegration::about_to_wait`] again: for event sources that do
/// not wake it themselves.
#[derive(Clone)]
pub struct DesktopWaker {
    proxy: EventLoopProxy,
}

impl DesktopWaker {
    /// Wakes the event loop.
    pub fn wake(&self) {
        self.proxy.wake_up();
    }
}

/// What a [`DesktopIntegration`] can do during a call.
pub struct DesktopIntegrationContext<'a> {
    app: &'a mut AppShell<WgpuRenderer>,
    proxy: &'a EventLoopProxy,
}

impl DesktopIntegrationContext<'_> {
    /// Runs `update` as an event handler runs: in the app's context and a
    /// mutable snapshot, so it may write state, and the app draws the frame
    /// the change asks for. Fails when the snapshot does not apply.
    pub fn update<T>(&mut self, update: impl FnOnce() -> T) -> Result<T, &'static str> {
        let _event_handler = cranpose_core::enter_event_handler_scope();
        let result = self
            .app
            .app_context()
            .enter(|| cranpose_core::run_in_mutable_snapshot(update));
        self.app.mark_dirty();
        self.proxy.wake_up();
        result
    }

    /// A waker for the event loop, which may move to another thread.
    pub fn waker(&self) -> DesktopWaker {
        DesktopWaker {
            proxy: self.proxy.clone(),
        }
    }
}

/// Code a desktop app runs on the shell's main thread, given to
/// [`AppLauncher::with_desktop_integration`](crate::AppLauncher::with_desktop_integration).
/// Every call does nothing by default.
pub trait DesktopIntegration: 'static {
    /// `window` opened: the primary window first, then each
    /// [`NativeWindow`](crate::NativeWindow) as it opens.
    fn window_opened(
        &mut self,
        window: &DesktopWindow<'_>,
        context: &mut DesktopIntegrationContext<'_>,
    ) {
        let _ = (window, context);
    }

    /// The native window `id` closed.
    fn window_closed(&mut self, id: DesktopWindowId, context: &mut DesktopIntegrationContext<'_>) {
        let _ = (id, context);
    }

    /// The event loop is about to wait for events. Take the events of the
    /// integration's own sources here, such as a tray icon's clicks.
    fn about_to_wait(&mut self, context: &mut DesktopIntegrationContext<'_>) {
        let _ = context;
    }
}

/// The integration of a running desktop app, if it has one.
#[derive(Default)]
pub(crate) struct Integration(Option<Box<dyn DesktopIntegration>>);

impl Integration {
    pub(crate) fn new(integration: Option<Box<dyn DesktopIntegration>>) -> Self {
        Self(integration)
    }

    pub(crate) fn window_opened(
        &mut self,
        id: DesktopWindowId,
        window: &dyn winit::window::Window,
        app: &mut AppShell<WgpuRenderer>,
        proxy: &EventLoopProxy,
    ) {
        if let Some(integration) = &mut self.0 {
            let window = DesktopWindow { id, window };
            integration.window_opened(&window, &mut DesktopIntegrationContext { app, proxy });
        }
    }

    pub(crate) fn window_closed(
        &mut self,
        id: DesktopWindowId,
        app: &mut AppShell<WgpuRenderer>,
        proxy: &EventLoopProxy,
    ) {
        if let Some(integration) = &mut self.0 {
            integration.window_closed(id, &mut DesktopIntegrationContext { app, proxy });
        }
    }

    pub(crate) fn about_to_wait(
        &mut self,
        app: &mut AppShell<WgpuRenderer>,
        proxy: &EventLoopProxy,
    ) {
        if let Some(integration) = &mut self.0 {
            integration.about_to_wait(&mut DesktopIntegrationContext { app, proxy });
        }
    }
}
