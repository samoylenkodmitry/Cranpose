#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
use std::cell::Cell;
#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
use std::collections::HashMap;
#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
use std::hash::{Hash, Hasher};
use std::{cell::RefCell, fmt, rc::Rc};

use cranpose_core::MutableState;
use cranpose_ui::{Modifier, Point, PointerEventKind, PointerInputScope, Size, composable};

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct WindowId(u64);

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
impl WindowId {
    #[cfg(test)]
    pub(crate) fn from_static(id: &'static str) -> Self {
        Self(hash_id(id))
    }

    pub(crate) fn raw(self) -> u64 {
        self.0
    }

    pub(crate) fn from_node(node: cranpose_core::NodeId) -> Self {
        Self(node as u64)
    }
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) type NativeWindowKey = WindowId;

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct WindowGroupId(u64);

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
impl WindowGroupId {
    pub(crate) fn from_static(id: &'static str) -> Self {
        Self(hash_id(id))
    }
}

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

/// Movement behavior for a group of attached peer windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMoveMode {
    /// Dragging any window in the group moves its attached component.
    AllAttached,
    /// Only windows configured with [`WindowConfig::leads_group`] move their
    /// attached component; other windows move alone.
    LeadersOnly,
}

impl WindowMoveMode {
    #[cfg(all(
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        not(target_arch = "wasm32")
    ))]
    fn moves_attached_component(self, leads_group: bool) -> bool {
        match self {
            Self::AllAttached => true,
            Self::LeadersOnly => leads_group,
        }
    }
}

/// Attachment and snapping policy for a declarative peer-window group.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowAttachPolicy {
    /// Maximum edge distance, in logical pixels, that counts as a snap target.
    pub snap_distance: f32,
    /// Maximum edge distance, in logical pixels, that counts as attached.
    pub attach_epsilon: f32,
    /// Determines which dragged windows move attached neighbors.
    pub move_mode: WindowMoveMode,
}

impl WindowAttachPolicy {
    /// Creates a peer-window attachment policy.
    pub fn new(snap_distance: f32, attach_epsilon: f32, move_mode: WindowMoveMode) -> Self {
        Self {
            snap_distance,
            attach_epsilon,
            move_mode,
        }
    }
}

impl Default for WindowAttachPolicy {
    fn default() -> Self {
        Self {
            snap_distance: 8.0,
            attach_epsilon: 3.0,
            move_mode: WindowMoveMode::AllAttached,
        }
    }
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeWindowGroupMembership {
    pub(crate) id: WindowGroupId,
    pub(crate) policy: WindowAttachPolicy,
    pub(crate) leads: bool,
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone)]
pub(crate) struct NativeWindowParts {
    pub(crate) options: NativeWindowOptions,
    pub(crate) events: NativeWindowEvents,
    pub(crate) state: Option<WindowState>,
    pub(crate) group: Option<NativeWindowGroupMembership>,
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
    presented: MutableState<bool>,
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
}

/// Remembers native-window position and size across recompositions.
#[allow(non_snake_case)]
#[composable]
#[track_caller]
pub fn rememberWindowState(width: f32, height: f32) -> WindowState {
    WindowState {
        position: cranpose_core::rememberMutableStateOf(|| None::<Point>),
        size: cranpose_core::rememberMutableStateOf(move || Size::new(width, height)),
        presented: cranpose_core::rememberMutableStateOf(|| false),
    }
}

/// Remembers native-window position and size across recompositions, with the
/// window first placed at the given screen position.
#[allow(non_snake_case)]
#[composable]
#[track_caller]
pub fn rememberWindowStateAt(x: f32, y: f32, width: f32, height: f32) -> WindowState {
    WindowState {
        position: cranpose_core::rememberMutableStateOf(move || Some(Point::new(x, y))),
        size: cranpose_core::rememberMutableStateOf(move || Size::new(width, height)),
        presented: cranpose_core::rememberMutableStateOf(|| false),
    }
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
    group: Option<WindowGroupConfig>,
    leads_group: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct WindowGroupConfig {
    id: &'static str,
    policy: WindowAttachPolicy,
}

impl PartialEq for WindowConfig {
    fn eq(&self, other: &Self) -> bool {
        self.options == other.options
            && self.state == other.state
            && self.group == other.group
            && self.leads_group == other.leads_group
    }
}

impl fmt::Debug for WindowConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowConfig")
            .field("options", &self.options)
            .field("group", &self.group)
            .field("leads_group", &self.leads_group)
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
            group: None,
            leads_group: false,
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
            group: None,
            leads_group: false,
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

    /// Joins the window to the group named `id`. Windows of one group snap
    /// to each other's edges and, under `policy`, move together.
    pub fn group(mut self, id: &'static str, policy: WindowAttachPolicy) -> Self {
        self.group = Some(WindowGroupConfig { id, policy });
        self
    }

    /// Whether dragging this window carries the windows attached to it when
    /// its group moves under [`WindowMoveMode::LeadersOnly`].
    pub fn leads_group(mut self, leads: bool) -> Self {
        self.leads_group = leads;
        self
    }

    pub(crate) fn state(&self) -> Option<WindowState> {
        self.state
    }

    #[cfg(all(
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        not(target_arch = "wasm32")
    ))]
    pub(crate) fn title(&self) -> &str {
        &self.options.title
    }

    #[cfg(all(
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        not(target_arch = "wasm32")
    ))]
    pub(crate) fn into_parts(self) -> NativeWindowParts {
        NativeWindowParts {
            options: self.options,
            events: self.callbacks,
            state: self.state,
            group: self.group.map(|group| NativeWindowGroupMembership {
                id: WindowGroupId::from_static(group.id),
                policy: group.policy,
                leads: self.leads_group,
            }),
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

    /// Marks this component as a drag target for its containing OS window.
    ///
    /// The modifier is inert when the component is not currently rendered in a
    /// native desktop sub-window, so the same UI can be used inline.
    fn window_drag_area(self) -> Modifier;

    /// Marks this component as a drag target and reports the native drag lifecycle.
    ///
    /// The callbacks run only when an OS-window drag is actually accepted by
    /// the current native desktop sub-window.
    fn window_drag_area_with_callbacks(
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
            feature = "desktop-shell",
            feature = "renderer-wgpu",
            not(target_arch = "wasm32")
        ))]
        {
            crate::window_node::window(modifier, config)
        }

        #[cfg(not(all(
            feature = "desktop-shell",
            feature = "renderer-wgpu",
            not(target_arch = "wasm32")
        )))]
        {
            let _ = config;
            modifier
        }
    }

    fn window_drag_area(self) -> Modifier {
        self.window_drag_area_with_callbacks(|| {}, || {})
    }

    fn window_drag_area_with_callbacks(
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
                                        event.consume();
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
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) struct NativeWindowRoot {
    size: Cell<Size>,
}

#[cfg(all(
    feature = "desktop-shell",
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
    feature = "desktop-shell",
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
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) type NativeWindowRootHandle = Rc<NativeWindowRoot>;

#[cfg(all(
    feature = "desktop-shell",
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
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone)]
pub(crate) struct NativeWindowRequest {
    pub(crate) key: NativeWindowKey,
    pub(crate) options: NativeWindowOptions,
    pub(crate) events: NativeWindowEvents,
    pub(crate) state: Option<WindowState>,
    pub(crate) group: Option<NativeWindowGroupMembership>,
    pub(crate) root: NativeWindowRootHandle,
    pub(crate) revision: u64,
    owner: NativeWindowOwner,
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
struct NativeWindowRegistration {
    key: NativeWindowKey,
    options: NativeWindowOptions,
    events: NativeWindowEvents,
    state: Option<WindowState>,
    group: Option<NativeWindowGroupMembership>,
    root: NativeWindowRootHandle,
    owner: NativeWindowOwner,
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Default)]
pub(crate) struct NativeWindowRegistry {
    windows: RefCell<HashMap<NativeWindowKey, NativeWindowRequest>>,
    next_revision: Cell<u64>,
}

#[cfg(all(
    feature = "desktop-shell",
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
                group: registration.group,
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
    feature = "desktop-shell",
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
    feature = "desktop-shell",
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
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn current_native_window_registry() -> Option<Rc<NativeWindowRegistry>> {
    CURRENT_NATIVE_WINDOW_REGISTRY.with(crate::scoped_weak_stack::ScopedWeakStack::current)
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) fn native_window_requests(registry: &NativeWindowRegistry) -> Vec<NativeWindowRequest> {
    registry.requests()
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) fn has_native_window_requests(registry: &NativeWindowRegistry) -> bool {
    registry.has_requests()
}

#[cfg(all(
    test,
    feature = "desktop-shell",
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
    feature = "desktop-shell",
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
    feature = "desktop-shell",
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
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
pub(crate) fn register_native_window(
    key: NativeWindowKey,
    options: NativeWindowOptions,
    events: NativeWindowEvents,
    state: Option<WindowState>,
    group: Option<NativeWindowGroupMembership>,
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
        group,
        root,
        owner,
    });
}

#[cfg(all(
    feature = "desktop-shell",
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

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn hash_id(id: &'static str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowGraphNodeSnapshot {
    pub(crate) id: WindowId,
    pub(crate) position: Point,
    pub(crate) size: Size,
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WindowGraphPeerSnapshot {
    pub(crate) node: WindowGraphNodeSnapshot,
    pub(crate) group: Option<NativeWindowGroupMembership>,
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowGraphMove {
    pub(crate) id: WindowId,
    pub(crate) position: Point,
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone, Debug)]
struct WindowGraphDragSession {
    group: Option<NativeWindowGroupMembership>,
    dragged: WindowId,
    start_dragged_position: Point,
    captured: Vec<WindowGraphNodeSnapshot>,
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Default)]
pub(crate) struct WindowGraphState {
    active_drag: Option<WindowGraphDragSession>,
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
impl WindowGraphState {
    pub(crate) fn start_drag(&mut self, windows: &[WindowGraphPeerSnapshot], dragged: WindowId) {
        let Some(dragged_window) = windows.iter().find(|window| window.node.id == dragged) else {
            self.active_drag = None;
            return;
        };
        let group = dragged_window.group.clone();
        let captured = if let Some(group) = &group {
            let group_windows = group_windows(windows, group);
            let moves_attached = group.policy.move_mode.moves_attached_component(group.leads);
            let component = if moves_attached {
                attached_component(&group_windows, dragged, group.policy.attach_epsilon)
            } else {
                vec![dragged]
            };
            group_windows
                .into_iter()
                .filter(|window| component.contains(&window.id))
                .collect()
        } else {
            vec![dragged_window.node]
        };

        self.active_drag = Some(WindowGraphDragSession {
            group,
            dragged,
            start_dragged_position: dragged_window.node.position,
            captured,
        });
    }

    pub(crate) fn drag_carries_peers(&self) -> bool {
        self.active_drag
            .as_ref()
            .is_some_and(|session| session.captured.len() > 1)
    }

    pub(crate) fn drag_to(
        &self,
        dragged: WindowId,
        target_position: Point,
    ) -> Vec<WindowGraphMove> {
        let Some(session) = &self.active_drag else {
            return vec![WindowGraphMove {
                id: dragged,
                position: target_position,
            }];
        };
        if session.dragged != dragged {
            return Vec::new();
        }

        let delta = Point::new(
            target_position.x - session.start_dragged_position.x,
            target_position.y - session.start_dragged_position.y,
        );
        session
            .captured
            .iter()
            .map(|window| WindowGraphMove {
                id: window.id,
                position: Point::new(window.position.x + delta.x, window.position.y + delta.y),
            })
            .collect()
    }

    pub(crate) fn cancel_drag(&mut self) {
        self.active_drag = None;
    }

    pub(crate) fn finish_drag(
        &mut self,
        windows: &[WindowGraphPeerSnapshot],
    ) -> Vec<WindowGraphMove> {
        let Some(session) = self.active_drag.take() else {
            return Vec::new();
        };
        let Some(group) = &session.group else {
            return Vec::new();
        };
        let group_windows = group_windows(windows, group);
        if group_windows
            .iter()
            .all(|window| window.id != session.dragged)
        {
            return Vec::new();
        }

        let moves_attached = group.policy.move_mode.moves_attached_component(group.leads);
        let mut component = if moves_attached {
            session.captured.iter().map(|window| window.id).collect()
        } else {
            vec![session.dragged]
        };
        if let Some(snap) = closest_snap(&group_windows, &component, group.policy.snap_distance) {
            let mut moved = group_windows;
            translate_nodes(&mut moved, &component, snap.delta);
            if moves_attached {
                for id in attached_component(&moved, snap.target, group.policy.attach_epsilon) {
                    if !component.contains(&id) {
                        component.push(id);
                    }
                }
            }
            return moved
                .into_iter()
                .filter(|window| component.contains(&window.id))
                .map(|window| WindowGraphMove {
                    id: window.id,
                    position: window.position,
                })
                .collect();
        }

        Vec::new()
    }

    pub(crate) fn external_move(
        &self,
        windows: &[WindowGraphPeerSnapshot],
        moved: WindowId,
        new_position: Point,
    ) -> Vec<WindowGraphMove> {
        let Some(moved_window) = windows.iter().find(|window| window.node.id == moved) else {
            return Vec::new();
        };
        let Some(group) = &moved_window.group else {
            return Vec::new();
        };
        if !group.policy.move_mode.moves_attached_component(group.leads) {
            return Vec::new();
        }

        let delta = Point::new(
            new_position.x - moved_window.node.position.x,
            new_position.y - moved_window.node.position.y,
        );
        if delta.x.abs() <= f32::EPSILON && delta.y.abs() <= f32::EPSILON {
            return Vec::new();
        }
        let group_windows = group_windows(windows, group);
        let component = attached_component(&group_windows, moved, group.policy.attach_epsilon);
        group_windows
            .into_iter()
            .filter(|window| component.contains(&window.id) && window.id != moved)
            .map(|window| WindowGraphMove {
                id: window.id,
                position: Point::new(window.position.x + delta.x, window.position.y + delta.y),
            })
            .collect()
    }
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn group_windows(
    windows: &[WindowGraphPeerSnapshot],
    group: &NativeWindowGroupMembership,
) -> Vec<WindowGraphNodeSnapshot> {
    windows
        .iter()
        .filter(|window| {
            window
                .group
                .as_ref()
                .is_some_and(|candidate| candidate.id == group.id)
        })
        .map(|window| window.node)
        .collect()
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn attached_component(
    windows: &[WindowGraphNodeSnapshot],
    dragged: WindowId,
    attach_epsilon: f32,
) -> Vec<WindowId> {
    let mut component = vec![dragged];
    let mut changed = true;

    while changed {
        changed = false;
        for candidate in windows {
            if component.contains(&candidate.id) {
                continue;
            }
            let attached_to_component = windows
                .iter()
                .filter(|window| component.contains(&window.id))
                .any(|window| rects_attached(candidate, window, attach_epsilon));
            if attached_to_component {
                component.push(candidate.id);
                changed = true;
            }
        }
    }

    component
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn rects_attached(
    child: &WindowGraphNodeSnapshot,
    main: &WindowGraphNodeSnapshot,
    attach_epsilon: f32,
) -> bool {
    let child_right = child.position.x + child.size.width;
    let child_bottom = child.position.y + child.size.height;
    let main_right = main.position.x + main.size.width;
    let main_bottom = main.position.y + main.size.height;

    let touches_horizontal = near(child.position.x, main_right, attach_epsilon)
        || near(child_right, main.position.x, attach_epsilon);
    let overlaps_vertical = ranges_overlap(
        child.position.y,
        child_bottom,
        main.position.y,
        main_bottom,
        attach_epsilon,
    );
    let touches_vertical = near(child.position.y, main_bottom, attach_epsilon)
        || near(child_bottom, main.position.y, attach_epsilon);
    let overlaps_horizontal = ranges_overlap(
        child.position.x,
        child_right,
        main.position.x,
        main_right,
        attach_epsilon,
    );

    touches_horizontal && overlaps_vertical || touches_vertical && overlaps_horizontal
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone, Copy, Debug, PartialEq)]
struct GraphSnap {
    target: WindowId,
    delta: Point,
    distance: f32,
    contact: f32,
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[derive(Clone, Copy, Debug, PartialEq)]
struct GraphSnapCandidate {
    delta: Point,
    contact: f32,
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn closest_snap(
    windows: &[WindowGraphNodeSnapshot],
    component: &[WindowId],
    snap_distance: f32,
) -> Option<GraphSnap> {
    let mut closest = None::<GraphSnap>;

    for moving in windows
        .iter()
        .filter(|window| component.contains(&window.id))
    {
        for stationary in windows
            .iter()
            .filter(|window| !component.contains(&window.id))
        {
            for candidate in snap_candidates(moving, stationary, snap_distance) {
                let snap = GraphSnap {
                    target: stationary.id,
                    delta: candidate.delta,
                    distance: candidate.delta.x.abs() + candidate.delta.y.abs(),
                    contact: candidate.contact,
                };
                if closest.is_none_or(|current| {
                    snap.contact > current.contact
                        || snap.contact == current.contact && snap.distance < current.distance
                }) {
                    closest = Some(snap);
                }
            }
        }
    }

    closest
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn snap_candidates(
    moving: &WindowGraphNodeSnapshot,
    stationary: &WindowGraphNodeSnapshot,
    snap_distance: f32,
) -> Vec<GraphSnapCandidate> {
    let moving_left = moving.position.x;
    let moving_top = moving.position.y;
    let moving_right = moving.position.x + moving.size.width;
    let moving_bottom = moving.position.y + moving.size.height;
    let stationary_left = stationary.position.x;
    let stationary_top = stationary.position.y;
    let stationary_right = stationary.position.x + stationary.size.width;
    let stationary_bottom = stationary.position.y + stationary.size.height;

    let mut candidates = Vec::new();
    if ranges_overlap_strict(moving_top, moving_bottom, stationary_top, stationary_bottom) {
        let contact =
            range_overlap_length(moving_top, moving_bottom, stationary_top, stationary_bottom);
        if near(moving_right, stationary_left, snap_distance) {
            candidates.push(GraphSnapCandidate {
                delta: Point::new(stationary_left - moving_right, 0.0),
                contact,
            });
        }
        if near(moving_left, stationary_right, snap_distance) {
            candidates.push(GraphSnapCandidate {
                delta: Point::new(stationary_right - moving_left, 0.0),
                contact,
            });
        }
    }
    if ranges_overlap_strict(moving_left, moving_right, stationary_left, stationary_right) {
        let contact =
            range_overlap_length(moving_left, moving_right, stationary_left, stationary_right);
        if near(moving_bottom, stationary_top, snap_distance) {
            candidates.push(GraphSnapCandidate {
                delta: Point::new(0.0, stationary_top - moving_bottom),
                contact,
            });
        }
        if near(moving_top, stationary_bottom, snap_distance) {
            candidates.push(GraphSnapCandidate {
                delta: Point::new(0.0, stationary_bottom - moving_top),
                contact,
            });
        }
    }

    candidates
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn translate_nodes(windows: &mut [WindowGraphNodeSnapshot], component: &[WindowId], delta: Point) {
    if delta.x.abs() <= f32::EPSILON && delta.y.abs() <= f32::EPSILON {
        return;
    }
    for window in windows {
        if component.contains(&window.id) {
            window.position.x += delta.x;
            window.position.y += delta.y;
        }
    }
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn near(a: f32, b: f32, distance: f32) -> bool {
    (a - b).abs() <= distance
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn ranges_overlap(a_start: f32, a_end: f32, b_start: f32, b_end: f32, attach_epsilon: f32) -> bool {
    a_start <= b_end + attach_epsilon && b_start <= a_end + attach_epsilon
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn ranges_overlap_strict(a_start: f32, a_end: f32, b_start: f32, b_end: f32) -> bool {
    a_start < b_end && b_start < a_end
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn range_overlap_length(a_start: f32, a_end: f32, b_start: f32, b_end: f32) -> f32 {
    (a_end.min(b_end) - a_start.max(b_start)).max(0.0)
}

#[cfg(test)]
#[path = "tests/native_window_tests.rs"]
mod tests;
