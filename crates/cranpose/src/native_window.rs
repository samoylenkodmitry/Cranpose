//! Declarative native window support.

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
use std::cell::Cell;
use std::cell::RefCell;
#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
use std::collections::HashMap;
#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
use std::hash::{Hash, Hasher};
use std::rc::Rc;
#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
use std::rc::Weak;

use cranpose_core::MutableState;
use cranpose_ui::{composable, Modifier, Point, PointerEventKind, PointerInputScope, Size};

/// A stable identifier for a declarative native window.
#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NativeWindowKey(u64);

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
impl NativeWindowKey {
    /// Creates a native window key from a static application identifier.
    pub fn from_static(id: &'static str) -> Self {
        Self(hash_id(id))
    }
}

/// Coordinate space used by a native window's configured position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeWindowPositionOrigin {
    /// Position is expressed in operating-system screen coordinates.
    Screen,
    /// Position is expressed relative to the owning application window.
    HostWindow,
}

/// Configuration for a declarative native window.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeWindowOptions {
    /// Window title exposed to the operating system.
    pub title: String,
    /// Initial content width in logical pixels.
    pub width: f32,
    /// Initial content height in logical pixels.
    pub height: f32,
    /// Optional initial outer-window x position in the configured coordinate space.
    pub x: Option<f32>,
    /// Optional initial outer-window y position in the configured coordinate space.
    pub y: Option<f32>,
    /// Coordinate space used by `x` and `y`.
    pub position_origin: NativeWindowPositionOrigin,
    /// Whether the operating system should draw window decorations.
    pub decorations: bool,
    /// Whether the window surface requests compositor transparency.
    pub transparent: bool,
    /// Whether the operating system should allow interactive resizing.
    pub resizable: bool,
    /// Whether the window should be visible when created.
    pub visible: bool,
    /// Whether the window should be kept above normal windows.
    pub always_on_top: bool,
    /// Optional minimum content width in logical pixels.
    pub min_width: Option<f32>,
    /// Optional minimum content height in logical pixels.
    pub min_height: Option<f32>,
    /// Optional maximum content width in logical pixels.
    pub max_width: Option<f32>,
    /// Optional maximum content height in logical pixels.
    pub max_height: Option<f32>,
}

impl NativeWindowOptions {
    /// Creates a decorated native window with a fixed initial size.
    pub fn new(title: impl Into<String>, width: f32, height: f32) -> Self {
        Self {
            title: title.into(),
            width,
            height,
            x: None,
            y: None,
            position_origin: NativeWindowPositionOrigin::Screen,
            decorations: true,
            transparent: false,
            resizable: true,
            visible: true,
            always_on_top: false,
            min_width: None,
            min_height: None,
            max_width: None,
            max_height: None,
        }
    }

    /// Creates a borderless, non-resizable native window with a fixed initial size.
    pub fn borderless(title: impl Into<String>, width: f32, height: f32) -> Self {
        Self {
            decorations: false,
            resizable: false,
            ..Self::new(title, width, height)
        }
    }

    /// Sets the initial outer-window position in logical screen coordinates.
    pub fn with_position(mut self, x: f32, y: f32) -> Self {
        self.x = Some(x);
        self.y = Some(y);
        self.position_origin = NativeWindowPositionOrigin::Screen;
        self
    }

    /// Sets the initial outer-window position relative to the host application window.
    pub fn with_host_window_position(mut self, x: f32, y: f32) -> Self {
        self.x = Some(x);
        self.y = Some(y);
        self.position_origin = NativeWindowPositionOrigin::HostWindow;
        self
    }

    /// Sets whether compositor transparency should be requested.
    pub fn with_transparent(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
        self
    }

    /// Sets whether the operating system should allow interactive resizing.
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Sets whether the window should be visible when created.
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Sets whether the window should be kept above normal windows.
    pub fn with_always_on_top(mut self, always_on_top: bool) -> Self {
        self.always_on_top = always_on_top;
        self
    }

    /// Sets the minimum content size in logical pixels.
    pub fn with_min_size(mut self, width: f32, height: f32) -> Self {
        self.min_width = Some(width);
        self.min_height = Some(height);
        self
    }

    /// Sets the maximum content size in logical pixels.
    pub fn with_max_size(mut self, width: f32, height: f32) -> Self {
        self.max_width = Some(width);
        self.max_height = Some(height);
        self
    }
}

/// Event callbacks emitted by a declarative native window.
#[derive(Clone, Default)]
pub(crate) struct NativeWindowEvents {
    pub(crate) on_moved: Option<Rc<dyn Fn(f32, f32)>>,
    pub(crate) on_resized: Option<Rc<dyn Fn(f32, f32)>>,
    pub(crate) on_close_requested: Option<Rc<dyn Fn()>>,
}

impl NativeWindowEvents {
    fn new() -> Self {
        Self::default()
    }

    fn with_on_moved(mut self, callback: impl Fn(f32, f32) + 'static) -> Self {
        let next = Rc::new(callback);
        self.on_moved = Some(match self.on_moved.take() {
            Some(previous) => Rc::new(move |x, y| {
                previous(x, y);
                next(x, y);
            }),
            None => next,
        });
        self
    }

    fn with_on_resized(mut self, callback: impl Fn(f32, f32) + 'static) -> Self {
        let next = Rc::new(callback);
        self.on_resized = Some(match self.on_resized.take() {
            Some(previous) => Rc::new(move |width, height| {
                previous(width, height);
                next(width, height);
            }),
            None => next,
        });
        self
    }

    fn with_on_close_requested(mut self, callback: impl Fn() + 'static) -> Self {
        let next = Rc::new(callback);
        self.on_close_requested = Some(match self.on_close_requested.take() {
            Some(previous) => Rc::new(move || {
                previous();
                next();
            }),
            None => next,
        });
        self
    }
}

/// Mutable position and size state for a declarative OS window.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct WindowState {
    position: MutableState<Option<Point>>,
    size: MutableState<Size>,
}

impl WindowState {
    /// Returns the last known outer-window position in logical screen coordinates.
    pub fn position(self) -> Option<Point> {
        self.position.get()
    }

    /// Returns the last known outer-window position without subscribing to changes.
    pub fn position_non_reactive(self) -> Option<Point> {
        self.position.get_non_reactive()
    }

    /// Updates the stored outer-window position.
    pub fn set_position(self, position: Option<Point>) {
        if self.position.get_non_reactive() != position {
            self.position.set(position);
        }
    }

    /// Moves the stored outer-window position by a logical delta when a position is known.
    pub fn translate(self, dx: f32, dy: f32) {
        if let Some(position) = self.position_non_reactive() {
            self.set_position(Some(Point::new(position.x + dx, position.y + dy)));
        }
    }

    /// Returns the current content size in logical pixels.
    pub fn size(self) -> Size {
        self.size.get()
    }

    /// Returns the current content size without subscribing to changes.
    pub fn size_non_reactive(self) -> Size {
        self.size.get_non_reactive()
    }

    /// Updates the stored content size in logical pixels.
    pub fn set_size(self, size: Size) {
        if self.size.get_non_reactive() != size {
            self.size.set(size);
        }
    }
}

/// Remembers native-window position and size across recompositions.
#[allow(non_snake_case)]
#[composable]
pub fn rememberWindowState(width: f32, height: f32) -> WindowState {
    WindowState {
        position: cranpose_core::useState(|| None::<Point>),
        size: cranpose_core::useState(move || Size::new(width, height)),
    }
}

/// Declarative configuration for an operating-system window.
///
/// Use this with [`Window`] to render a composable subtree into a separate OS
/// window on desktop. Platforms without native sub-window support render the
/// content inline, so pointer input and other composable behavior stay shared.
#[derive(Clone)]
pub struct WindowConfig {
    options: NativeWindowOptions,
    callbacks: NativeWindowEvents,
    state: Option<WindowState>,
}

impl WindowConfig {
    /// Creates a decorated, resizable window with a fixed initial content size.
    pub fn new(title: impl Into<String>, width: f32, height: f32) -> Self {
        Self {
            options: NativeWindowOptions::new(title, width, height),
            callbacks: NativeWindowEvents::new(),
            state: None,
        }
    }

    /// Creates a decorated, resizable window using a remembered state size.
    pub fn new_for_state(title: impl Into<String>, state: WindowState) -> Self {
        let size = state.size();
        Self::new(title, size.width, size.height).with_state(state)
    }

    /// Creates a borderless, non-resizable window with a fixed initial content size.
    pub fn borderless(title: impl Into<String>, width: f32, height: f32) -> Self {
        Self {
            options: NativeWindowOptions::borderless(title, width, height),
            callbacks: NativeWindowEvents::new(),
            state: None,
        }
    }

    /// Creates a borderless, non-resizable window using a remembered state size.
    pub fn borderless_for_state(title: impl Into<String>, state: WindowState) -> Self {
        let size = state.size();
        Self::borderless(title, size.width, size.height).with_state(state)
    }

    /// Sets the initial outer-window position in logical screen coordinates.
    pub fn with_position(mut self, x: f32, y: f32) -> Self {
        self.options = self.options.with_position(x, y);
        self
    }

    /// Sets the initial outer-window position relative to the host application window.
    pub fn with_host_window_position(mut self, x: f32, y: f32) -> Self {
        self.options = self.options.with_host_window_position(x, y);
        self
    }

    /// Sets whether compositor transparency should be requested.
    pub fn with_transparent(mut self, transparent: bool) -> Self {
        self.options = self.options.with_transparent(transparent);
        self
    }

    /// Sets whether the operating system should allow interactive resizing.
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.options = self.options.with_resizable(resizable);
        self
    }

    /// Sets whether the window should be visible when created.
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.options = self.options.with_visible(visible);
        self
    }

    /// Sets whether the window should be kept above normal windows.
    pub fn with_always_on_top(mut self, always_on_top: bool) -> Self {
        self.options = self.options.with_always_on_top(always_on_top);
        self
    }

    /// Sets the minimum content size in logical pixels.
    pub fn with_min_size(mut self, width: f32, height: f32) -> Self {
        self.options = self.options.with_min_size(width, height);
        self
    }

    /// Sets the maximum content size in logical pixels.
    pub fn with_max_size(mut self, width: f32, height: f32) -> Self {
        self.options = self.options.with_max_size(width, height);
        self
    }

    /// Called when the operating system reports an external outer-window move.
    ///
    /// Position changes requested through [`WindowState`] are acknowledged by the
    /// desktop host without re-entering this callback.
    pub fn on_moved(mut self, callback: impl Fn(f32, f32) + 'static) -> Self {
        self.callbacks = self.callbacks.with_on_moved(callback);
        self
    }

    /// Called when the operating system reports a new content size.
    pub fn on_resized(mut self, callback: impl Fn(f32, f32) + 'static) -> Self {
        self.callbacks = self.callbacks.with_on_resized(callback);
        self
    }

    /// Called when the operating system requests that this window close.
    pub fn on_close_requested(mut self, callback: impl Fn() + 'static) -> Self {
        self.callbacks = self.callbacks.with_on_close_requested(callback);
        self
    }

    /// Binds this configuration to a remembered [`WindowState`].
    ///
    /// The current state supplies the requested position and size. The desktop
    /// window host keeps the state in sync with the operating system unless an
    /// explicit callback updates it first.
    pub fn with_state(mut self, state: WindowState) -> Self {
        let size = state.size();
        self.options.width = size.width;
        self.options.height = size.height;
        if let Some(position) = state.position() {
            self.options.x = Some(position.x);
            self.options.y = Some(position.y);
            self.options.position_origin = NativeWindowPositionOrigin::Screen;
        }
        self.state = Some(state);
        self
    }

    pub(crate) fn into_parts(
        self,
    ) -> (NativeWindowOptions, NativeWindowEvents, Option<WindowState>) {
        (self.options, self.callbacks, self.state)
    }
}

/// Edge or corner used by [`WindowModifierExt::window_resize_area`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum WindowResizeDirection {
    /// Resize from the east edge.
    East,
    /// Resize from the north edge.
    North,
    /// Resize from the north-east corner.
    NorthEast,
    /// Resize from the north-west corner.
    NorthWest,
    /// Resize from the south edge.
    South,
    /// Resize from the south-east corner.
    SouthEast,
    /// Resize from the south-west corner.
    SouthWest,
    /// Resize from the west edge.
    West,
}

/// Modifier helpers for composables rendered in OS windows.
pub trait WindowModifierExt {
    /// Marks this component as a drag target for its containing OS window.
    ///
    /// The modifier is inert when the component is not currently rendered in a
    /// native desktop sub-window, so the same UI can be used inline.
    fn window_drag_area(self) -> Modifier;

    /// Marks this component as a resize target for its containing OS window.
    ///
    /// The modifier is inert when the component is not currently rendered in a
    /// native desktop sub-window.
    fn window_resize_area(self, direction: WindowResizeDirection) -> Modifier;
}

impl WindowModifierExt for Modifier {
    fn window_drag_area(self) -> Modifier {
        self.pointer_input((), move |scope: PointerInputScope| async move {
            scope
                .await_pointer_event_scope(|await_scope| async move {
                    loop {
                        let event = await_scope.await_pointer_event().await;
                        if event.kind == PointerEventKind::Down && request_native_window_drag() {
                            event.consume();
                        }
                    }
                })
                .await;
        })
    }

    fn window_resize_area(self, direction: WindowResizeDirection) -> Modifier {
        self.pointer_input(direction, move |scope: PointerInputScope| async move {
            scope
                .await_pointer_event_scope(|await_scope| async move {
                    loop {
                        let event = await_scope.await_pointer_event().await;
                        if event.kind == PointerEventKind::Down
                            && request_native_window_resize(direction)
                        {
                            event.consume();
                        }
                    }
                })
                .await;
        })
    }
}

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) type NativeWindowContent = Rc<RefCell<Box<dyn FnMut()>>>;

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
type NativeWindowOwner = Rc<()>;

type NativeWindowDragHandler = Rc<dyn Fn()>;
type NativeWindowResizeHandler = Rc<dyn Fn(WindowResizeDirection)>;

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone)]
pub(crate) struct NativeWindowRequest {
    pub(crate) key: NativeWindowKey,
    pub(crate) options: NativeWindowOptions,
    pub(crate) events: NativeWindowEvents,
    pub(crate) state: Option<WindowState>,
    pub(crate) content: NativeWindowContent,
    pub(crate) revision: u64,
    owner: NativeWindowOwner,
}

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
struct NativeWindowRevision {
    content: Weak<RefCell<Box<dyn FnMut()>>>,
    revision: u64,
}

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
thread_local! {
    static NATIVE_WINDOWS: RefCell<HashMap<NativeWindowKey, NativeWindowRequest>> =
        RefCell::new(HashMap::new());
    static NATIVE_WINDOW_REVISIONS: RefCell<HashMap<NativeWindowKey, NativeWindowRevision>> =
        RefCell::new(HashMap::new());
    static NEXT_NATIVE_WINDOW_REVISION: Cell<u64> = const { Cell::new(1) };
}

thread_local! {
    static CURRENT_NATIVE_WINDOW_DRAG: RefCell<Option<NativeWindowDragHandler>> = const { RefCell::new(None) };
    static CURRENT_NATIVE_WINDOW_RESIZE: RefCell<Option<NativeWindowResizeHandler>> = const { RefCell::new(None) };
    static CURRENT_NATIVE_WINDOW_SURFACE_ORIGIN: RefCell<Option<Point>> = const { RefCell::new(None) };
}

/// Renders content in an operating-system window owned by the current composition.
///
/// On desktop this creates or updates a separate OS window. Other platforms compose
/// the content inline so the same UI remains usable without native sub-window support.
#[allow(non_snake_case)]
#[composable(no_skip)]
pub fn Window(id: &'static str, config: WindowConfig, content: impl FnMut() + 'static) {
    let (options, events, state) = config.into_parts();
    NativeWindowWithEvents(id, options, events, state, content);
}

#[allow(non_snake_case)]
#[composable(no_skip)]
fn NativeWindowWithEvents(
    id: &'static str,
    options: NativeWindowOptions,
    events: NativeWindowEvents,
    state: Option<WindowState>,
    content: impl FnMut() + 'static,
) {
    #[cfg(all(
        feature = "desktop",
        feature = "renderer-wgpu",
        not(target_arch = "wasm32")
    ))]
    {
        let key = NativeWindowKey::from_static(id);
        let owner = cranpose_core::remember(|| Rc::new(())).with(Rc::clone);
        let content_cell =
            cranpose_core::remember(|| Rc::new(RefCell::new(Box::new(|| {}) as Box<dyn FnMut()>)))
                .with(Rc::clone);
        *content_cell.borrow_mut() = Box::new(content);

        {
            let options = options.clone();
            let events = events.clone();
            let content = Rc::clone(&content_cell);
            let owner = Rc::clone(&owner);
            cranpose_core::SideEffect(move || {
                register_native_window(key, options, events, state, content, owner);
            });
        }

        {
            let owner = Rc::clone(&owner);
            cranpose_core::DisposableEffect!(key, move |scope| {
                scope.on_dispose(move || unregister_native_window(key, owner))
            });
        }
    }

    #[cfg(not(all(
        feature = "desktop",
        feature = "renderer-wgpu",
        not(target_arch = "wasm32")
    )))]
    {
        let mut content = content;
        let _ = (id, options, events, state);
        content();
    }
}

/// Requests that the current native window begin an operating-system window drag.
///
/// This returns `true` when a desktop native window is currently dispatching the
/// pointer event and the request was forwarded to the platform window.
fn request_native_window_drag() -> bool {
    CURRENT_NATIVE_WINDOW_DRAG.with(|slot| {
        let handler = slot.borrow().clone();
        if let Some(handler) = handler {
            handler();
            true
        } else {
            false
        }
    })
}

fn request_native_window_resize(direction: WindowResizeDirection) -> bool {
    CURRENT_NATIVE_WINDOW_RESIZE.with(|slot| {
        let handler = slot.borrow().clone();
        if let Some(handler) = handler {
            handler(direction);
            true
        } else {
            false
        }
    })
}

/// Returns the desktop-space origin of the current native window surface while
/// dispatching input inside a native window.
///
/// This is `None` for inline content and outside native-window input dispatch.
pub fn current_native_window_surface_origin() -> Option<Point> {
    CURRENT_NATIVE_WINDOW_SURFACE_ORIGIN.with(|slot| *slot.borrow())
}

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) fn native_window_requests() -> Vec<NativeWindowRequest> {
    NATIVE_WINDOWS.with(|windows| windows.borrow().values().cloned().collect())
}

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) fn has_native_window_requests() -> bool {
    NATIVE_WINDOWS.with(|windows| !windows.borrow().is_empty())
}

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) fn clear_native_window_requests() {
    NATIVE_WINDOWS.with(|windows| windows.borrow_mut().clear());
}

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) fn with_native_window_drag_handler<R>(
    handler: NativeWindowDragHandler,
    resize_handler: NativeWindowResizeHandler,
    f: impl FnOnce() -> R,
) -> R {
    struct WindowHandlerGuard;

    impl Drop for WindowHandlerGuard {
        fn drop(&mut self) {
            CURRENT_NATIVE_WINDOW_DRAG.with(|slot| {
                *slot.borrow_mut() = None;
            });
            CURRENT_NATIVE_WINDOW_RESIZE.with(|slot| {
                *slot.borrow_mut() = None;
            });
        }
    }

    CURRENT_NATIVE_WINDOW_DRAG.with(|slot| {
        *slot.borrow_mut() = Some(handler);
    });
    CURRENT_NATIVE_WINDOW_RESIZE.with(|slot| {
        *slot.borrow_mut() = Some(resize_handler);
    });
    let _guard = WindowHandlerGuard;
    f()
}

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) fn with_native_window_surface_origin<R>(
    origin: Option<Point>,
    f: impl FnOnce() -> R,
) -> R {
    struct SurfaceOriginGuard(Option<Point>);

    impl Drop for SurfaceOriginGuard {
        fn drop(&mut self) {
            CURRENT_NATIVE_WINDOW_SURFACE_ORIGIN.with(|slot| {
                *slot.borrow_mut() = self.0;
            });
        }
    }

    let previous = CURRENT_NATIVE_WINDOW_SURFACE_ORIGIN.with(|slot| {
        let previous = *slot.borrow();
        *slot.borrow_mut() = origin;
        previous
    });
    let _guard = SurfaceOriginGuard(previous);
    f()
}

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn register_native_window(
    key: NativeWindowKey,
    options: NativeWindowOptions,
    events: NativeWindowEvents,
    state: Option<WindowState>,
    content: NativeWindowContent,
    owner: NativeWindowOwner,
) {
    let revision = native_window_content_revision(key, &content);
    NATIVE_WINDOWS.with(|windows| {
        windows.borrow_mut().insert(
            key,
            NativeWindowRequest {
                key,
                options,
                events,
                state,
                content,
                revision,
                owner,
            },
        );
    });
}

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn unregister_native_window(key: NativeWindowKey, owner: NativeWindowOwner) {
    NATIVE_WINDOWS.with(|windows| {
        let mut windows = windows.borrow_mut();
        if windows
            .get(&key)
            .is_some_and(|request| Rc::ptr_eq(&request.owner, &owner))
        {
            windows.remove(&key);
        }
    });
}

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn native_window_content_revision(key: NativeWindowKey, content: &NativeWindowContent) -> u64 {
    NATIVE_WINDOW_REVISIONS.with(|revisions| {
        let content = Rc::downgrade(content);
        let mut revisions = revisions.borrow_mut();
        if let Some(state) = revisions.get_mut(&key) {
            if state.content.ptr_eq(&content) {
                return state.revision;
            }
            state.content = content;
            state.revision = next_native_window_revision();
            return state.revision;
        }

        let revision = next_native_window_revision();
        revisions.insert(key, NativeWindowRevision { content, revision });
        revision
    })
}

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn next_native_window_revision() -> u64 {
    NEXT_NATIVE_WINDOW_REVISION.with(|revision| {
        let current = revision.get();
        revision.set(current.wrapping_add(1).max(1));
        current
    })
}

#[cfg(all(
    feature = "desktop",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn hash_id(id: &'static str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn test_window_state(
        width: f32,
        height: f32,
    ) -> (
        cranpose_core::Runtime,
        cranpose_core::OwnedMutableState<Option<Point>>,
        cranpose_core::OwnedMutableState<Size>,
        WindowState,
    ) {
        let runtime = cranpose_core::Runtime::new(Arc::new(cranpose_core::DefaultScheduler));
        let handle = runtime.handle();
        let position =
            cranpose_core::OwnedMutableState::with_runtime(None::<Point>, handle.clone());
        let size = cranpose_core::OwnedMutableState::with_runtime(Size::new(width, height), handle);
        let state = WindowState {
            position: position.handle(),
            size: size.handle(),
        };
        (runtime, position, size, state)
    }

    #[test]
    fn borderless_options_disable_decorations_and_resizing() {
        let options = NativeWindowOptions::borderless("Tool", 100.0, 50.0);
        assert_eq!(options.title, "Tool");
        assert_eq!(options.width, 100.0);
        assert_eq!(options.height, 50.0);
        assert_eq!(options.position_origin, NativeWindowPositionOrigin::Screen);
        assert!(!options.decorations);
        assert!(!options.resizable);
        assert!(options.visible);
    }

    #[test]
    fn option_builders_update_specific_fields() {
        let options = NativeWindowOptions::new("Panel", 10.0, 20.0)
            .with_position(3.0, 4.0)
            .with_transparent(true)
            .with_resizable(false)
            .with_visible(false)
            .with_always_on_top(true)
            .with_min_size(5.0, 6.0)
            .with_max_size(50.0, 60.0);
        assert_eq!(options.x, Some(3.0));
        assert_eq!(options.y, Some(4.0));
        assert_eq!(options.position_origin, NativeWindowPositionOrigin::Screen);
        assert!(options.transparent);
        assert!(!options.resizable);
        assert!(!options.visible);
        assert!(options.always_on_top);
        assert_eq!(options.min_width, Some(5.0));
        assert_eq!(options.min_height, Some(6.0));
        assert_eq!(options.max_width, Some(50.0));
        assert_eq!(options.max_height, Some(60.0));
    }

    #[test]
    fn host_window_position_records_origin() {
        let options =
            NativeWindowOptions::new("Panel", 10.0, 20.0).with_host_window_position(3.0, 4.0);
        assert_eq!(options.x, Some(3.0));
        assert_eq!(options.y, Some(4.0));
        assert_eq!(
            options.position_origin,
            NativeWindowPositionOrigin::HostWindow
        );
    }

    #[test]
    fn events_builder_registers_move_callback() {
        let events = NativeWindowEvents::new().with_on_moved(|_, _| {});
        assert!(events.on_moved.is_some());
    }

    #[test]
    fn window_state_accessors_update_position_and_size() {
        let (_runtime, _position_owner, _size_owner, state) = test_window_state(100.0, 50.0);

        assert_eq!(state.position_non_reactive(), None);
        assert_eq!(state.size_non_reactive(), Size::new(100.0, 50.0));

        state.set_position(Some(Point::new(4.0, 8.0)));
        assert_eq!(state.position_non_reactive(), Some(Point::new(4.0, 8.0)));

        state.translate(3.0, -2.0);
        assert_eq!(state.position_non_reactive(), Some(Point::new(7.0, 6.0)));

        state.set_size(Size::new(120.0, 64.0));
        assert_eq!(state.size_non_reactive(), Size::new(120.0, 64.0));
    }

    #[test]
    fn window_config_collects_window_settings_and_callbacks() {
        let config = WindowConfig::borderless("Panel", 100.0, 50.0)
            .with_host_window_position(7.0, 9.0)
            .with_transparent(true)
            .with_resizable(false)
            .with_visible(false)
            .with_always_on_top(true)
            .with_min_size(20.0, 10.0)
            .with_max_size(400.0, 200.0)
            .on_moved(|_, _| {})
            .on_resized(|_, _| {})
            .on_close_requested(|| {});

        let (options, callbacks, state) = config.into_parts();
        assert_eq!(options.title, "Panel");
        assert_eq!(options.width, 100.0);
        assert_eq!(options.height, 50.0);
        assert_eq!(options.x, Some(7.0));
        assert_eq!(options.y, Some(9.0));
        assert_eq!(
            options.position_origin,
            NativeWindowPositionOrigin::HostWindow
        );
        assert!(!options.decorations);
        assert!(options.transparent);
        assert!(!options.resizable);
        assert!(!options.visible);
        assert!(options.always_on_top);
        assert_eq!(options.min_width, Some(20.0));
        assert_eq!(options.min_height, Some(10.0));
        assert_eq!(options.max_width, Some(400.0));
        assert_eq!(options.max_height, Some(200.0));
        assert!(callbacks.on_moved.is_some());
        assert!(callbacks.on_resized.is_some());
        assert!(callbacks.on_close_requested.is_some());
        assert!(state.is_none());
    }

    #[test]
    fn state_window_configs_bind_size_position() {
        let (_runtime, _position_owner, _size_owner, state) = test_window_state(100.0, 50.0);
        state.set_position(Some(Point::new(7.0, 9.0)));

        let (options, callbacks, bound_state) =
            WindowConfig::borderless_for_state("Panel", state).into_parts();
        assert_eq!(options.title, "Panel");
        assert_eq!(options.width, 100.0);
        assert_eq!(options.height, 50.0);
        assert_eq!(options.x, Some(7.0));
        assert_eq!(options.y, Some(9.0));
        assert!(!options.decorations);
        assert!(!options.resizable);
        assert!(bound_state == Some(state));
        assert!(callbacks.on_moved.is_none());
        assert!(callbacks.on_resized.is_none());

        state.set_size(Size::new(320.0, 200.0));

        let (decorated_options, _, decorated_state) =
            WindowConfig::new_for_state("Decorated", state).into_parts();
        assert_eq!(decorated_options.width, 320.0);
        assert_eq!(decorated_options.height, 200.0);
        assert!(decorated_options.decorations);
        assert!(decorated_options.resizable);
        assert!(decorated_state == Some(state));
    }

    #[test]
    fn drag_request_reports_missing_handler() {
        assert!(!request_native_window_drag());
    }

    #[test]
    fn resize_request_reports_missing_handler() {
        assert!(!request_native_window_resize(
            WindowResizeDirection::SouthEast
        ));
    }

    #[cfg(all(
        feature = "desktop",
        feature = "renderer-wgpu",
        not(target_arch = "wasm32")
    ))]
    #[test]
    fn native_window_surface_origin_is_scoped_to_dispatch() {
        assert_eq!(current_native_window_surface_origin(), None);

        with_native_window_surface_origin(Some(Point::new(4.0, 8.0)), || {
            assert_eq!(
                current_native_window_surface_origin(),
                Some(Point::new(4.0, 8.0))
            );
            with_native_window_surface_origin(Some(Point::new(12.0, 16.0)), || {
                assert_eq!(
                    current_native_window_surface_origin(),
                    Some(Point::new(12.0, 16.0))
                );
            });
            assert_eq!(
                current_native_window_surface_origin(),
                Some(Point::new(4.0, 8.0))
            );
        });

        assert_eq!(current_native_window_surface_origin(), None);
    }

    #[test]
    fn static_keys_are_stable() {
        assert_eq!(
            NativeWindowKey::from_static("stable"),
            NativeWindowKey::from_static("stable")
        );
        assert_ne!(
            NativeWindowKey::from_static("stable"),
            NativeWindowKey::from_static("other")
        );
    }

    #[cfg(all(
        feature = "desktop",
        feature = "renderer-wgpu",
        not(target_arch = "wasm32")
    ))]
    fn test_owner() -> NativeWindowOwner {
        Rc::new(())
    }

    #[cfg(all(
        feature = "desktop",
        feature = "renderer-wgpu",
        not(target_arch = "wasm32")
    ))]
    #[test]
    fn registry_replaces_native_window_declarations_by_key() {
        clear_native_window_requests();

        let key = NativeWindowKey::from_static("visibility-update");
        let owner = test_owner();
        let content: NativeWindowContent =
            Rc::new(RefCell::new(Box::new(|| {}) as Box<dyn FnMut()>));

        register_native_window(
            key,
            NativeWindowOptions::new("Panel", 100.0, 50.0).with_visible(true),
            NativeWindowEvents::new(),
            None,
            Rc::clone(&content),
            Rc::clone(&owner),
        );
        let initial_revision = native_window_requests()
            .into_iter()
            .next()
            .expect("first native window request")
            .revision;

        register_native_window(
            key,
            NativeWindowOptions::new("Panel", 100.0, 50.0).with_visible(false),
            NativeWindowEvents::new(),
            None,
            content,
            owner,
        );

        let requests = native_window_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].key, key);
        assert_eq!(requests[0].revision, initial_revision);
        assert!(!requests[0].options.visible);

        clear_native_window_requests();
        assert!(native_window_requests().is_empty());
    }

    #[cfg(all(
        feature = "desktop",
        feature = "renderer-wgpu",
        not(target_arch = "wasm32")
    ))]
    #[test]
    fn registry_revisions_change_when_content_identity_changes() {
        clear_native_window_requests();

        let key = NativeWindowKey::from_static("content-update");
        let owner = test_owner();
        let first_content: NativeWindowContent =
            Rc::new(RefCell::new(Box::new(|| {}) as Box<dyn FnMut()>));
        let second_content: NativeWindowContent =
            Rc::new(RefCell::new(Box::new(|| {}) as Box<dyn FnMut()>));

        register_native_window(
            key,
            NativeWindowOptions::new("Panel", 100.0, 50.0),
            NativeWindowEvents::new(),
            None,
            first_content,
            Rc::clone(&owner),
        );
        let first_revision = native_window_requests()
            .into_iter()
            .next()
            .expect("first native window request")
            .revision;

        register_native_window(
            key,
            NativeWindowOptions::new("Panel", 100.0, 50.0),
            NativeWindowEvents::new(),
            None,
            second_content,
            owner,
        );
        let second_revision = native_window_requests()
            .into_iter()
            .next()
            .expect("second native window request")
            .revision;

        assert_ne!(first_revision, second_revision);

        clear_native_window_requests();
    }

    #[cfg(all(
        feature = "desktop",
        feature = "renderer-wgpu",
        not(target_arch = "wasm32")
    ))]
    #[test]
    fn unregister_ignores_stale_owner_after_redeclaration() {
        clear_native_window_requests();

        let key = NativeWindowKey::from_static("reattach-window");
        let stale_owner = test_owner();
        let current_owner = test_owner();
        let first_content: NativeWindowContent =
            Rc::new(RefCell::new(Box::new(|| {}) as Box<dyn FnMut()>));
        let second_content: NativeWindowContent =
            Rc::new(RefCell::new(Box::new(|| {}) as Box<dyn FnMut()>));

        register_native_window(
            key,
            NativeWindowOptions::new("Panel", 100.0, 50.0),
            NativeWindowEvents::new(),
            None,
            Rc::clone(&first_content),
            Rc::clone(&stale_owner),
        );

        register_native_window(
            key,
            NativeWindowOptions::new("Panel", 100.0, 50.0),
            NativeWindowEvents::new(),
            None,
            Rc::clone(&second_content),
            Rc::clone(&current_owner),
        );
        unregister_native_window(key, stale_owner);

        let requests = native_window_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].key, key);
        assert!(Rc::ptr_eq(&requests[0].content, &second_content));

        unregister_native_window(key, current_owner);
        assert!(native_window_requests().is_empty());

        clear_native_window_requests();
    }

    #[cfg(all(
        feature = "desktop",
        feature = "renderer-wgpu",
        not(target_arch = "wasm32")
    ))]
    #[test]
    fn clear_preserves_same_content_revision() {
        clear_native_window_requests();

        let key = NativeWindowKey::from_static("remove-window");
        let owner = test_owner();
        let content: NativeWindowContent =
            Rc::new(RefCell::new(Box::new(|| {}) as Box<dyn FnMut()>));

        register_native_window(
            key,
            NativeWindowOptions::new("Panel", 100.0, 50.0),
            NativeWindowEvents::new(),
            None,
            Rc::clone(&content),
            Rc::clone(&owner),
        );
        let first_revision = native_window_requests()
            .into_iter()
            .next()
            .expect("registered native window request")
            .revision;

        clear_native_window_requests();
        assert!(native_window_requests().is_empty());

        register_native_window(
            key,
            NativeWindowOptions::new("Panel", 100.0, 50.0),
            NativeWindowEvents::new(),
            None,
            content,
            owner,
        );
        let second_revision = native_window_requests()
            .into_iter()
            .next()
            .expect("registered native window request after cleanup")
            .revision;

        assert_eq!(first_revision, second_revision);

        clear_native_window_requests();
    }
}
