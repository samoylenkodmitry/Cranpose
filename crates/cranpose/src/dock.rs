//! Dockable panes: composables that live in one OS window now and another later.
//!
//! Every native window owns a whole `AppShell` — its own composition, its own
//! renderer, its own pointer state. Two windows are two compositions running in
//! parallel; a pane that moves from one to the other is composed from scratch
//! on arrival, because the composition it left is gone. So a dock keeps nothing
//! inside a window's composition that must survive the move. The topology —
//! which panes exist, which window holds each, which drag is in flight — lives
//! in a [`DockModel`] remembered by the root composition, which outlives every
//! window, and each window's content is a pure function of it.
//!
//! The drag is one gesture, as in a browser. The window that receives the press
//! keeps receiving every move until release, wherever the pointer goes, because
//! that is how every desktop routes a held button. A handler on that window's
//! root therefore survives the pane leaving it, and does the whole job: it tears
//! the pane into a window of its own, carries that window under the pointer by
//! setting its position, and lets the pane join another window when the pointer
//! enters that window's drop zone. Positions are read from the OS at the moment
//! of each event, so the carried window never drifts from the pointer.

use std::{
    cell::RefCell,
    collections::HashMap,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{
    CompositionLocalProvider, MutableState, Owned, StaticCompositionLocal, key, remember,
    rememberMutableStateOf, staticCompositionLocalOf,
};
use cranpose_ui::{
    BoxSpec, Modifier, Point, PointerEventKind, PointerInputScope, Size, composable,
};

use crate::native_window::{
    WindowConfig, WindowId, WindowNode, WindowState, current_native_window_surface_origin,
    rememberWindowState,
};

/// Names one pane for as long as it exists, in whichever window holds it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct DockKey(u64);

impl DockKey {
    /// A key for a pane the source names, such as a tool window.
    pub fn from_static(id: &'static str) -> Self {
        Self(hash_of(id, 0))
    }

    /// A key for a pane that exists because of user action, such as an opened
    /// document. `namespace` keeps one family of panes from colliding with
    /// another; `key` tells the panes of that family apart.
    pub fn from_runtime(namespace: &'static str, key: u64) -> Self {
        Self(hash_of(namespace, key))
    }

    /// The raw key, for deriving other identifiers.
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// Names one window of a dock.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct DockWindowId(u64);

impl DockWindowId {
    /// The raw identifier, for deriving a [`WindowId`].
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// One window of a dock: the panes it holds in order, and the one in front.
///
/// A window holds at least one pane unless it is parked: a window whose last
/// pane joined another window mid-drag stays, hidden, until the drag ends,
/// because it is the window that received the press and so the one the OS
/// still routes the drag to.
#[derive(Clone, PartialEq, Debug)]
pub struct DockWindow {
    /// Identifier of this window.
    pub id: DockWindowId,
    /// The panes this window holds, in the order it shows them.
    pub panes: Vec<DockKey>,
    /// The pane in front. Meaningless while the window is parked.
    pub active: DockKey,
    /// Where the window is first placed, in screen coordinates.
    pub origin: Point,
    /// Whether the window is empty and hidden until the current drag ends.
    pub parked: bool,
}

/// Where a carried pane joins a window when the pointer enters it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DockDropZone {
    /// A band this tall along the top of every window, like a tab strip.
    TopBand(f32),
    /// The whole window, grown outward by this much, like a snapping edge.
    Around(f32),
}

/// How a dock hosts its panes.
#[derive(Clone, PartialEq, Debug)]
pub struct DockPolicy {
    /// The OS title of every window of the dock.
    pub title: String,
    /// The content size of every window of the dock.
    pub window_size: Size,
    /// Where a carried pane may join a window.
    pub drop_zone: DockDropZone,
    /// How far a pressed pane travels before it tears out of its window.
    pub tear_distance: f32,
    /// Where the first window goes when no window exists yet.
    pub first_origin: Point,
    /// Where the pointer sits inside a window that has just been torn off.
    pub tear_grab: Point,
}

impl DockPolicy {
    /// Windows with a tab strip this tall: panes join by being dropped on the strip.
    pub fn tabs(title: impl Into<String>, window_size: Size, strip_height: f32) -> Self {
        Self {
            title: title.into(),
            window_size,
            drop_zone: DockDropZone::TopBand(strip_height),
            tear_distance: 24.0,
            first_origin: Point::new(200.0, 160.0),
            tear_grab: Point::new(56.0, strip_height / 2.0),
        }
    }

    /// Windows that snap: panes join when carried within this distance of a window.
    pub fn snapping(title: impl Into<String>, window_size: Size, snap_distance: f32) -> Self {
        Self {
            title: title.into(),
            window_size,
            drop_zone: DockDropZone::Around(snap_distance),
            tear_distance: 12.0,
            first_origin: Point::new(200.0, 160.0),
            tear_grab: Point::new(window_size.width / 2.0, 8.0),
        }
    }

    fn keeps(&self, screen: Point, anchor: Point, holder: DockRect) -> bool {
        match self.drop_zone {
            DockDropZone::TopBand(height) => {
                let y = screen.y - holder.origin.y;
                y >= -self.tear_distance && y <= height + self.tear_distance
            }
            DockDropZone::Around(_) => distance(screen, anchor) < self.tear_distance,
        }
    }

    fn zone_hit(
        &self,
        screen: Point,
        rects: &[DockRect],
        except: DockWindowId,
    ) -> Option<DockWindowId> {
        rects
            .iter()
            .filter(|rect| rect.window != except)
            .find(|rect| self.zone_contains(rect, screen))
            .map(|rect| rect.window)
    }

    fn zone_contains(&self, rect: &DockRect, point: Point) -> bool {
        let (origin, size) = (rect.origin, rect.size);
        match self.drop_zone {
            DockDropZone::TopBand(height) => {
                point.x >= origin.x
                    && point.x <= origin.x + size.width
                    && point.y >= origin.y
                    && point.y <= origin.y + height
            }
            DockDropZone::Around(reach) => {
                point.x >= origin.x - reach
                    && point.x <= origin.x + size.width + reach
                    && point.y >= origin.y - reach
                    && point.y <= origin.y + size.height + reach
            }
        }
    }
}

/// Where one window of a dock sits on screen.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DockRect {
    /// The window this rectangle belongs to.
    pub window: DockWindowId,
    /// Top-left corner, in screen coordinates.
    pub origin: Point,
    /// Content size.
    pub size: Size,
}

/// One pane being dragged, from press to release.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DockDrag {
    /// The pane under the pointer.
    pub pane: DockKey,
    /// The window that received the press, and so receives every move.
    pub source: DockWindowId,
    /// The pointer's offset inside the carried window.
    pub grab: Point,
    /// Where the pointer was when the pane last settled in a window.
    pub anchor: Point,
    /// The window carried under the pointer, when the pane is loose.
    pub carrying: Option<DockWindowId>,
}

/// What one step of a drag asks the windows to do.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DockStep {
    /// Nothing moved.
    Rest,
    /// Move this window so that its origin lands here.
    Carry(DockWindowId, Point),
    /// The pane joined this window.
    Join(DockWindowId),
}

/// Which panes exist, which window holds each, and the drag in flight.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct DockModel {
    windows: Vec<DockWindow>,
    drag: Option<DockDrag>,
    placements: Vec<(DockKey, DockWindowId)>,
    next_id: u64,
}

impl DockModel {
    /// A dock with no windows and no panes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every window of the dock, parked ones included.
    pub fn windows(&self) -> &[DockWindow] {
        &self.windows
    }

    /// The window with the given identifier.
    pub fn window(&self, id: DockWindowId) -> Option<&DockWindow> {
        self.windows.iter().find(|window| window.id == id)
    }

    /// The window currently holding the given pane.
    pub fn window_of(&self, pane: DockKey) -> Option<DockWindowId> {
        self.windows
            .iter()
            .find(|window| window.panes.contains(&pane))
            .map(|window| window.id)
    }

    /// The drag in flight, if any.
    pub fn drag(&self) -> Option<DockDrag> {
        self.drag
    }

    /// Brings the windows in line with the panes the application declares.
    ///
    /// Panes no longer declared leave their windows; panes declared for the
    /// first time go where [`DockModel::place_next_in`] asked, else into the
    /// first window, else into a new window at `first_origin`. Returns whether
    /// anything changed.
    pub fn reconcile(&mut self, declared: &[DockKey], first_origin: Point) -> bool {
        let gone: Vec<DockKey> = self
            .windows
            .iter()
            .flat_map(|window| window.panes.iter().copied())
            .filter(|pane| !declared.contains(pane))
            .collect();
        for pane in &gone {
            self.detach(*pane);
        }
        let mut changed = !gone.is_empty();
        for &pane in declared {
            if self.window_of(pane).is_none() {
                self.place(pane, first_origin);
                changed = true;
            }
        }
        changed
    }

    /// Asks that `pane`, when it is next declared, open in `window`.
    pub fn place_next_in(&mut self, pane: DockKey, window: DockWindowId) {
        self.placements.push((pane, window));
    }

    /// Brings the given pane to the front of its window.
    pub fn activate(&mut self, pane: DockKey) {
        if let Some(window) = self
            .windows
            .iter_mut()
            .find(|window| window.panes.contains(&pane))
        {
            window.active = pane;
        }
    }

    /// Starts dragging `pane` from `source`, where `grab` is the pointer inside
    /// the window and `screen` the pointer on screen. Returns whether a drag
    /// began.
    pub fn press(
        &mut self,
        pane: DockKey,
        source: DockWindowId,
        grab: Point,
        screen: Point,
    ) -> bool {
        if self.drag.is_some() || self.window_of(pane) != Some(source) {
            return false;
        }
        self.activate(pane);
        self.drag = Some(DockDrag {
            pane,
            source,
            grab,
            anchor: screen,
            carrying: None,
        });
        true
    }

    /// Moves the drag in flight to `screen`, given where every open window sits.
    pub fn drag_to(&mut self, screen: Point, rects: &[DockRect], policy: &DockPolicy) -> DockStep {
        let Some(drag) = self.drag else {
            return DockStep::Rest;
        };
        match drag.carrying {
            None => self.tear_if_pulled(drag, screen, rects, policy),
            Some(carried) => self.carry(drag, carried, screen, rects, policy),
        }
    }

    /// Ends the drag in flight, closing any window parked by it.
    pub fn release(&mut self) -> bool {
        let released = self.drag.take().is_some();
        self.windows.retain(|window| !window.parked);
        released
    }

    fn tear_if_pulled(
        &mut self,
        drag: DockDrag,
        screen: Point,
        rects: &[DockRect],
        policy: &DockPolicy,
    ) -> DockStep {
        let Some(holder) = self.window_of(drag.pane) else {
            return DockStep::Rest;
        };
        let Some(rect) = rects.iter().copied().find(|rect| rect.window == holder) else {
            return DockStep::Rest;
        };
        if policy.keeps(screen, drag.anchor, rect) {
            return DockStep::Rest;
        }
        let alone = self
            .window(holder)
            .is_some_and(|window| window.panes.len() == 1);
        let grab = if alone { drag.grab } else { policy.tear_grab };
        let origin = minus(screen, grab);
        let carried = if alone {
            holder
        } else {
            self.detach(drag.pane);
            self.reopen_or_open(drag.pane, origin)
        };
        self.drag = Some(DockDrag {
            grab,
            carrying: Some(carried),
            ..drag
        });
        DockStep::Carry(carried, origin)
    }

    fn carry(
        &mut self,
        drag: DockDrag,
        carried: DockWindowId,
        screen: Point,
        rects: &[DockRect],
        policy: &DockPolicy,
    ) -> DockStep {
        let origin = minus(screen, drag.grab);
        let Some(target) = policy.zone_hit(screen, rects, carried) else {
            return DockStep::Carry(carried, origin);
        };
        self.detach(drag.pane);
        self.attach(drag.pane, target);
        self.drag = Some(DockDrag {
            anchor: screen,
            carrying: None,
            ..drag
        });
        DockStep::Join(target)
    }

    fn place(&mut self, pane: DockKey, first_origin: Point) {
        let hinted = self.take_placement(pane);
        let target = hinted
            .filter(|id| self.window(*id).is_some_and(|window| !window.parked))
            .or_else(|| {
                self.windows
                    .iter()
                    .find(|window| !window.parked)
                    .map(|w| w.id)
            });
        match target {
            Some(id) => {
                self.attach(pane, id);
            }
            None => {
                self.open_window(pane, first_origin);
            }
        }
    }

    fn take_placement(&mut self, pane: DockKey) -> Option<DockWindowId> {
        let index = self.placements.iter().position(|(held, _)| *held == pane)?;
        Some(self.placements.remove(index).1)
    }

    fn attach(&mut self, pane: DockKey, window: DockWindowId) {
        if let Some(found) = self.windows.iter_mut().find(|held| held.id == window) {
            found.panes.push(pane);
            found.active = pane;
            found.parked = false;
        }
    }

    fn detach(&mut self, pane: DockKey) {
        let Some(index) = self
            .windows
            .iter()
            .position(|window| window.panes.contains(&pane))
        else {
            return;
        };
        let window = &mut self.windows[index];
        let removed = window
            .panes
            .iter()
            .position(|held| *held == pane)
            .unwrap_or_default();
        window.panes.retain(|held| *held != pane);
        match window.panes.len().checked_sub(1) {
            Some(last) if window.active == pane => window.active = window.panes[removed.min(last)],
            Some(_) => {}
            None if self.drag.is_some_and(|drag| drag.source == window.id) => window.parked = true,
            None => {
                self.windows.remove(index);
            }
        }
    }

    fn reopen_or_open(&mut self, pane: DockKey, origin: Point) -> DockWindowId {
        if let Some(parked) = self.windows.iter_mut().find(|window| window.parked) {
            parked.panes = vec![pane];
            parked.active = pane;
            parked.origin = origin;
            parked.parked = false;
            return parked.id;
        }
        self.open_window(pane, origin)
    }

    fn open_window(&mut self, pane: DockKey, origin: Point) -> DockWindowId {
        self.next_id += 1;
        let id = DockWindowId(self.next_id);
        self.windows.push(DockWindow {
            id,
            panes: vec![pane],
            active: pane,
            origin,
            parked: false,
        });
        id
    }
}

type PaneContent = Rc<RefCell<dyn FnMut()>>;

#[derive(Default)]
struct DockShared {
    contents: HashMap<DockKey, PaneContent>,
    declared: Vec<DockKey>,
    states: HashMap<DockWindowId, WindowState>,
    policy: Option<DockPolicy>,
    host: Option<Rc<dyn Fn(&DockHost)>>,
}

/// A handle to one dock, cheap to clone and valid inside every window of it.
#[derive(Clone)]
pub struct DockRef {
    id: &'static str,
    model: MutableState<DockModel>,
    shared: Owned<DockShared>,
}

impl PartialEq for DockRef {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.model.runtime_state_id() == other.model.runtime_state_id()
    }
}

impl DockRef {
    /// The dock's topology, read reactively.
    pub fn model(&self) -> MutableState<DockModel> {
        self.model
    }

    fn policy(&self) -> DockPolicy {
        self.shared
            .with(|shared| shared.policy.clone())
            .unwrap_or_else(|| DockPolicy::tabs("", Size::new(320.0, 240.0), 32.0))
    }

    fn adopt(&self, policy: DockPolicy, host: Rc<dyn Fn(&DockHost)>) {
        self.shared.update(|shared| {
            shared.policy = Some(policy);
            shared.host = Some(host);
        });
    }

    fn declare(&self, mut panes: impl FnMut()) -> Vec<DockKey> {
        self.shared.update(|shared| shared.declared.clear());
        {
            let _declaring = Declaring::begin(self.clone());
            panes();
        }
        let declared = self.shared.with(|shared| shared.declared.clone());
        self.shared.update(|shared| {
            shared.contents.retain(|pane, _| declared.contains(pane));
        });
        declared
    }

    fn reconcile(&self, declared: &[DockKey], first_origin: Point) {
        let mut next = self.model.get_non_reactive();
        if next.reconcile(declared, first_origin) {
            self.model.set(next);
        }
    }

    fn rects(&self) -> Vec<DockRect> {
        let model = self.model.get_non_reactive();
        self.shared.with(|shared| {
            model
                .windows()
                .iter()
                .filter(|window| !window.parked)
                .filter_map(|window| {
                    let state = shared.states.get(&window.id)?;
                    Some(DockRect {
                        window: window.id,
                        origin: state.position_non_reactive()?,
                        size: state.size_non_reactive(),
                    })
                })
                .collect()
        })
    }

    fn press(&self, pane: DockKey, window: DockWindowId, local: Point) {
        let Some(origin) = current_native_window_surface_origin() else {
            return;
        };
        let screen = plus(origin, local);
        self.model.update(|model| {
            model.press(pane, window, local, screen);
        });
    }

    fn drag_step(&self, window: DockWindowId, local: Point) {
        let drives = self
            .model
            .get_non_reactive()
            .drag()
            .is_some_and(|drag| drag.source == window);
        let Some(origin) = current_native_window_surface_origin() else {
            return;
        };
        if !drives {
            return;
        }
        let screen = plus(origin, local);
        let policy = self.policy();
        let rects = self.rects();
        let step = self
            .model
            .update(|model| model.drag_to(screen, &rects, &policy));
        if let DockStep::Carry(carried, at) = step {
            let state = self
                .shared
                .with(|shared| shared.states.get(&carried).copied());
            if let Some(state) = state {
                state.set_position(Some(at));
            }
        }
    }

    fn release(&self) {
        self.model.update(|model| {
            model.release();
        });
    }
}

struct Declaring;

impl Declaring {
    fn begin(dock: DockRef) -> Self {
        DECLARING.with(|stack| stack.borrow_mut().push(dock));
        Self
    }
}

impl Drop for Declaring {
    fn drop(&mut self) {
        DECLARING.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

thread_local! {
    static DECLARING: RefCell<Vec<DockRef>> = const { RefCell::new(Vec::new()) };
}

/// One window of a dock, as seen by the code that draws its chrome.
#[derive(Clone)]
pub struct DockHost {
    dock: DockRef,
    window: DockWindow,
}

impl PartialEq for DockHost {
    fn eq(&self, other: &Self) -> bool {
        self.dock == other.dock && self.window == other.window
    }
}

impl DockHost {
    /// The dock this window belongs to.
    pub fn dock(&self) -> &DockRef {
        &self.dock
    }

    /// This window's identifier.
    pub fn window(&self) -> DockWindowId {
        self.window.id
    }

    /// The panes this window holds, in order.
    pub fn panes(&self) -> &[DockKey] {
        &self.window.panes
    }

    /// The pane in front.
    pub fn active(&self) -> DockKey {
        self.window.active
    }

    /// Whether this window is the one being carried under the pointer.
    pub fn is_carried(&self) -> bool {
        self.dock
            .model
            .get_non_reactive()
            .drag()
            .is_some_and(|drag| drag.carrying == Some(self.window.id))
    }

    /// Brings the given pane to the front.
    pub fn activate(&self, pane: DockKey) {
        self.dock.model.update(|model| model.activate(pane));
    }

    /// Asks that `pane`, when the application next declares it, open here.
    pub fn open_next_here(&self, pane: DockKey) {
        let window = self.window.id;
        self.dock
            .model
            .update(|model| model.place_next_in(pane, window));
    }

    /// Composes the given pane's content at this point of the window.
    pub fn content(&self, pane: DockKey) {
        let content = self
            .dock
            .shared
            .with(|shared| shared.contents.get(&pane).cloned());
        if let Some(content) = content {
            (content.borrow_mut())();
        }
    }
}

#[derive(Clone)]
struct DockScope {
    dock: DockRef,
    window: DockWindowId,
}

fn local_dock_scope() -> StaticCompositionLocal<Option<DockScope>> {
    thread_local! {
        static LOCAL: RefCell<Option<StaticCompositionLocal<Option<DockScope>>>> =
            const { RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| staticCompositionLocalOf(|| None))
            .clone()
    })
}

/// Modifier helpers for the panes of a [`Dock`].
pub trait DockModifierExt {
    /// Makes this component the grip by which its pane is dragged between windows.
    ///
    /// Pressing it brings the pane to the front. Pulling it out of the window's
    /// drop zone tears the pane into a window of its own that follows the
    /// pointer, and carrying it into another window's drop zone makes the pane
    /// join that window — all in one gesture. Inert outside a dock window.
    fn dock_handle(self, pane: DockKey) -> Modifier;
}

impl DockModifierExt for Modifier {
    fn dock_handle(self, pane: DockKey) -> Modifier {
        let Some(scope) = local_dock_scope().current() else {
            return self;
        };
        self.pointer_input(pane, move |input: PointerInputScope| {
            let scope = scope.clone();
            async move {
                input
                    .await_pointer_event_scope(|await_scope| async move {
                        loop {
                            let event = await_scope.await_pointer_event().await;
                            if event.kind == PointerEventKind::Down {
                                scope.dock.press(pane, scope.window, event.global_position);
                                event.consume();
                            }
                        }
                    })
                    .await;
            }
        })
    }
}

/// Declares one pane of the enclosing [`Dock`].
///
/// The content is not composed here: it is composed inside whichever window
/// currently holds the pane, where the window's chrome calls
/// [`DockHost::content`]. Declaring a pane for the first time opens it; no
/// longer declaring it closes it.
#[allow(non_snake_case)]
pub fn Pane(key: DockKey, content: impl FnMut() + 'static) {
    let Some(dock) = DECLARING.with(|stack| stack.borrow().last().cloned()) else {
        return;
    };
    dock.shared.update(|shared| {
        shared.declared.push(key);
        shared.contents.insert(key, Rc::new(RefCell::new(content)));
    });
}

/// Hosts panes in as many OS windows as the user has torn them into.
///
/// `panes` declares the panes with [`Pane`]; `host` draws one window's chrome
/// around them, calling [`DockHost::content`] wherever a pane's content goes
/// and marking grips with [`DockModifierExt::dock_handle`]. Everything else —
/// which window holds what, tearing, carrying, joining, opening and closing
/// windows — is the dock's.
#[composable]
#[allow(non_snake_case)]
pub fn Dock(
    id: &'static str,
    policy: DockPolicy,
    host: impl Fn(&DockHost) + 'static,
    panes: impl FnMut() + 'static,
) {
    let dock = rememberDock(id);
    dock.adopt(policy.clone(), Rc::new(host));
    let declared = dock.declare(panes);
    dock.reconcile(&declared, policy.first_origin);
    let windows = dock.model.get().windows().to_vec();
    for window in windows {
        let dock = dock.clone();
        key(window.id.raw(), move || DockWindowNode(dock, window));
    }
}

#[composable]
#[track_caller]
#[allow(non_snake_case)]
fn rememberDock(id: &'static str) -> DockRef {
    let model = rememberMutableStateOf(DockModel::new);
    let shared = remember(DockShared::default);
    DockRef { id, model, shared }
}

#[composable]
#[allow(non_snake_case)]
fn DockWindowNode(dock: DockRef, window: DockWindow) {
    let policy = dock.policy();
    let state = rememberWindowState(policy.window_size.width, policy.window_size.height);
    dock.shared.update(|shared| {
        shared.states.insert(window.id, state);
    });
    let id = window.id;
    WindowNode(
        WindowId::from_runtime(dock.id, id.raw()),
        WindowConfig::borderless_for_state(policy.title.clone(), state)
            .with_position(window.origin.x, window.origin.y)
            .with_visible(!window.parked),
        move || DockHostRoot(dock.clone(), id),
    );
}

#[composable]
#[allow(non_snake_case)]
fn DockHostRoot(dock: DockRef, id: DockWindowId) {
    let snapshot = dock.model.get();
    let Some(window) = snapshot.window(id).cloned() else {
        return;
    };
    let host = DockHost {
        dock: dock.clone(),
        window,
    };
    let scope = DockScope {
        dock: dock.clone(),
        window: id,
    };
    let draw = dock.shared.with(|shared| shared.host.clone());
    CompositionLocalProvider([local_dock_scope().provides(Some(scope))], move || {
        cranpose_ui::Box(
            drag_session(Modifier::empty().fill_max_size(), dock, id),
            BoxSpec::default(),
            move || {
                if let Some(draw) = &draw {
                    draw(&host);
                }
            },
        );
    });
}

fn drag_session(base: Modifier, dock: DockRef, id: DockWindowId) -> Modifier {
    base.pointer_input(id, move |scope: PointerInputScope| {
        let dock = dock.clone();
        async move {
            scope
                .await_pointer_event_scope(|await_scope| async move {
                    loop {
                        let event = await_scope.await_pointer_event().await;
                        match event.kind {
                            PointerEventKind::Move => dock.drag_step(id, event.global_position),
                            PointerEventKind::Up | PointerEventKind::Cancel => dock.release(),
                            _ => {}
                        }
                    }
                })
                .await;
        }
    })
}

fn hash_of(namespace: &'static str, key: u64) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    namespace.hash(&mut hasher);
    key.hash(&mut hasher);
    hasher.finish()
}

fn plus(a: Point, b: Point) -> Point {
    Point::new(a.x + b.x, a.y + b.y)
}

fn minus(a: Point, b: Point) -> Point {
    Point::new(a.x - b.x, a.y - b.y)
}

fn distance(a: Point, b: Point) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: Point = Point { x: 100.0, y: 100.0 };
    const SIZE: Size = Size {
        width: 400.0,
        height: 300.0,
    };

    fn pane(n: u64) -> DockKey {
        DockKey::from_runtime("pane", n)
    }

    fn tabs() -> DockPolicy {
        DockPolicy::tabs("t", SIZE, 30.0)
    }

    fn rect(model: &DockModel, window: DockWindowId, origin: Point) -> DockRect {
        let _ = model;
        DockRect {
            window,
            origin,
            size: SIZE,
        }
    }

    fn two_panes_in_one_window() -> (DockModel, DockWindowId) {
        let mut model = DockModel::new();
        model.reconcile(&[pane(1), pane(2)], ORIGIN);
        let window = model.windows()[0].id;
        (model, window)
    }

    #[test]
    fn declared_panes_open_in_one_window() {
        let (model, window) = two_panes_in_one_window();
        assert_eq!(model.windows().len(), 1);
        assert_eq!(
            model.window(window).map(|w| w.panes.clone()),
            Some(vec![pane(1), pane(2)])
        );
    }

    #[test]
    fn an_undeclared_pane_leaves_and_an_empty_window_closes() {
        let (mut model, _) = two_panes_in_one_window();
        assert!(model.reconcile(&[pane(2)], ORIGIN));
        assert_eq!(model.window_of(pane(1)), None);
        assert!(model.reconcile(&[], ORIGIN));
        assert!(model.windows().is_empty());
    }

    #[test]
    fn reconciling_with_nothing_new_changes_nothing() {
        let (mut model, _) = two_panes_in_one_window();
        assert!(!model.reconcile(&[pane(1), pane(2)], ORIGIN));
    }

    #[test]
    fn a_placement_hint_sends_a_new_pane_to_that_window() {
        let (mut model, first) = two_panes_in_one_window();
        model.press(
            pane(2),
            first,
            Point::new(80.0, 15.0),
            Point::new(180.0, 115.0),
        );
        let rects = [rect(&model, first, ORIGIN)];
        model.drag_to(Point::new(180.0, 300.0), &rects, &tabs());
        model.release();
        let second = model.window_of(pane(2)).expect("torn window");
        model.place_next_in(pane(3), second);
        model.reconcile(&[pane(1), pane(2), pane(3)], ORIGIN);
        assert_eq!(model.window_of(pane(3)), Some(second));
    }

    #[test]
    fn pulling_inside_the_strip_does_not_tear() {
        let (mut model, window) = two_panes_in_one_window();
        model.press(
            pane(2),
            window,
            Point::new(80.0, 15.0),
            Point::new(180.0, 115.0),
        );
        let rects = [rect(&model, window, ORIGIN)];
        let step = model.drag_to(Point::new(260.0, 120.0), &rects, &tabs());
        assert_eq!(step, DockStep::Rest);
        assert_eq!(model.windows().len(), 1);
    }

    #[test]
    fn pulling_out_of_the_strip_tears_into_a_carried_window() {
        let (mut model, window) = two_panes_in_one_window();
        model.press(
            pane(2),
            window,
            Point::new(80.0, 15.0),
            Point::new(180.0, 115.0),
        );
        let rects = [rect(&model, window, ORIGIN)];
        let step = model.drag_to(Point::new(180.0, 300.0), &rects, &tabs());
        let DockStep::Carry(carried, at) = step else {
            panic!("expected a carry, got {step:?}");
        };
        assert_ne!(carried, window);
        assert_eq!(model.window_of(pane(2)), Some(carried));
        assert_eq!(at, minus(Point::new(180.0, 300.0), tabs().tear_grab));
        assert_eq!(model.drag().and_then(|d| d.carrying), Some(carried));
    }

    #[test]
    fn a_carried_pane_joins_the_window_whose_strip_it_enters() {
        let (mut model, window) = two_panes_in_one_window();
        model.press(
            pane(2),
            window,
            Point::new(80.0, 15.0),
            Point::new(180.0, 115.0),
        );
        let rects = [rect(&model, window, ORIGIN)];
        let DockStep::Carry(carried, _) = model.drag_to(Point::new(180.0, 300.0), &rects, &tabs())
        else {
            panic!("expected a carry");
        };
        let rects = [
            rect(&model, window, ORIGIN),
            rect(&model, carried, Point::new(600.0, 600.0)),
        ];
        let step = model.drag_to(Point::new(300.0, 110.0), &rects, &tabs());
        assert_eq!(step, DockStep::Join(window));
        assert_eq!(model.window_of(pane(2)), Some(window));
        assert!(model.window(carried).is_none());
        assert_eq!(model.drag().and_then(|d| d.carrying), None);
    }

    #[test]
    fn a_lone_pane_carries_its_own_window_and_parks_it_on_joining() {
        let mut model = DockModel::new();
        model.reconcile(&[pane(1)], ORIGIN);
        let first = model.windows()[0].id;
        model.press(
            pane(1),
            first,
            Point::new(80.0, 15.0),
            Point::new(180.0, 115.0),
        );
        let rects = [rect(&model, first, ORIGIN)];
        model.drag_to(Point::new(180.0, 300.0), &rects, &tabs());
        model.release();
        let second = model.window_of(pane(1)).expect("lone window");
        assert_eq!(second, first);

        model.reconcile(&[pane(1), pane(2)], ORIGIN);
        let other = model.window_of(pane(2)).expect("second window");
        assert_eq!(other, first);
        model.drag_to(Point::new(0.0, 0.0), &[], &tabs());
        let step = model.press(
            pane(1),
            first,
            Point::new(10.0, 10.0),
            Point::new(110.0, 110.0),
        );
        assert!(step);
    }

    fn torn_out_then_carried_back() -> (DockModel, DockWindowId, DockWindowId, [DockRect; 2]) {
        let mut model = DockModel::new();
        model.reconcile(&[pane(1), pane(2)], ORIGIN);
        let first = model.windows()[0].id;
        model.press(
            pane(2),
            first,
            Point::new(80.0, 15.0),
            Point::new(180.0, 115.0),
        );
        let rects = [rect(&model, first, ORIGIN)];
        let DockStep::Carry(second, _) = model.drag_to(Point::new(180.0, 300.0), &rects, &tabs())
        else {
            panic!("expected a carry");
        };
        model.release();

        model.press(
            pane(2),
            second,
            Point::new(20.0, 15.0),
            Point::new(620.0, 615.0),
        );
        let rects = [
            rect(&model, first, ORIGIN),
            rect(&model, second, Point::new(600.0, 600.0)),
        ];
        let step = model.drag_to(Point::new(620.0, 700.0), &rects, &tabs());
        assert_eq!(step, DockStep::Carry(second, Point::new(600.0, 685.0)));
        (model, first, second, rects)
    }

    #[test]
    fn a_lone_window_that_joins_another_is_parked_until_release() {
        let (mut model, first, second, rects) = torn_out_then_carried_back();
        let step = model.drag_to(Point::new(300.0, 110.0), &rects, &tabs());
        assert_eq!(step, DockStep::Join(first));
        assert_eq!(model.window(second).map(|w| w.parked), Some(true));
        assert_eq!(model.windows().len(), 2);
        model.release();
        assert_eq!(model.windows().len(), 1);
    }

    #[test]
    fn tearing_again_reuses_the_parked_source_window() {
        let (mut model, first, second, rects) = torn_out_then_carried_back();
        model.drag_to(Point::new(300.0, 110.0), &rects, &tabs());
        let rects = [rect(&model, first, ORIGIN)];
        let step = model.drag_to(Point::new(300.0, 400.0), &rects, &tabs());
        let DockStep::Carry(reused, _) = step else {
            panic!("expected a carry, got {step:?}");
        };
        assert_eq!(reused, second);
        assert_eq!(model.window(second).map(|w| w.parked), Some(false));
    }

    #[test]
    fn closing_the_shown_pane_shows_its_neighbour() {
        let mut model = DockModel::new();
        model.reconcile(&[pane(1), pane(2), pane(3)], ORIGIN);
        let window = model.windows()[0].id;
        model.activate(pane(2));
        model.reconcile(&[pane(1), pane(3)], ORIGIN);
        assert_eq!(model.window(window).map(|w| w.active), Some(pane(3)));
    }
}
