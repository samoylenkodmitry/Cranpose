#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
use std::cell::Cell;
#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
use std::collections::HashMap;
#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
use std::hash::Hash;
use std::{cell::RefCell, fmt, rc::Rc};

use cranpose_core::MutableState;
use cranpose_ui::{Modifier, Point, PointerEventKind, PointerInputScope, Size, composable};

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct WindowId(u64);

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
impl WindowId {
    #[cfg(test)]
    pub(crate) fn from_static(id: &'static str) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        id.hash(&mut hasher);
        Self(hasher.finish())
    }

    pub(crate) fn raw(self) -> u64 {
        self.0
    }

    pub(crate) fn from_node(node: cranpose_core::NodeId) -> Self {
        Self(node as u64)
    }
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) type NativeWindowKey = WindowId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeWindowPositionOrigin {
    Screen,
    HostWindow,
}

/// Whether a window takes keyboard focus when it is created.
///
/// macOS reads a window's cursor rectangles only while it is key, so a window
/// that never takes focus shows the system arrow whatever the application
/// sets. The policy applies when the desktop creates the window; a window
/// shown later comes up as the platform shows it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WindowFocus {
    /// The window never takes focus when it is created.
    Never,
    /// The window takes focus only when no window of the application holds
    /// it, so a window the user is working in keeps it.
    #[default]
    WhenNoneFocused,
    /// The window takes focus whenever it is created.
    Always,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NativeWindowOptions {
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub position_origin: NativeWindowPositionOrigin,
    pub decorations: bool,
    pub transparent: bool,
    pub shadow: bool,
    pub resizable: bool,
    pub visible: bool,
    pub always_on_top: bool,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub max_width: Option<f32>,
    pub max_height: Option<f32>,
    /// Whether the window takes focus when it is created.
    pub focus: WindowFocus,
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone)]
pub(crate) struct NativeWindowParts {
    pub(crate) options: NativeWindowOptions,
    pub(crate) events: NativeWindowEvents,
    pub(crate) state: Option<WindowState>,
}

impl NativeWindowOptions {
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
            shadow: true,
            resizable: true,
            visible: true,
            always_on_top: false,
            min_width: None,
            min_height: None,
            max_width: None,
            max_height: None,
            focus: WindowFocus::default(),
        }
    }

    pub fn borderless(title: impl Into<String>, width: f32, height: f32) -> Self {
        Self {
            decorations: false,
            resizable: false,
            ..Self::new(title, width, height)
        }
    }

    pub fn with_position(mut self, x: f32, y: f32) -> Self {
        self.x = Some(x);
        self.y = Some(y);
        self.position_origin = NativeWindowPositionOrigin::Screen;
        self
    }

    pub fn with_host_window_position(mut self, x: f32, y: f32) -> Self {
        self.x = Some(x);
        self.y = Some(y);
        self.position_origin = NativeWindowPositionOrigin::HostWindow;
        self
    }

    pub fn with_transparent(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
        self
    }

    /// Whether the desktop draws its own drop shadow behind the window.
    ///
    /// The desktop takes the shadow's shape from the window's alpha, which
    /// suits a window whose edges are hard. A window that fades out, a glow
    /// or a blur, gets a contour drawn where the desktop's threshold falls,
    /// and turns the shadow off to draw its own.
    pub fn with_shadow(mut self, shadow: bool) -> Self {
        self.shadow = shadow;
        self
    }

    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn with_always_on_top(mut self, always_on_top: bool) -> Self {
        self.always_on_top = always_on_top;
        self
    }

    /// Sets whether the window takes focus when it is created.
    pub fn with_focus(mut self, focus: WindowFocus) -> Self {
        self.focus = focus;
        self
    }

    pub fn with_min_size(mut self, width: f32, height: f32) -> Self {
        self.min_width = Some(width);
        self.min_height = Some(height);
        self
    }

    pub fn with_max_size(mut self, width: f32, height: f32) -> Self {
        self.max_width = Some(width);
        self.max_height = Some(height);
        self
    }
}

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

/// Mutable position and size state for a declarative OS window, and whether
/// the window has a frame on the screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowState {
    position: MutableState<Option<Point>>,
    size: MutableState<Size>,
    frame: MutableState<Size>,
    presented: MutableState<bool>,
}

impl WindowState {
    /// Returns the size of the whole window, title bar and borders included,
    /// in logical pixels.
    ///
    /// [`WindowState::size`] is the content, which is what a layout is given.
    /// This is what the window occupies on screen, which is what lines up
    /// against another window's edge. They are the same for a borderless
    /// window. Zero until the window is on screen.
    pub fn frame_size(self) -> Size {
        self.frame.get()
    }

    /// The size of the whole window, without subscribing to changes.
    pub fn frame_size_non_reactive(self) -> Size {
        self.frame.get_non_reactive()
    }

    /// Records the size of the whole window. The desktop sets it from the
    /// window itself; an application reads it.
    pub fn set_frame_size(self, frame: Size) {
        if self.frame.get_non_reactive() != frame {
            self.frame.set(frame);
        }
    }

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

    /// Whether the window has presented a frame since it was last shown.
    ///
    /// A window comes up a few frames after it is declared, and the desktop
    /// cannot present to it before it is on the screen. Content that moves
    /// from one window into a new one can wait for this before it leaves the
    /// old window, so the user never sees it in neither.
    pub fn presented(self) -> bool {
        self.presented.get()
    }

    /// Whether the window has presented a frame, without subscribing to changes.
    pub fn presented_non_reactive(self) -> bool {
        self.presented.get_non_reactive()
    }

    /// Records whether the window has a frame on the screen. The desktop sets
    /// it after a present and clears it when the window is hidden or let go,
    /// which can be after the composition that remembered the state is gone;
    /// a state that is gone is left alone.
    pub fn set_presented(self, presented: bool) {
        if self.presented.is_alive() && self.presented.get_non_reactive() != presented {
            self.presented.set(presented);
        }
    }

    /// A window's states, for code that remembers the window itself.
    ///
    /// Call it inside a `remember`, either through [`rememberWindowState`] or
    /// as one field of a larger holder that is remembered whole; the states
    /// then belong to that slot. Called straight from a composable body it
    /// makes a new window state every pass, as in Kotlin.
    pub fn new(width: f32, height: f32) -> Self {
        Self::sized(None, Size::new(width, height))
    }

    /// Like [`WindowState::new`], with the window first placed at the given
    /// screen position.
    pub fn placed_at(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self::sized(Some(Point::new(x, y)), Size::new(width, height))
    }

    fn sized(position: Option<Point>, size: Size) -> Self {
        WindowState {
            position: cranpose_core::mutableStateOf(position),
            size: cranpose_core::mutableStateOf(size),
            frame: cranpose_core::mutableStateOf(size),
            presented: cranpose_core::mutableStateOf(false),
        }
    }
}

/// Remembers native-window position and size across recompositions.
#[composable]
#[track_caller]
pub fn rememberWindowState(width: f32, height: f32) -> WindowState {
    cranpose_core::remember(move || WindowState::new(width, height)).with(|state| *state)
}

/// Remembers native-window position and size across recompositions, with the
/// window first placed at the given screen position.
#[composable]
#[track_caller]
pub fn rememberWindowStateAt(x: f32, y: f32, width: f32, height: f32) -> WindowState {
    cranpose_core::remember(move || WindowState::placed_at(x, y, width, height))
        .with(|state| *state)
}

/// Declarative configuration for an operating-system window.
///
/// Apply it with [`WindowModifierExt::window`] to render a composable subtree
/// in a separate OS window on desktop. Platforms without native sub-window
/// support render the content inline, so pointer input and other composable
/// behavior stay shared. Two configurations compare equal when they ask the
/// same of the window; their callbacks are not compared.
#[derive(Clone)]
pub struct WindowConfig {
    options: NativeWindowOptions,
    callbacks: NativeWindowEvents,
    state: Option<WindowState>,
}

impl PartialEq for WindowConfig {
    fn eq(&self, other: &Self) -> bool {
        self.options == other.options && self.state == other.state
    }
}

impl fmt::Debug for WindowConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowConfig")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
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

    /// Sets whether the desktop draws its own drop shadow behind the window.
    ///
    /// The desktop takes the shadow's shape from the window's alpha, which
    /// suits a window whose edges are hard. A window that fades out, a glow
    /// or a blur, gets a contour drawn where the desktop's threshold falls,
    /// and turns the shadow off to draw its own.
    pub fn with_shadow(mut self, shadow: bool) -> Self {
        self.options = self.options.with_shadow(shadow);
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

    /// Sets whether the window takes focus when it is created. See
    /// [`WindowFocus`].
    pub fn with_focus(mut self, focus: WindowFocus) -> Self {
        self.options = self.options.with_focus(focus);
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

    pub(crate) fn state(&self) -> Option<WindowState> {
        self.state
    }

    #[cfg(all(
        feature = "native-windows",
        feature = "renderer-wgpu",
        not(target_arch = "wasm32")
    ))]
    pub(crate) fn title(&self) -> &str {
        &self.options.title
    }

    #[cfg(all(
        feature = "native-windows",
        feature = "renderer-wgpu",
        not(target_arch = "wasm32")
    ))]
    pub(crate) fn into_parts(self) -> NativeWindowParts {
        NativeWindowParts {
            options: self.options,
            events: self.callbacks,
            state: self.state,
        }
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
    /// Renders this node's subtree in an operating-system window of its own
    /// while the modifier is applied, and inline again when it is not. The
    /// node keeps its identity, its remembered state and its running effects
    /// either way, so an application tears a subtree out by choosing the
    /// modifier from state. Platforms without native sub-windows leave the
    /// subtree inline.
    fn window(self, config: WindowConfig) -> Modifier;

    /// Marks this component as a drag target for its containing OS window,
    /// and reports when a drag the platform accepted starts and finishes.
    ///
    /// The modifier is inert when the component is not currently rendered in a
    /// native desktop sub-window, so the same UI can be used inline.
    /// The callbacks run only when a drag is actually accepted; pass
    /// `|| {}` for either one there is nothing to do about.
    ///
    /// The press is left unconsumed, so whatever sits under the drag area
    /// still sees it: a title bar moves its window and answers a click, and
    /// the click is the one `Modifier::clickable` recognises, on every
    /// platform.
    fn window_drag_area(
        self,
        on_started: impl Fn() + 'static,
        on_finished: impl Fn() + 'static,
    ) -> Modifier;

    /// Marks this component as a resize target for its containing OS window.
    ///
    /// The modifier is inert when the component is not currently rendered in a
    /// native desktop sub-window.
    fn window_resize_area(self, direction: WindowResizeDirection) -> Modifier;
}

impl WindowModifierExt for Modifier {
    fn window(self, config: WindowConfig) -> Modifier {
        let modifier = crate::window_local::with_window_state_local(self, config.state());

        #[cfg(all(
            feature = "native-windows",
            feature = "renderer-wgpu",
            not(target_arch = "wasm32")
        ))]
        {
            crate::window_node::window(modifier, config)
        }

        #[cfg(not(all(
            feature = "native-windows",
            feature = "renderer-wgpu",
            not(target_arch = "wasm32")
        )))]
        {
            let _ = config;
            modifier
        }
    }

    fn window_drag_area(
        self,
        on_started: impl Fn() + 'static,
        on_finished: impl Fn() + 'static,
    ) -> Modifier {
        let on_started: Rc<dyn Fn()> = Rc::new(on_started);
        let on_finished: Rc<dyn Fn()> = Rc::new(on_finished);
        self.pointer_input((), move |scope: PointerInputScope| {
            let on_started = on_started.clone();
            let on_finished = on_finished.clone();
            async move {
                scope
                    .await_pointer_event_scope(|await_scope| async move {
                        let mut dragging = false;
                        loop {
                            let event = await_scope.await_pointer_event().await;
                            match event.kind {
                                PointerEventKind::Down => {
                                    if request_native_window_drag() {
                                        dragging = true;
                                        on_started();
                                    }
                                }
                                PointerEventKind::Move => {
                                    if dragging && event.buttons == Default::default() {
                                        dragging = false;
                                        on_finished();
                                    }
                                }
                                PointerEventKind::Up | PointerEventKind::Cancel => {
                                    if dragging {
                                        dragging = false;
                                        on_finished();
                                    }
                                }
                                PointerEventKind::Scroll
                                | PointerEventKind::Zoom
                                | PointerEventKind::RotaryScrollPre
                                | PointerEventKind::RotaryScroll
                                | PointerEventKind::Enter
                                | PointerEventKind::Exit => {}
                            }
                        }
                    })
                    .await;
            }
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
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) struct NativeWindowRoot {
    size: Cell<Size>,
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
impl NativeWindowRoot {
    pub(crate) fn new(size: Size) -> Self {
        Self {
            size: Cell::new(size),
        }
    }

    pub(crate) fn set_size(&self, size: Size) {
        self.size.set(size);
    }
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
impl cranpose_ui::WindowRootDescriptor for NativeWindowRoot {
    fn layout_size(&self) -> Size {
        self.size.get()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) type NativeWindowRootHandle = Rc<NativeWindowRoot>;

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) type NativeWindowOwner = Rc<()>;

type NativeWindowDragHandler = Rc<dyn Fn() -> bool>;
type NativeWindowResizeHandler = Rc<dyn Fn(WindowResizeDirection)>;

#[derive(Clone, Default)]
struct NativeWindowDispatchContext {
    drag_handler: Option<NativeWindowDragHandler>,
    resize_handler: Option<NativeWindowResizeHandler>,
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone)]
pub(crate) struct NativeWindowRequest {
    pub(crate) key: NativeWindowKey,
    pub(crate) options: NativeWindowOptions,
    pub(crate) events: NativeWindowEvents,
    pub(crate) state: Option<WindowState>,
    pub(crate) root: NativeWindowRootHandle,
    pub(crate) revision: u64,
    owner: NativeWindowOwner,
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
struct NativeWindowRegistration {
    key: NativeWindowKey,
    options: NativeWindowOptions,
    events: NativeWindowEvents,
    state: Option<WindowState>,
    root: NativeWindowRootHandle,
    owner: NativeWindowOwner,
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Default)]
pub(crate) struct NativeWindowRegistry {
    windows: RefCell<HashMap<NativeWindowKey, NativeWindowRequest>>,
    next_revision: Cell<u64>,
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
impl NativeWindowRegistry {
    fn requests(&self) -> Vec<NativeWindowRequest> {
        self.windows.borrow().values().cloned().collect()
    }

    fn has_requests(&self) -> bool {
        !self.windows.borrow().is_empty()
    }

    #[cfg(test)]
    fn clear(&self) {
        self.windows.borrow_mut().clear();
    }

    fn register(&self, registration: NativeWindowRegistration) {
        let revision = self.next_revision();
        let key = registration.key;
        self.windows.borrow_mut().insert(
            key,
            NativeWindowRequest {
                key,
                options: registration.options,
                events: registration.events,
                state: registration.state,
                root: registration.root,
                revision,
                owner: registration.owner,
            },
        );
    }

    fn unregister(&self, key: NativeWindowKey, owner: NativeWindowOwner) {
        let mut windows = self.windows.borrow_mut();
        if windows
            .get(&key)
            .is_some_and(|request| Rc::ptr_eq(&request.owner, &owner))
        {
            windows.remove(&key);
        }
    }

    fn next_revision(&self) -> u64 {
        let current = self.next_revision.get().max(1);
        self.next_revision.set(current.wrapping_add(1).max(1));
        current
    }
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
thread_local! {
    static CURRENT_NATIVE_WINDOW_REGISTRY: crate::scoped_weak_stack::ScopedWeakStack<NativeWindowRegistry> =
        const { crate::scoped_weak_stack::ScopedWeakStack::new() };
}

thread_local! {
    static CURRENT_NATIVE_WINDOW_DISPATCH: RefCell<Vec<NativeWindowDispatchContext>> = const { RefCell::new(Vec::new()) };
}

fn request_native_window_drag() -> bool {
    current_native_window_dispatch_context()
        .and_then(|context| context.drag_handler)
        .is_some_and(|handler| handler())
}

fn request_native_window_resize(direction: WindowResizeDirection) -> bool {
    current_native_window_dispatch_context()
        .and_then(|context| context.resize_handler)
        .is_some_and(|handler| {
            handler(direction);
            true
        })
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) fn with_native_window_registry<R>(
    registry: &Rc<NativeWindowRegistry>,
    f: impl FnOnce() -> R,
) -> R {
    CURRENT_NATIVE_WINDOW_REGISTRY.with(|stack| stack.with_scope(registry, f))
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn current_native_window_registry() -> Option<Rc<NativeWindowRegistry>> {
    CURRENT_NATIVE_WINDOW_REGISTRY.with(crate::scoped_weak_stack::ScopedWeakStack::current)
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) fn native_window_requests(registry: &NativeWindowRegistry) -> Vec<NativeWindowRequest> {
    registry.requests()
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) fn has_native_window_requests(registry: &NativeWindowRegistry) -> bool {
    registry.has_requests()
}

#[cfg(all(
    test,
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) fn clear_native_window_requests(registry: &NativeWindowRegistry) {
    registry.clear();
}

fn current_native_window_dispatch_context() -> Option<NativeWindowDispatchContext> {
    CURRENT_NATIVE_WINDOW_DISPATCH.with(|stack| stack.borrow().last().cloned())
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn with_native_window_dispatch_context<R>(
    context: NativeWindowDispatchContext,
    f: impl FnOnce() -> R,
) -> R {
    struct DispatchContextGuard;

    impl Drop for DispatchContextGuard {
        fn drop(&mut self) {
            CURRENT_NATIVE_WINDOW_DISPATCH.with(|stack| {
                stack.borrow_mut().pop();
            });
        }
    }

    CURRENT_NATIVE_WINDOW_DISPATCH.with(|stack| {
        stack.borrow_mut().push(context);
    });
    let _guard = DispatchContextGuard;
    f()
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) fn with_native_window_drag_handler<R>(
    handler: NativeWindowDragHandler,
    resize_handler: NativeWindowResizeHandler,
    f: impl FnOnce() -> R,
) -> R {
    let mut context = current_native_window_dispatch_context().unwrap_or_default();
    context.drag_handler = Some(handler);
    context.resize_handler = Some(resize_handler);
    with_native_window_dispatch_context(context, f)
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) fn register_native_window(
    key: NativeWindowKey,
    options: NativeWindowOptions,
    events: NativeWindowEvents,
    state: Option<WindowState>,
    root: NativeWindowRootHandle,
    owner: NativeWindowOwner,
) {
    let Some(registry) = current_native_window_registry() else {
        log::error!(
            "native window declaration {key:?} ignored because no native-window registry is active"
        );
        return;
    };
    registry.register(NativeWindowRegistration {
        key,
        options,
        events,
        state,
        root,
        owner,
    });
}

#[cfg(all(
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) fn unregister_native_window(key: NativeWindowKey, owner: NativeWindowOwner) {
    let Some(registry) = current_native_window_registry() else {
        log::error!(
            "native window declaration {key:?} could not unregister because no native-window registry is active"
        );
        return;
    };
    registry.unregister(key, owner);
}

#[cfg(test)]
#[path = "tests/native_window_tests.rs"]
mod tests;

#[cfg(all(
    test,
    feature = "native-windows",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[path = "tests/native_window_desktop_tests.rs"]
mod desktop_tests;
