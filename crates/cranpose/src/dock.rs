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
//! setting its position, and lets the pane join another window when it reaches
//! that window's drop zone. Positions are read from the OS at the moment of
//! each event, so the carried window never drifts from the pointer.
//!
//! Two layouts. [`DockLayout::Tabs`] shows one pane at a time behind a strip,
//! and a pane joins by being dropped on the strip. [`DockLayout::Stack`] shows
//! every pane laid along an axis, sizes the window from its panes, and a pane
//! joins by being carried edge to edge with a window it lines up with, the way
//! a classic media player's windows glue together; tearing a pane out of the
//! middle of a stack splits the stack around the gap so nothing else moves.
//! A grip marked with [`DockModifierExt::dock_handle`] tears and snaps its
//! pane; a [`WindowModifierExt::window_drag_area`] moves the whole window.
//!
//! Every window of a dock is transparent. A window torn off mid-drag comes
//! up before the desktop has marked it visible, and until it has, nothing
//! can be presented into it; an opaque window would show the desktop's
//! window colour for those frames, a transparent one shows nothing.

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

#[allow(unused_imports)]
use crate::native_window::WindowModifierExt;
use crate::native_window::{
    WindowConfig, WindowId, WindowNode, WindowState, current_native_window_surface_origin,
    rememberWindowStateAt,
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
    /// Where the window sits, in screen coordinates, as the dock last placed
    /// it or last saw the OS place it.
    pub origin: Point,
    /// Whether the window is empty and hidden until the current drag ends.
    pub parked: bool,
}

/// The axis along which a stacking dock lays a window's panes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockAxis {
    /// Panes stack top to bottom.
    Vertical,
    /// Panes sit left to right.
    Horizontal,
}

impl DockAxis {
    fn along(self, size: Size) -> f32 {
        match self {
            Self::Vertical => size.height,
            Self::Horizontal => size.width,
        }
    }

    fn across(self, size: Size) -> f32 {
        match self {
            Self::Vertical => size.width,
            Self::Horizontal => size.height,
        }
    }

    fn at(self, point: Point) -> f32 {
        match self {
            Self::Vertical => point.y,
            Self::Horizontal => point.x,
        }
    }

    fn beside(self, point: Point) -> f32 {
        match self {
            Self::Vertical => point.x,
            Self::Horizontal => point.y,
        }
    }

    fn size(self, along: f32, across: f32) -> Size {
        match self {
            Self::Vertical => Size::new(across, along),
            Self::Horizontal => Size::new(along, across),
        }
    }

    fn shifted(self, point: Point, along: f32) -> Point {
        match self {
            Self::Vertical => Point::new(point.x, point.y + along),
            Self::Horizontal => Point::new(point.x + along, point.y),
        }
    }
}

/// How a dock lays out one window's panes, and where a carried pane joins.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DockLayout {
    /// One pane in front at a time, behind a strip along the top. Every
    /// window has the policy's `window_size`; a pane joins a window by being
    /// dropped on its strip.
    Tabs {
        /// Height of the strip.
        strip_height: f32,
    },
    /// Every pane shown, laid along `axis` in order; a window is as large as
    /// its panes together. A carried pane joins a window when the window
    /// carrying it lines up with that window across the axis and its edge
    /// comes within `reach` of that window's edge along the axis, going before
    /// it or after it.
    Stack {
        /// The direction panes are laid along.
        axis: DockAxis,
        /// How close two edges must come to snap.
        reach: f32,
    },
}

/// Which end of a window a joining pane goes to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockSide {
    /// In front of the window's first pane.
    Before,
    /// After the window's last pane.
    After,
}

/// How a dock hosts its panes.
#[derive(Clone, PartialEq, Debug)]
pub struct DockPolicy {
    /// The OS title of every window of the dock.
    pub title: String,
    /// The size of every window of a tabs dock, and of any pane that declares
    /// no size of its own.
    pub window_size: Size,
    /// How a window lays out its panes, and where a carried pane joins.
    pub layout: DockLayout,
    /// How far a pressed pane travels before it tears out of its window.
    pub tear_distance: f32,
    /// Where the first window goes when no window exists yet.
    pub first_origin: Point,
    /// Where the pointer sits inside a window torn off a tabs dock.
    pub tear_grab: Point,
}

impl DockPolicy {
    /// Windows with a tab strip this tall: panes join by being dropped on the strip.
    pub fn tabs(title: impl Into<String>, window_size: Size, strip_height: f32) -> Self {
        Self {
            title: title.into(),
            window_size,
            layout: DockLayout::Tabs { strip_height },
            tear_distance: 24.0,
            first_origin: Point::new(200.0, 160.0),
            tear_grab: Point::new(56.0, strip_height / 2.0),
        }
    }

    /// Windows that stack their panes along `axis` and snap edge to edge:
    /// a carried pane joins a window when their edges come within `reach`.
    /// `pane_size` is the size of a pane that declares none.
    pub fn stack(title: impl Into<String>, pane_size: Size, axis: DockAxis, reach: f32) -> Self {
        Self {
            title: title.into(),
            window_size: pane_size,
            layout: DockLayout::Stack { axis, reach },
            tear_distance: 12.0,
            first_origin: Point::new(200.0, 160.0),
            tear_grab: Point::new(0.0, 0.0),
        }
    }

    fn keeps(&self, screen: Point, anchor: Point, holder: DockRect) -> bool {
        match self.layout {
            DockLayout::Tabs { strip_height } => {
                let y = screen.y - holder.origin.y;
                y >= -self.tear_distance && y <= strip_height + self.tear_distance
            }
            DockLayout::Stack { .. } => distance(screen, anchor) < self.tear_distance,
        }
    }

    fn join_target(
        &self,
        carried: DockRect,
        screen: Point,
        rects: &[DockRect],
    ) -> Option<(DockWindowId, DockSide)> {
        let mut others = rects.iter().filter(|rect| rect.window != carried.window);
        match self.layout {
            DockLayout::Tabs { strip_height } => others
                .filter(|rect| strip_contains(rect, strip_height, screen))
                .map(|rect| (rect.window, DockSide::After))
                .next(),
            DockLayout::Stack { axis, reach } => others.find_map(|rect| {
                stack_side(axis, reach, carried, *rect).map(|side| (rect.window, side))
            }),
        }
    }
}

fn strip_contains(rect: &DockRect, strip_height: f32, point: Point) -> bool {
    let (origin, size) = (rect.origin, rect.size);
    point.x >= origin.x
        && point.x <= origin.x + size.width
        && point.y >= origin.y
        && point.y <= origin.y + strip_height
}

fn whole(point: Point) -> Point {
    Point::new(point.x.round(), point.y.round())
}

fn tear_grab(drag: DockDrag, alone: bool, policy: &DockPolicy) -> Point {
    match policy.layout {
        DockLayout::Tabs { .. } if !alone => policy.tear_grab,
        DockLayout::Tabs { .. } | DockLayout::Stack { .. } => drag.grab,
    }
}

fn stack_side(axis: DockAxis, reach: f32, carried: DockRect, target: DockRect) -> Option<DockSide> {
    let aligned = (axis.beside(carried.origin) - axis.beside(target.origin)).abs() <= reach;
    if !aligned {
        return None;
    }
    let carried_start = axis.at(carried.origin);
    let carried_end = carried_start + axis.along(carried.size);
    let target_start = axis.at(target.origin);
    let target_end = target_start + axis.along(target.size);
    if (carried_end - target_start).abs() <= reach {
        Some(DockSide::Before)
    } else if (carried_start - target_end).abs() <= reach {
        Some(DockSide::After)
    } else {
        None
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
    /// The pointer's offset inside the pane, which is its offset inside the
    /// window carrying the pane alone.
    pub grab: Point,
    /// Where the pointer was when the pane last settled in a window.
    pub anchor: Point,
    /// The window carried under the pointer, when the pane is loose.
    pub carrying: Option<DockWindowId>,
    /// Whether the carried window has been clear of every window since it
    /// was torn off; a pane joins nothing until it has been.
    pub clear: bool,
}

/// What one step of a drag did.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DockStep {
    /// Nothing moved.
    Rest,
    /// This window is carried, and its origin is now here.
    Carry(DockWindowId, Point),
    /// The pane joined this window at this end.
    Join(DockWindowId, DockSide),
}

/// Which panes exist, how large each is, which window holds each, and the
/// drag in flight.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct DockModel {
    windows: Vec<DockWindow>,
    sizes: Vec<(DockKey, Size)>,
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

    /// The size the given pane was declared with.
    pub fn pane_size(&self, pane: DockKey) -> Size {
        self.sizes
            .iter()
            .find(|(held, _)| *held == pane)
            .map(|(_, size)| *size)
            .unwrap_or(Size::new(0.0, 0.0))
    }

    /// The content size of the given window under `policy`: the policy's
    /// window size for tabs, the panes laid along the axis for a stack.
    pub fn window_size(&self, window: DockWindowId, policy: &DockPolicy) -> Size {
        match policy.layout {
            DockLayout::Tabs { .. } => policy.window_size,
            DockLayout::Stack { axis, .. } => {
                let panes = self
                    .window(window)
                    .map(|window| window.panes.as_slice())
                    .unwrap_or(&[]);
                let across = panes
                    .iter()
                    .map(|pane| axis.across(self.pane_size(*pane)))
                    .fold(0.0, f32::max);
                axis.size(self.extent(panes, axis), across)
            }
        }
    }

    /// Where the given pane's top-left corner sits inside its window.
    pub fn pane_offset(&self, pane: DockKey, policy: &DockPolicy) -> Point {
        let DockLayout::Stack { axis, .. } = policy.layout else {
            return Point::new(0.0, 0.0);
        };
        let before: Vec<DockKey> = self
            .windows
            .iter()
            .find(|window| window.panes.contains(&pane))
            .map(|window| {
                window
                    .panes
                    .iter()
                    .take_while(|held| **held != pane)
                    .copied()
                    .collect()
            })
            .unwrap_or_default();
        axis.shifted(Point::new(0.0, 0.0), self.extent(&before, axis))
    }

    /// Takes the windows' current positions from where the OS put them.
    /// Returns whether any window had moved.
    pub fn adopt(&mut self, rects: &[DockRect]) -> bool {
        let mut moved = false;
        for rect in rects {
            if let Some(window) = self
                .windows
                .iter_mut()
                .find(|window| window.id == rect.window)
                && window.origin != rect.origin
            {
                window.origin = rect.origin;
                moved = true;
            }
        }
        moved
    }

    /// Brings the windows in line with the panes the application declares,
    /// each with its size.
    ///
    /// Panes no longer declared leave their windows; panes declared for the
    /// first time go where [`DockModel::place_next_in`] asked, else into the
    /// first window, else into a new window at the policy's first origin.
    /// Returns whether anything changed.
    pub fn reconcile(&mut self, declared: &[(DockKey, Size)], policy: &DockPolicy) -> bool {
        let gone: Vec<DockKey> = self
            .windows
            .iter()
            .flat_map(|window| window.panes.iter().copied())
            .filter(|pane| !declared.iter().any(|(held, _)| held == pane))
            .collect();
        for pane in &gone {
            self.detach(*pane, policy);
        }
        let mut changed = !gone.is_empty() || self.sizes != declared;
        self.sizes = declared.to_vec();
        for (pane, _) in declared {
            if self.window_of(*pane).is_none() {
                self.place(*pane, policy);
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

    /// Starts dragging `pane` from `source`, where `local` is the pointer
    /// inside the window and `screen` the pointer on screen. Returns whether a
    /// drag began.
    pub fn press(
        &mut self,
        pane: DockKey,
        source: DockWindowId,
        local: Point,
        screen: Point,
        policy: &DockPolicy,
    ) -> bool {
        if self.drag.is_some() || self.window_of(pane) != Some(source) {
            return false;
        }
        self.activate(pane);
        self.drag = Some(DockDrag {
            pane,
            source,
            grab: minus(local, self.pane_offset(pane, policy)),
            anchor: screen,
            carrying: None,
            clear: false,
        });
        true
    }

    /// Moves the drag in flight to `screen`, given where every open window sits.
    pub fn drag_to(&mut self, screen: Point, rects: &[DockRect], policy: &DockPolicy) -> DockStep {
        self.adopt(rects);
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

    fn extent(&self, panes: &[DockKey], axis: DockAxis) -> f32 {
        panes
            .iter()
            .map(|pane| axis.along(self.pane_size(*pane)))
            .sum()
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
        let grab = tear_grab(drag, alone, policy);
        let origin = whole(minus(screen, grab));
        let carried = if alone {
            self.set_origin(holder, origin);
            holder
        } else {
            self.detach(drag.pane, policy);
            self.reopen_or_open(drag.pane, origin)
        };
        let rect = DockRect {
            window: carried,
            origin,
            size: self.window_size(carried, policy),
        };
        let clear = policy
            .join_target(rect, screen, &self.placed_rects(policy))
            .is_none();
        self.drag = Some(DockDrag {
            grab,
            carrying: Some(carried),
            clear,
            ..drag
        });
        DockStep::Carry(carried, origin)
    }

    fn placed_rects(&self, policy: &DockPolicy) -> Vec<DockRect> {
        self.windows
            .iter()
            .filter(|window| !window.parked)
            .map(|window| DockRect {
                window: window.id,
                origin: window.origin,
                size: self.window_size(window.id, policy),
            })
            .collect()
    }

    fn carry(
        &mut self,
        drag: DockDrag,
        carried: DockWindowId,
        screen: Point,
        rects: &[DockRect],
        policy: &DockPolicy,
    ) -> DockStep {
        let origin = whole(minus(screen, drag.grab));
        let rect = DockRect {
            window: carried,
            origin,
            size: self.window_size(carried, policy),
        };
        let target = policy.join_target(rect, screen, rects);
        let Some((target, side)) = target.filter(|_| drag.clear) else {
            self.set_origin(carried, origin);
            self.drag = Some(DockDrag {
                clear: drag.clear || target.is_none(),
                ..drag
            });
            return DockStep::Carry(carried, origin);
        };
        self.detach(drag.pane, policy);
        self.attach(drag.pane, target, side, policy);
        self.drag = Some(DockDrag {
            anchor: screen,
            carrying: None,
            ..drag
        });
        DockStep::Join(target, side)
    }

    fn set_origin(&mut self, window: DockWindowId, origin: Point) {
        if let Some(found) = self.windows.iter_mut().find(|held| held.id == window) {
            found.origin = origin;
        }
    }

    fn place(&mut self, pane: DockKey, policy: &DockPolicy) {
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
            Some(id) => self.attach(pane, id, DockSide::After, policy),
            None => {
                self.open_window(vec![pane], policy.first_origin);
            }
        }
    }

    fn take_placement(&mut self, pane: DockKey) -> Option<DockWindowId> {
        let index = self.placements.iter().position(|(held, _)| *held == pane)?;
        Some(self.placements.remove(index).1)
    }

    fn attach(&mut self, pane: DockKey, window: DockWindowId, side: DockSide, policy: &DockPolicy) {
        let pull = match (policy.layout, side) {
            (DockLayout::Stack { axis, .. }, DockSide::Before) => {
                Some((axis, -axis.along(self.pane_size(pane))))
            }
            _ => None,
        };
        let Some(found) = self.windows.iter_mut().find(|held| held.id == window) else {
            return;
        };
        match side {
            DockSide::Before => found.panes.insert(0, pane),
            DockSide::After => found.panes.push(pane),
        }
        if let Some((axis, by)) = pull {
            found.origin = axis.shifted(found.origin, by);
        }
        found.active = pane;
        found.parked = false;
    }

    fn detach(&mut self, pane: DockKey, policy: &DockPolicy) {
        let Some(index) = self
            .windows
            .iter()
            .position(|window| window.panes.contains(&pane))
        else {
            return;
        };
        let removed = self.windows[index]
            .panes
            .iter()
            .position(|held| *held == pane)
            .unwrap_or_default();
        self.windows[index].panes.retain(|held| *held != pane);
        if let DockLayout::Stack { axis, .. } = policy.layout {
            self.keep_neighbours_in_place(index, removed, pane, axis);
        }
        let window = &mut self.windows[index];
        match window.panes.len().checked_sub(1) {
            Some(last) if window.active == pane => window.active = window.panes[removed.min(last)],
            Some(_) => {}
            None if self.drag.is_some_and(|drag| drag.source == window.id) => window.parked = true,
            None => {
                self.windows.remove(index);
            }
        }
    }

    fn keep_neighbours_in_place(
        &mut self,
        index: usize,
        removed: usize,
        pane: DockKey,
        axis: DockAxis,
    ) {
        let gap = axis.along(self.pane_size(pane));
        if removed == 0 {
            let window = &mut self.windows[index];
            window.origin = axis.shifted(window.origin, gap);
            return;
        }
        if removed >= self.windows[index].panes.len() {
            return;
        }
        let tail = self.windows[index].panes.split_off(removed);
        let head = self.windows[index].panes.clone();
        let origin = axis.shifted(self.windows[index].origin, self.extent(&head, axis) + gap);
        if tail.contains(&self.windows[index].active) {
            self.windows[index].active = head[0];
        }
        self.open_window(tail, origin);
    }

    fn reopen_or_open(&mut self, pane: DockKey, origin: Point) -> DockWindowId {
        if let Some(parked) = self.windows.iter_mut().find(|window| window.parked) {
            parked.panes = vec![pane];
            parked.active = pane;
            parked.origin = origin;
            parked.parked = false;
            return parked.id;
        }
        self.open_window(vec![pane], origin)
    }

    fn open_window(&mut self, panes: Vec<DockKey>, origin: Point) -> DockWindowId {
        self.next_id += 1;
        let id = DockWindowId(self.next_id);
        let active = panes[0];
        self.windows.push(DockWindow {
            id,
            panes,
            active,
            origin,
            parked: false,
        });
        id
    }
}

type PaneContent = Rc<RefCell<dyn FnMut()>>;
type HostChrome = Rc<dyn Fn(&DockHost)>;

#[derive(Default)]
struct DockShared {
    contents: HashMap<DockKey, PaneContent>,
    declared: Vec<(DockKey, Size)>,
    states: HashMap<DockWindowId, WindowState>,
    policy: Option<DockPolicy>,
    host: Option<HostChrome>,
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

    fn adopt(&self, policy: DockPolicy, host: HostChrome) {
        self.shared.update(|shared| {
            shared.policy = Some(policy);
            shared.host = Some(host);
        });
    }

    fn declare(&self, mut panes: impl FnMut()) -> Vec<(DockKey, Size)> {
        self.shared.update(|shared| shared.declared.clear());
        {
            let _declaring = Declaring::begin(self.clone());
            panes();
        }
        let declared = self.shared.with(|shared| shared.declared.clone());
        self.shared.update(|shared| {
            shared
                .contents
                .retain(|pane, _| declared.iter().any(|(held, _)| held == pane));
        });
        declared
    }

    fn reconcile(&self, declared: &[(DockKey, Size)], policy: &DockPolicy) {
        let mut next = self.model.get_non_reactive();
        let moved = next.adopt(&self.rects());
        if next.reconcile(declared, policy) || moved {
            self.model.set(next);
        }
    }

    fn sync_states(&self) {
        let model = self.model.get_non_reactive();
        let policy = self.policy();
        let states: Vec<(DockWindowId, WindowState)> = self.shared.with(|shared| {
            shared
                .states
                .iter()
                .map(|(id, state)| (*id, *state))
                .collect()
        });
        for (id, state) in states {
            if let Some(window) = model.window(id) {
                state.set_position(Some(window.origin));
                state.set_size(model.window_size(id, &policy));
            }
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
            trace_dock(format_args!("press ignored: no surface origin"));
            return;
        };
        let screen = plus(origin, local);
        let policy = self.policy();
        let began = self
            .model
            .update(|model| model.press(pane, window, local, screen, &policy));
        trace_dock(format_args!(
            "press pane={pane:?} window={window:?} local=({:.1},{:.1}) origin=({:.1},{:.1}) began={began}",
            local.x, local.y, origin.x, origin.y
        ));
    }

    fn drag_step(&self, window: DockWindowId, local: Point) {
        let drives = self
            .model
            .get_non_reactive()
            .drag()
            .is_some_and(|drag| drag.source == window);
        let Some(origin) = current_native_window_surface_origin() else {
            trace_dock(format_args!("move ignored: no surface origin"));
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
        trace_dock(format_args!(
            "move window={window:?} local=({:.1},{:.1}) origin=({:.1},{:.1}) screen=({:.1},{:.1}) rects={:?} step={step:?}",
            local.x,
            local.y,
            origin.x,
            origin.y,
            screen.x,
            screen.y,
            rects
                .iter()
                .map(|r| (
                    r.window.raw(),
                    r.origin.x,
                    r.origin.y,
                    r.size.width,
                    r.size.height
                ))
                .collect::<Vec<_>>()
        ));
        if step != DockStep::Rest {
            self.sync_states();
        }
    }

    fn release(&self) {
        let released = self.model.update(|model| model.release());
        trace_dock(format_args!("release released={released}"));
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

    /// The size the given pane was declared with.
    pub fn size_of(&self, pane: DockKey) -> Size {
        self.dock.model.get_non_reactive().pane_size(pane)
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

/// Declares one pane of the enclosing [`Dock`], sized by the policy.
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
    let size = dock.policy().window_size;
    declare_pane(&dock, key, size, content);
}

/// Declares one pane of the enclosing [`Dock`] with a size of its own, which
/// a stacking dock adds up to size the window that holds it.
#[allow(non_snake_case)]
pub fn SizedPane(key: DockKey, size: Size, content: impl FnMut() + 'static) {
    let Some(dock) = DECLARING.with(|stack| stack.borrow().last().cloned()) else {
        return;
    };
    declare_pane(&dock, key, size, content);
}

fn declare_pane(dock: &DockRef, key: DockKey, size: Size, content: impl FnMut() + 'static) {
    dock.shared.update(|shared| {
        shared.declared.push((key, size));
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
    dock.reconcile(&declared, &policy);
    dock.sync_states();
    let windows = dock.model.get().windows().to_vec();
    trace_dock(format_args!(
        "windows={}",
        windows
            .iter()
            .filter(|window| !window.parked)
            .map(|window| window.id.raw().to_string())
            .collect::<Vec<_>>()
            .join(",")
    ));
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
    let size = dock
        .model
        .get_non_reactive()
        .window_size(window.id, &policy);
    let state = rememberWindowStateAt(window.origin.x, window.origin.y, size.width, size.height);
    dock.shared.update(|shared| {
        shared.states.insert(window.id, state);
    });
    let id = window.id;
    if let Some(origin) = state.position() {
        let size = state.size();
        trace_dock(format_args!(
            "window id={} origin=({:.1},{:.1}) size=({:.1},{:.1}) panes={} parked={}",
            id.raw(),
            origin.x,
            origin.y,
            size.width,
            size.height,
            window.panes.len(),
            window.parked
        ));
    }
    WindowNode(
        WindowId::from_runtime(dock.id, id.raw()),
        WindowConfig::borderless_for_state(policy.title.clone(), state)
            .with_transparent(true)
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

fn trace_dock(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("CRANPOSE_DOCK_TRACE").is_some() {
        println!("dock trace: {args}");
    }
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
    const PANE: Size = Size {
        width: 200.0,
        height: 100.0,
    };

    fn pane(n: u64) -> DockKey {
        DockKey::from_runtime("pane", n)
    }

    fn tabs() -> DockPolicy {
        DockPolicy::tabs("t", SIZE, 30.0)
    }

    fn stack() -> DockPolicy {
        DockPolicy::stack("s", PANE, DockAxis::Vertical, 10.0)
    }

    fn declared(panes: &[DockKey]) -> Vec<(DockKey, Size)> {
        panes.iter().map(|pane| (*pane, SIZE)).collect()
    }

    fn rect(window: DockWindowId, origin: Point) -> DockRect {
        DockRect {
            window,
            origin,
            size: SIZE,
        }
    }

    fn two_panes_in_one_window() -> (DockModel, DockWindowId) {
        let mut model = DockModel::new();
        model.reconcile(&declared(&[pane(1), pane(2)]), &tabs());
        let window = model.windows()[0].id;
        (model, window)
    }

    fn second_tab_pressed() -> (DockModel, DockWindowId, [DockRect; 1]) {
        let (mut model, window) = two_panes_in_one_window();
        model.press(
            pane(2),
            window,
            Point::new(80.0, 15.0),
            Point::new(180.0, 115.0),
            &tabs(),
        );
        (model, window, [rect(window, ORIGIN)])
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
        assert!(model.reconcile(&declared(&[pane(2)]), &tabs()));
        assert_eq!(model.window_of(pane(1)), None);
        assert!(model.reconcile(&[], &tabs()));
        assert!(model.windows().is_empty());
    }

    #[test]
    fn reconciling_with_nothing_new_changes_nothing() {
        let (mut model, _) = two_panes_in_one_window();
        assert!(!model.reconcile(&declared(&[pane(1), pane(2)]), &tabs()));
    }

    #[test]
    fn a_placement_hint_sends_a_new_pane_to_that_window() {
        let (mut model, first) = two_panes_in_one_window();
        model.press(
            pane(2),
            first,
            Point::new(80.0, 15.0),
            Point::new(180.0, 115.0),
            &tabs(),
        );
        let rects = [rect(first, ORIGIN)];
        model.drag_to(Point::new(180.0, 300.0), &rects, &tabs());
        model.release();
        let second = model.window_of(pane(2)).expect("torn window");
        model.place_next_in(pane(3), second);
        model.reconcile(&declared(&[pane(1), pane(2), pane(3)]), &tabs());
        assert_eq!(model.window_of(pane(3)), Some(second));
    }

    #[test]
    fn pulling_inside_the_strip_does_not_tear() {
        let (mut model, _, rects) = second_tab_pressed();
        let step = model.drag_to(Point::new(260.0, 120.0), &rects, &tabs());
        assert_eq!(step, DockStep::Rest);
        assert_eq!(model.windows().len(), 1);
    }

    #[test]
    fn pulling_out_of_the_strip_tears_into_a_carried_window() {
        let (mut model, window, rects) = second_tab_pressed();
        let step = model.drag_to(Point::new(180.0, 300.0), &rects, &tabs());
        let DockStep::Carry(carried, at) = step else {
            panic!("expected a carry, got {step:?}");
        };
        assert_ne!(carried, window);
        assert_eq!(model.window_of(pane(2)), Some(carried));
        assert_eq!(at, minus(Point::new(180.0, 300.0), tabs().tear_grab));
        assert_eq!(model.window(carried).map(|w| w.origin), Some(at));
        assert_eq!(model.drag().and_then(|d| d.carrying), Some(carried));
    }

    #[test]
    fn a_carried_pane_joins_the_window_whose_strip_it_enters() {
        let (mut model, window, rects) = second_tab_pressed();
        let DockStep::Carry(carried, _) = model.drag_to(Point::new(180.0, 300.0), &rects, &tabs())
        else {
            panic!("expected a carry");
        };
        let rects = [
            rect(window, ORIGIN),
            rect(carried, Point::new(600.0, 600.0)),
        ];
        let step = model.drag_to(Point::new(300.0, 110.0), &rects, &tabs());
        assert_eq!(step, DockStep::Join(window, DockSide::After));
        assert_eq!(model.window_of(pane(2)), Some(window));
        assert!(model.window(carried).is_none());
        assert_eq!(model.drag().and_then(|d| d.carrying), None);
    }

    #[test]
    fn a_lone_pane_carries_its_own_window_and_parks_it_on_joining() {
        let mut model = DockModel::new();
        model.reconcile(&declared(&[pane(1)]), &tabs());
        let first = model.windows()[0].id;
        model.press(
            pane(1),
            first,
            Point::new(80.0, 15.0),
            Point::new(180.0, 115.0),
            &tabs(),
        );
        let rects = [rect(first, ORIGIN)];
        model.drag_to(Point::new(180.0, 300.0), &rects, &tabs());
        model.release();
        let second = model.window_of(pane(1)).expect("lone window");
        assert_eq!(second, first);

        model.reconcile(&declared(&[pane(1), pane(2)]), &tabs());
        let other = model.window_of(pane(2)).expect("second window");
        assert_eq!(other, first);
        model.drag_to(Point::new(0.0, 0.0), &[], &tabs());
        let step = model.press(
            pane(1),
            first,
            Point::new(10.0, 10.0),
            Point::new(110.0, 110.0),
            &tabs(),
        );
        assert!(step);
    }

    #[test]
    fn a_window_the_os_moved_is_adopted_before_the_next_step() {
        let (mut model, window) = two_panes_in_one_window();
        let moved = Point::new(700.0, 50.0);
        assert!(model.adopt(&[rect(window, moved)]));
        assert_eq!(model.window(window).map(|w| w.origin), Some(moved));
        assert!(!model.adopt(&[rect(window, moved)]));
    }

    fn torn_out_then_carried_back() -> (DockModel, DockWindowId, DockWindowId, [DockRect; 2]) {
        let mut model = DockModel::new();
        model.reconcile(&declared(&[pane(1), pane(2)]), &tabs());
        let first = model.windows()[0].id;
        model.press(
            pane(2),
            first,
            Point::new(80.0, 15.0),
            Point::new(180.0, 115.0),
            &tabs(),
        );
        let rects = [rect(first, ORIGIN)];
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
            &tabs(),
        );
        let rects = [rect(first, ORIGIN), rect(second, Point::new(600.0, 600.0))];
        let step = model.drag_to(Point::new(620.0, 700.0), &rects, &tabs());
        assert_eq!(step, DockStep::Carry(second, Point::new(600.0, 685.0)));
        (model, first, second, rects)
    }

    #[test]
    fn a_lone_window_that_joins_another_is_parked_until_release() {
        let (mut model, first, second, rects) = torn_out_then_carried_back();
        let step = model.drag_to(Point::new(300.0, 110.0), &rects, &tabs());
        assert_eq!(step, DockStep::Join(first, DockSide::After));
        assert_eq!(model.window(second).map(|w| w.parked), Some(true));
        assert_eq!(model.windows().len(), 2);
        model.release();
        assert_eq!(model.windows().len(), 1);
    }

    #[test]
    fn tearing_again_reuses_the_parked_source_window() {
        let (mut model, first, second, rects) = torn_out_then_carried_back();
        model.drag_to(Point::new(300.0, 110.0), &rects, &tabs());
        let rects = [rect(first, ORIGIN)];
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
        model.reconcile(&declared(&[pane(1), pane(2), pane(3)]), &tabs());
        let window = model.windows()[0].id;
        model.activate(pane(2));
        model.reconcile(&declared(&[pane(1), pane(3)]), &tabs());
        assert_eq!(model.window(window).map(|w| w.active), Some(pane(3)));
    }

    fn sized(panes: &[(u64, f32)]) -> Vec<(DockKey, Size)> {
        panes
            .iter()
            .map(|(n, height)| (pane(*n), Size::new(200.0, *height)))
            .collect()
    }

    fn three_stacked() -> (DockModel, DockWindowId) {
        let mut model = DockModel::new();
        model.reconcile(&sized(&[(1, 100.0), (2, 100.0), (3, 200.0)]), &stack());
        let window = model.windows()[0].id;
        model.adopt(&[DockRect {
            window,
            origin: ORIGIN,
            size: model.window_size(window, &stack()),
        }]);
        (model, window)
    }

    fn stack_rects(model: &DockModel) -> Vec<DockRect> {
        model
            .windows()
            .iter()
            .map(|window| DockRect {
                window: window.id,
                origin: window.origin,
                size: model.window_size(window.id, &stack()),
            })
            .collect()
    }

    #[test]
    fn a_stacked_window_is_as_large_as_its_panes_together() {
        let (model, window) = three_stacked();
        assert_eq!(model.window_size(window, &stack()), Size::new(200.0, 400.0));
        assert_eq!(model.pane_offset(pane(3), &stack()), Point::new(0.0, 200.0));
    }

    #[test]
    fn tearing_the_first_pane_leaves_the_others_where_they_were() {
        let (mut model, window) = three_stacked();
        model.press(
            pane(1),
            window,
            Point::new(100.0, 10.0),
            Point::new(200.0, 110.0),
            &stack(),
        );
        let step = model.drag_to(Point::new(200.0, 140.0), &stack_rects(&model), &stack());
        let DockStep::Carry(carried, at) = step else {
            panic!("expected a carry, got {step:?}");
        };
        assert_eq!(
            at,
            Point::new(100.0, 130.0),
            "the pane stays under the pointer"
        );
        assert_eq!(model.window_of(pane(1)), Some(carried));
        assert_eq!(
            model.window(window).map(|w| (w.panes.clone(), w.origin)),
            Some((vec![pane(2), pane(3)], Point::new(100.0, 200.0))),
            "the rest of the stack does not jump up into the gap"
        );
    }

    #[test]
    fn tearing_a_middle_pane_splits_the_stack_in_two() {
        let (mut model, window) = three_stacked();
        model.press(
            pane(2),
            window,
            Point::new(100.0, 110.0),
            Point::new(200.0, 210.0),
            &stack(),
        );
        let step = model.drag_to(Point::new(230.0, 210.0), &stack_rects(&model), &stack());
        let DockStep::Carry(carried, at) = step else {
            panic!("expected a carry, got {step:?}");
        };
        assert_eq!(at, Point::new(130.0, 200.0));
        assert_eq!(model.windows().len(), 3);
        assert_eq!(
            model.window(window).map(|w| (w.panes.clone(), w.origin)),
            Some((vec![pane(1)], ORIGIN))
        );
        let tail = model.window_of(pane(3)).expect("a window for the tail");
        assert_ne!(tail, carried);
        assert_eq!(
            model.window(tail).map(|w| w.origin),
            Some(Point::new(100.0, 300.0)),
            "the panes below the gap stay where they were"
        );
    }

    #[test]
    fn closing_a_middle_pane_splits_the_stack_too() {
        let (mut model, window) = three_stacked();
        assert!(model.reconcile(&sized(&[(1, 100.0), (3, 200.0)]), &stack()));
        assert_eq!(
            model.window(window).map(|w| w.panes.clone()),
            Some(vec![pane(1)])
        );
        let tail = model.window_of(pane(3)).expect("a window for the tail");
        assert_eq!(
            model.window(tail).map(|w| w.origin),
            Some(Point::new(100.0, 300.0))
        );
    }

    fn a_pane_alone_beside_a_stack() -> (DockModel, DockWindowId, DockWindowId) {
        let mut model = DockModel::new();
        model.reconcile(&sized(&[(1, 100.0)]), &stack());
        let first = model.windows()[0].id;
        model.place_next_in(pane(2), DockWindowId(u64::MAX));
        model.reconcile(&sized(&[(1, 100.0), (2, 100.0)]), &stack());
        let second = model.windows()[0].id;
        assert_eq!(
            first, second,
            "a missing placement falls back to the first window"
        );
        model.press(
            pane(2),
            first,
            Point::new(100.0, 110.0),
            Point::new(200.0, 210.0),
            &stack(),
        );
        model.adopt(&[rect(first, ORIGIN)]);
        let step = model.drag_to(Point::new(600.0, 610.0), &stack_rects(&model), &stack());
        let DockStep::Carry(carried, _) = step else {
            panic!("expected a carry, got {step:?}");
        };
        (model, first, carried)
    }

    #[test]
    fn a_carried_pane_snaps_under_the_window_it_nears() {
        let (mut model, first, carried) = a_pane_alone_beside_a_stack();
        let step = model.drag_to(Point::new(205.0, 215.0), &stack_rects(&model), &stack());
        assert_eq!(step, DockStep::Join(first, DockSide::After));
        assert_eq!(
            model.window(first).map(|w| (w.panes.clone(), w.origin)),
            Some((vec![pane(1), pane(2)], ORIGIN))
        );
        assert_eq!(model.window_size(first, &stack()), Size::new(200.0, 200.0));
        assert!(
            model.window(carried).is_none(),
            "the carried window was not the one pressed, so nothing routes to it"
        );
    }

    #[test]
    fn a_carried_pane_snaps_above_a_window_which_grows_upward() {
        let (mut model, first, _) = a_pane_alone_beside_a_stack();
        let step = model.drag_to(Point::new(205.0, 5.0), &stack_rects(&model), &stack());
        assert_eq!(step, DockStep::Join(first, DockSide::Before));
        assert_eq!(
            model.window(first).map(|w| (w.panes.clone(), w.origin)),
            Some((vec![pane(2), pane(1)], Point::new(100.0, 0.0))),
            "the window's top moves up by the pane's height so pane 1 stays put"
        );
    }

    #[test]
    fn a_carried_pane_beside_a_window_does_not_snap() {
        let (mut model, first, carried) = a_pane_alone_beside_a_stack();
        let step = model.drag_to(Point::new(450.0, 215.0), &stack_rects(&model), &stack());
        assert_eq!(step, DockStep::Carry(carried, Point::new(350.0, 205.0)));
        assert_eq!(model.window(first).map(|w| w.panes.len()), Some(1));
    }

    #[test]
    fn a_carried_pane_overlapping_a_window_by_a_sliver_does_not_snap() {
        let (mut model, first, carried) = a_pane_alone_beside_a_stack();
        let step = model.drag_to(Point::new(390.0, 215.0), &stack_rects(&model), &stack());
        assert_eq!(
            step,
            DockStep::Carry(carried, Point::new(290.0, 205.0)),
            "joining would drag the pane 190 pixels sideways into the stack"
        );
        assert_eq!(model.window(first).map(|w| w.panes.len()), Some(1));
    }

    #[test]
    fn a_carried_pane_nearly_in_line_with_a_window_still_snaps() {
        let (mut model, first, _) = a_pane_alone_beside_a_stack();
        let step = model.drag_to(Point::new(208.0, 215.0), &stack_rects(&model), &stack());
        assert_eq!(step, DockStep::Join(first, DockSide::After));
    }

    #[test]
    fn a_torn_pane_does_not_snap_back_onto_the_stack_it_left() {
        let (mut model, window) = three_stacked();
        model.press(
            pane(2),
            window,
            Point::new(100.0, 110.0),
            Point::new(200.0, 210.0),
            &stack(),
        );
        let step = model.drag_to(Point::new(200.0, 225.0), &stack_rects(&model), &stack());
        let DockStep::Carry(carried, _) = step else {
            panic!("expected a carry, got {step:?}");
        };
        let step = model.drag_to(Point::new(200.0, 226.0), &stack_rects(&model), &stack());
        assert_eq!(
            step,
            DockStep::Carry(carried, Point::new(100.0, 216.0)),
            "a pane torn off starts edge to edge with the stack and must first get clear of it"
        );
        model.drag_to(Point::new(200.0, 400.0), &stack_rects(&model), &stack());
        let step = model.drag_to(Point::new(200.0, 215.0), &stack_rects(&model), &stack());
        assert_eq!(step, DockStep::Join(window, DockSide::After));
    }

    #[test]
    fn a_pane_torn_off_a_second_time_stays_under_the_pointer() {
        let (mut model, window) = three_stacked();
        model.press(
            pane(3),
            window,
            Point::new(100.0, 210.0),
            Point::new(200.0, 310.0),
            &stack(),
        );
        model.drag_to(Point::new(200.0, 340.0), &stack_rects(&model), &stack());
        model.drag_to(Point::new(200.0, 500.0), &stack_rects(&model), &stack());
        model.drag_to(Point::new(200.0, 315.0), &stack_rects(&model), &stack());
        assert_eq!(model.window_of(pane(3)), Some(window), "snapped back on");
        let step = model.drag_to(Point::new(200.0, 340.0), &stack_rects(&model), &stack());
        assert_eq!(
            step,
            DockStep::Carry(
                model.window_of(pane(3)).expect("carried"),
                Point::new(100.0, 330.0)
            ),
            "the grab is the pointer's place in the pane, whatever window holds it"
        );
    }
}
