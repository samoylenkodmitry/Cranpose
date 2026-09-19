//! Windows a pane can be torn out of and joined back into, the way a
//! browser's tabs and a media player's tool windows do, written over the
//! framework's primitives and nothing else: one [`WindowNode`] per window,
//! [`movable`](cranpose_core::movable) content for each pane so it keeps its
//! state whichever window shows it, a `pointer_input` on each grip and on each
//! window's root for the gesture, and the pointer events' screen positions
//! with the windows' [`WindowState`]s for the geometry. It is what an
//! application writes for its own tabs or tool windows; the framework knows
//! nothing of panes.
//!
//! The drag is one gesture. The window that receives the press keeps
//! receiving every move until release, wherever the pointer goes, because
//! that is how every desktop routes a held button. The handler on that
//! window's root therefore survives the pane leaving it, and does the whole
//! job: it tears the pane into a window of its own, carries that window under
//! the pointer by setting its position, and lets the pane join another window
//! when it reaches that window's drop zone. A window whose last pane joined
//! another window mid-drag stays, hidden, until the drag ends, because it is
//! the window the OS still routes the drag to.
//!
//! Two ways to join. [`JoinRule::Strip`] shows one pane at a time behind a
//! strip, and a pane joins by being dropped on the strip. [`JoinRule::Edge`]
//! shows every pane laid along an axis, sizes the window from its panes, and
//! a pane joins by being carried edge to edge with a window it lines up with;
//! tearing a pane out of the middle of a stack splits the stack around the
//! gap so nothing else moves.
//!
//! Every window here is transparent: a window torn off mid-drag comes up
//! before the desktop has marked it visible, and until it has, nothing can be
//! presented into it. An opaque window would show the desktop's window colour
//! for those frames; a transparent one shows nothing.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use cranpose::{rememberWindowStateAt, WindowConfig, WindowId, WindowNode, WindowState};
use cranpose_core::{key, remember, rememberMutableStateOf, MutableState};
use cranpose_ui::{
    composable, Box, BoxSpec, Modifier, Point, PointerEventKind, PointerInputScope, Size,
};

/// Prints a `demo trace:` line when `CRANPOSE_DEMO_TRACE` is set, and
/// otherwise evaluates none of its arguments.
macro_rules! trace {
    ($($arg:tt)*) => {
        if trace_enabled() {
            print_trace(format_args!($($arg)*));
        }
    };
}

/// One window: the panes it holds in order, and the one in front.
///
/// A window holds at least one pane unless it is parked: a window whose last
/// pane joined another window mid-drag stays, hidden, until the drag ends,
/// because it is the window that received the press and so the one the OS
/// still routes the drag to.
#[derive(Clone, PartialEq, Debug)]
pub struct TornWindow {
    /// Identifier of this window.
    pub id: u64,
    /// The panes this window holds, in the order it shows them.
    pub panes: Vec<u64>,
    /// The pane in front. Meaningless while the window is parked.
    pub active: u64,
    /// Where the window sits, in screen coordinates, as last placed or as the
    /// OS last reported it.
    pub origin: Point,
    /// Whether the window is empty and hidden until the current drag ends.
    pub parked: bool,
}

/// The axis along which a stacking window lays its panes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// Panes stack top to bottom.
    Vertical,
    /// Panes sit left to right.
    Horizontal,
}

impl Axis {
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

/// How a window lays out its panes, and where a carried pane joins.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum JoinRule {
    /// One pane in front at a time, behind a strip along the top. Every
    /// window has the rules' `window_size`; a pane joins a window by being
    /// dropped on its strip.
    Strip {
        /// Height of the strip.
        height: f32,
    },
    /// Every pane shown, laid along `axis` in order; a window is as large as
    /// its panes together. A carried pane joins a window when the window
    /// carrying it lines up with that window across the axis and its edge
    /// comes within `reach` of that window's edge along the axis, going
    /// before it or after it.
    Edge {
        /// The direction panes are laid along.
        axis: Axis,
        /// How close two edges must come to snap.
        reach: f32,
    },
}

/// Which end of a window a joining pane goes to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    /// In front of the window's first pane.
    Before,
    /// After the window's last pane.
    After,
}

/// How the windows host their panes.
#[derive(Clone, PartialEq, Debug)]
pub struct Rules {
    /// The OS title of every window.
    pub title: String,
    /// The size of every window under [`JoinRule::Strip`], and of any pane
    /// that declares no size of its own.
    pub window_size: Size,
    /// How a window lays out its panes, and where a carried pane joins.
    pub join: JoinRule,
    /// How far a pressed pane travels before it tears out of its window.
    pub tear_distance: f32,
    /// Where the first window goes when no window exists yet.
    pub first_origin: Point,
    /// Where the pointer sits inside a window torn off a strip.
    pub tear_grab: Point,
}

impl Rules {
    /// Windows with a tab strip this tall: panes join by being dropped on the
    /// strip.
    pub fn tabs(title: impl Into<String>, window_size: Size, strip_height: f32) -> Self {
        Self {
            title: title.into(),
            window_size,
            join: JoinRule::Strip {
                height: strip_height,
            },
            tear_distance: 24.0,
            first_origin: Point::new(200.0, 160.0),
            tear_grab: Point::new(56.0, strip_height / 2.0),
        }
    }

    /// Windows that stack their panes along `axis` and snap edge to edge: a
    /// carried pane joins a window when their edges come within `reach`.
    /// `pane_size` is the size of a pane that declares none.
    pub fn stack(title: impl Into<String>, pane_size: Size, axis: Axis, reach: f32) -> Self {
        Self {
            title: title.into(),
            window_size: pane_size,
            join: JoinRule::Edge { axis, reach },
            tear_distance: 12.0,
            first_origin: Point::new(200.0, 160.0),
            tear_grab: Point::new(0.0, 0.0),
        }
    }

    fn keeps(&self, screen: Point, anchor: Point, holder: Rect) -> bool {
        match self.join {
            JoinRule::Strip { height } => {
                let y = screen.y - holder.origin.y;
                y >= -self.tear_distance && y <= height + self.tear_distance
            }
            JoinRule::Edge { .. } => distance(screen, anchor) < self.tear_distance,
        }
    }

    fn join_target(&self, carried: Rect, screen: Point, rects: &[Rect]) -> Option<(u64, Side)> {
        let mut others = rects.iter().filter(|rect| rect.window != carried.window);
        match self.join {
            JoinRule::Strip { height } => others
                .filter(|rect| strip_contains(rect, height, screen))
                .map(|rect| (rect.window, Side::After))
                .next(),
            JoinRule::Edge { axis, reach } => others.find_map(|rect| {
                edge_side(axis, reach, carried, *rect).map(|side| (rect.window, side))
            }),
        }
    }
}

fn strip_contains(rect: &Rect, strip_height: f32, point: Point) -> bool {
    let (origin, size) = (rect.origin, rect.size);
    point.x >= origin.x
        && point.x <= origin.x + size.width
        && point.y >= origin.y
        && point.y <= origin.y + strip_height
}

fn whole(point: Point) -> Point {
    Point::new(point.x.round(), point.y.round())
}

fn tear_grab(drag: Drag, alone: bool, rules: &Rules) -> Point {
    match rules.join {
        JoinRule::Strip { .. } if !alone => rules.tear_grab,
        JoinRule::Strip { .. } | JoinRule::Edge { .. } => drag.grab,
    }
}

fn edge_side(axis: Axis, reach: f32, carried: Rect, target: Rect) -> Option<Side> {
    let aligned = (axis.beside(carried.origin) - axis.beside(target.origin)).abs() <= reach;
    if !aligned {
        return None;
    }
    let carried_start = axis.at(carried.origin);
    let carried_end = carried_start + axis.along(carried.size);
    let target_start = axis.at(target.origin);
    let target_end = target_start + axis.along(target.size);
    if (carried_end - target_start).abs() <= reach {
        Some(Side::Before)
    } else if (carried_start - target_end).abs() <= reach {
        Some(Side::After)
    } else {
        None
    }
}

/// Where one window sits on screen.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    /// The window this rectangle belongs to.
    pub window: u64,
    /// Top-left corner, in screen coordinates.
    pub origin: Point,
    /// Content size.
    pub size: Size,
}

/// One pane being dragged, from press to release.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Drag {
    /// The pane under the pointer.
    pub pane: u64,
    /// The window that received the press, and so receives every move.
    pub source: u64,
    /// The pointer's offset inside the pane, which is its offset inside the
    /// window carrying the pane alone.
    pub grab: Point,
    /// Where the pointer was when the pane last settled in a window.
    pub anchor: Point,
    /// The window carried under the pointer, when the pane is loose.
    pub carrying: Option<u64>,
    /// Whether the carried window has been clear of every window since it
    /// was torn off; a pane joins nothing until it has been.
    pub clear: bool,
}

/// What one step of a drag did.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Step {
    /// Nothing moved.
    Rest,
    /// This window is carried, and its origin is now here.
    Carry(u64, Point),
    /// The pane joined this window at this end.
    Join(u64, Side),
}

/// Which panes exist, how large each is, which window holds each, and the
/// drag in flight. Pure data: the composition reads it and the gesture
/// handlers step it.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct TornWindows {
    windows: Vec<TornWindow>,
    sizes: Vec<(u64, Size)>,
    drag: Option<Drag>,
    placements: Vec<(u64, u64)>,
    next_id: u64,
}

impl TornWindows {
    /// No windows and no panes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every window, parked ones included.
    pub fn windows(&self) -> &[TornWindow] {
        &self.windows
    }

    /// The window with the given identifier.
    pub fn window(&self, id: u64) -> Option<&TornWindow> {
        self.windows.iter().find(|window| window.id == id)
    }

    /// The window currently holding the given pane.
    pub fn window_of(&self, pane: u64) -> Option<u64> {
        self.windows
            .iter()
            .find(|window| window.panes.contains(&pane))
            .map(|window| window.id)
    }

    /// The drag in flight, if any.
    pub fn drag(&self) -> Option<Drag> {
        self.drag
    }

    /// The size the given pane was declared with.
    pub fn pane_size(&self, pane: u64) -> Size {
        self.sizes
            .iter()
            .find(|(held, _)| *held == pane)
            .map(|(_, size)| *size)
            .unwrap_or(Size::new(0.0, 0.0))
    }

    /// The content size of the given window under `rules`: the rules' window
    /// size for a strip, the panes laid along the axis for a stack.
    pub fn window_size(&self, window: u64, rules: &Rules) -> Size {
        match rules.join {
            JoinRule::Strip { .. } => rules.window_size,
            JoinRule::Edge { axis, .. } => {
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
    pub fn pane_offset(&self, pane: u64, rules: &Rules) -> Point {
        let JoinRule::Edge { axis, .. } = rules.join else {
            return Point::new(0.0, 0.0);
        };
        let before: Vec<u64> = self
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
    pub fn adopt(&mut self, rects: &[Rect]) -> bool {
        let mut moved = false;
        for rect in rects {
            let Some(window) = self
                .windows
                .iter_mut()
                .find(|window| window.id == rect.window)
            else {
                continue;
            };
            if window.origin != rect.origin {
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
    /// first time go where [`TornWindows::place_next_in`] asked, else into the
    /// first window, else into a new window at the rules' first origin.
    /// Returns whether anything changed.
    pub fn reconcile(&mut self, declared: &[(u64, Size)], rules: &Rules) -> bool {
        let gone: Vec<u64> = self
            .windows
            .iter()
            .flat_map(|window| window.panes.iter().copied())
            .filter(|pane| !declared.iter().any(|(held, _)| held == pane))
            .collect();
        for pane in &gone {
            self.detach(*pane, rules);
        }
        let mut changed = !gone.is_empty() || self.sizes != declared;
        self.sizes = declared.to_vec();
        for (pane, _) in declared {
            if self.window_of(*pane).is_none() {
                self.place(*pane, rules);
                changed = true;
            }
        }
        changed
    }

    /// Asks that `pane`, when it is next declared, open in `window`.
    pub fn place_next_in(&mut self, pane: u64, window: u64) {
        self.placements.push((pane, window));
    }

    /// Brings the given pane to the front of its window.
    pub fn activate(&mut self, pane: u64) {
        if let Some(window) = self
            .windows
            .iter_mut()
            .find(|window| window.panes.contains(&pane))
        {
            window.active = pane;
        }
    }

    /// Starts dragging `pane` from the window holding it, where `local` is
    /// the pointer inside that window and `screen` the pointer on screen.
    /// Returns whether a drag began.
    pub fn press(&mut self, pane: u64, local: Point, screen: Point, rules: &Rules) -> bool {
        let Some(source) = self.window_of(pane) else {
            return false;
        };
        if self.drag.is_some() {
            return false;
        }
        self.activate(pane);
        self.drag = Some(Drag {
            pane,
            source,
            grab: minus(local, self.pane_offset(pane, rules)),
            anchor: screen,
            carrying: None,
            clear: false,
        });
        true
    }

    /// Moves the drag in flight to `screen`, given where every open window
    /// sits.
    pub fn drag_to(&mut self, screen: Point, rects: &[Rect], rules: &Rules) -> Step {
        self.adopt(rects);
        let Some(drag) = self.drag else {
            return Step::Rest;
        };
        match drag.carrying {
            None => self.tear_if_pulled(drag, screen, rects, rules),
            Some(carried) => self.carry(drag, carried, screen, rects, rules),
        }
    }

    /// Ends the drag in flight, closing any window parked by it.
    pub fn release(&mut self) -> bool {
        let released = self.drag.take().is_some();
        self.windows.retain(|window| !window.parked);
        released
    }

    fn extent(&self, panes: &[u64], axis: Axis) -> f32 {
        panes
            .iter()
            .map(|pane| axis.along(self.pane_size(*pane)))
            .sum()
    }

    fn tear_if_pulled(&mut self, drag: Drag, screen: Point, rects: &[Rect], rules: &Rules) -> Step {
        let Some(holder) = self.window_of(drag.pane) else {
            return Step::Rest;
        };
        let Some(rect) = rects.iter().copied().find(|rect| rect.window == holder) else {
            return Step::Rest;
        };
        if rules.keeps(screen, drag.anchor, rect) {
            return Step::Rest;
        }
        let alone = self
            .window(holder)
            .is_some_and(|window| window.panes.len() == 1);
        let grab = tear_grab(drag, alone, rules);
        let origin = whole(minus(screen, grab));
        let carried = if alone {
            self.set_origin(holder, origin);
            holder
        } else {
            self.detach(drag.pane, rules);
            self.reopen_or_open(drag.pane, origin)
        };
        let rect = Rect {
            window: carried,
            origin,
            size: self.window_size(carried, rules),
        };
        let clear = rules
            .join_target(rect, screen, &self.placed_rects(rules))
            .is_none();
        self.drag = Some(Drag {
            grab,
            carrying: Some(carried),
            clear,
            ..drag
        });
        Step::Carry(carried, origin)
    }

    fn placed_rects(&self, rules: &Rules) -> Vec<Rect> {
        self.windows
            .iter()
            .filter(|window| !window.parked)
            .map(|window| Rect {
                window: window.id,
                origin: window.origin,
                size: self.window_size(window.id, rules),
            })
            .collect()
    }

    fn carry(
        &mut self,
        drag: Drag,
        carried: u64,
        screen: Point,
        rects: &[Rect],
        rules: &Rules,
    ) -> Step {
        let origin = whole(minus(screen, drag.grab));
        let rect = Rect {
            window: carried,
            origin,
            size: self.window_size(carried, rules),
        };
        let target = rules.join_target(rect, screen, rects);
        let Some((target, side)) = target.filter(|_| drag.clear) else {
            self.set_origin(carried, origin);
            self.drag = Some(Drag {
                clear: drag.clear || target.is_none(),
                ..drag
            });
            return Step::Carry(carried, origin);
        };
        self.detach(drag.pane, rules);
        self.attach(drag.pane, target, side, rules);
        self.drag = Some(Drag {
            anchor: screen,
            carrying: None,
            ..drag
        });
        Step::Join(target, side)
    }

    fn set_origin(&mut self, window: u64, origin: Point) {
        if let Some(found) = self.windows.iter_mut().find(|held| held.id == window) {
            found.origin = origin;
        }
    }

    fn place(&mut self, pane: u64, rules: &Rules) {
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
            Some(id) => self.attach(pane, id, Side::After, rules),
            None => {
                self.open_window(vec![pane], rules.first_origin);
            }
        }
    }

    fn take_placement(&mut self, pane: u64) -> Option<u64> {
        let index = self.placements.iter().position(|(held, _)| *held == pane)?;
        Some(self.placements.remove(index).1)
    }

    fn attach(&mut self, pane: u64, window: u64, side: Side, rules: &Rules) {
        let pull = match (rules.join, side) {
            (JoinRule::Edge { axis, .. }, Side::Before) => {
                Some((axis, -axis.along(self.pane_size(pane))))
            }
            _ => None,
        };
        let Some(found) = self.windows.iter_mut().find(|held| held.id == window) else {
            return;
        };
        match side {
            Side::Before => found.panes.insert(0, pane),
            Side::After => found.panes.push(pane),
        }
        if let Some((axis, by)) = pull {
            found.origin = axis.shifted(found.origin, by);
        }
        found.active = pane;
        found.parked = false;
    }

    fn detach(&mut self, pane: u64, rules: &Rules) {
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
        if let JoinRule::Edge { axis, .. } = rules.join {
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

    fn keep_neighbours_in_place(&mut self, index: usize, removed: usize, pane: u64, axis: Axis) {
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

    fn reopen_or_open(&mut self, pane: u64, origin: Point) -> u64 {
        if let Some(parked) = self.windows.iter_mut().find(|window| window.parked) {
            parked.panes = vec![pane];
            parked.active = pane;
            parked.origin = origin;
            parked.parked = false;
            return parked.id;
        }
        self.open_window(vec![pane], origin)
    }

    fn open_window(&mut self, panes: Vec<u64>, origin: Point) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        let active = panes[0];
        self.windows.push(TornWindow {
            id,
            panes,
            active,
            origin,
            parked: false,
        });
        id
    }
}

type Chrome = Rc<dyn Fn(&WindowView)>;

struct Shared {
    rules: Rules,
    states: HashMap<u64, WindowState>,
    chrome: Option<Chrome>,
}

/// A handle to the windows, cheap to clone and valid inside every one of
/// them: what a grip presses, what a window root drags, what a strip asks.
#[derive(Clone)]
pub struct Windows {
    id: &'static str,
    model: MutableState<TornWindows>,
    shared: Rc<RefCell<Shared>>,
}

impl PartialEq for Windows {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.model.runtime_state_id() == other.model.runtime_state_id()
    }
}

impl Windows {
    fn new(id: &'static str, model: MutableState<TornWindows>, rules: Rules) -> Self {
        Self {
            id,
            model,
            shared: Rc::new(RefCell::new(Shared {
                rules,
                states: HashMap::new(),
                chrome: None,
            })),
        }
    }

    /// The arrangement, read reactively.
    pub fn model(&self) -> MutableState<TornWindows> {
        self.model
    }

    /// Brings the given pane to the front of its window.
    pub fn activate(&self, pane: u64) {
        self.model.update(|model| model.activate(pane));
    }

    /// The size the given pane was declared with.
    pub fn size_of(&self, pane: u64) -> Size {
        self.model.get_non_reactive().pane_size(pane)
    }

    /// Makes the component the grip by which its pane is dragged between
    /// windows.
    ///
    /// Pressing it brings the pane to the front. Pulling it out of the
    /// window's drop zone tears the pane into a window of its own that follows
    /// the pointer, and carrying it into another window's drop zone makes the
    /// pane join that window, all in one gesture. The grip may sit inside the
    /// pane's movable content: it names only the pane, and the window holding
    /// the pane is looked up at the press.
    pub fn grip(&self, modifier: Modifier, pane: u64) -> Modifier {
        let windows = self.clone();
        modifier.pointer_input(pane, move |input: PointerInputScope| {
            let windows = windows.clone();
            async move {
                input
                    .await_pointer_event_scope(|await_scope| async move {
                        loop {
                            let event = await_scope.await_pointer_event().await;
                            if event.kind == PointerEventKind::Down {
                                windows.press(pane, event.global_position, event.screen_position);
                                event.consume();
                            }
                        }
                    })
                    .await;
            }
        })
    }

    fn rules(&self) -> Rules {
        self.shared.borrow().rules.clone()
    }

    fn adopt(&self, rules: Rules, chrome: Chrome) {
        let mut shared = self.shared.borrow_mut();
        shared.rules = rules;
        shared.chrome = Some(chrome);
    }

    fn chrome(&self) -> Option<Chrome> {
        self.shared.borrow().chrome.clone()
    }

    fn reconcile(&self, declared: &[(u64, Size)]) {
        let rules = self.rules();
        let mut next = self.model.get_non_reactive();
        let moved = next.adopt(&self.rects());
        if next.reconcile(declared, &rules) || moved {
            self.model.set(next);
        }
    }

    fn remember_state(&self, window: u64, state: WindowState) {
        self.shared.borrow_mut().states.insert(window, state);
    }

    fn sync_states(&self) {
        let model = self.model.get_non_reactive();
        let rules = self.rules();
        let mut shared = self.shared.borrow_mut();
        shared.states.retain(|id, _| model.window(*id).is_some());
        for (id, state) in &shared.states {
            if let Some(window) = model.window(*id) {
                state.set_position(Some(window.origin));
                state.set_size(model.window_size(*id, &rules));
            }
        }
    }

    fn rects(&self) -> Vec<Rect> {
        let shared = self.shared.borrow();
        self.model.read(|model| {
            model
                .windows()
                .iter()
                .filter(|window| !window.parked)
                .filter_map(|window| {
                    let state = shared.states.get(&window.id)?;
                    Some(Rect {
                        window: window.id,
                        origin: state.position_non_reactive()?,
                        size: state.size_non_reactive(),
                    })
                })
                .collect()
        })
    }

    fn press(&self, pane: u64, local: Point, screen: Option<Point>) {
        let Some(screen) = screen else {
            trace!("press ignored: no screen position");
            return;
        };
        let rules = self.rules();
        let began = self
            .model
            .update(|model| model.press(pane, local, screen, &rules));
        trace!(
            "press pane={pane} local=({:.1},{:.1}) screen=({:.1},{:.1}) began={began}",
            local.x,
            local.y,
            screen.x,
            screen.y
        );
    }

    fn drag_step(&self, window: u64, local: Point, screen: Option<Point>) {
        let drives = self
            .model
            .read(|model| model.drag().is_some_and(|drag| drag.source == window));
        if !drives {
            return;
        }
        let Some(screen) = screen else {
            trace!("move ignored: no screen position");
            return;
        };
        let rules = self.rules();
        let rects = self.rects();
        let step = self
            .model
            .update(|model| model.drag_to(screen, &rects, &rules));
        trace!(
            "move window={window} local=({:.1},{:.1}) screen=({:.1},{:.1}) rects={:?} step={step:?}",
            local.x,
            local.y,
            screen.x,
            screen.y,
            rects
                .iter()
                .map(|r| (r.window, r.origin.x, r.origin.y, r.size.width, r.size.height))
                .collect::<Vec<_>>()
        );
        if step != Step::Rest {
            self.sync_states();
        }
    }

    fn release(&self) {
        let released = self.model.update(|model| model.release());
        trace!("release released={released}");
    }

    fn drag_session(&self, base: Modifier, window: u64) -> Modifier {
        let windows = self.clone();
        base.pointer_input(window, move |scope: PointerInputScope| {
            let windows = windows.clone();
            async move {
                scope
                    .await_pointer_event_scope(|await_scope| async move {
                        loop {
                            let event = await_scope.await_pointer_event().await;
                            match event.kind {
                                PointerEventKind::Move => windows.drag_step(
                                    window,
                                    event.global_position,
                                    event.screen_position,
                                ),
                                PointerEventKind::Up | PointerEventKind::Cancel => {
                                    windows.release()
                                }
                                _ => {}
                            }
                        }
                    })
                    .await;
            }
        })
    }
}

/// One window as seen by the code that draws its chrome.
#[derive(Clone)]
pub struct WindowView {
    windows: Windows,
    window: TornWindow,
}

impl PartialEq for WindowView {
    fn eq(&self, other: &Self) -> bool {
        self.windows == other.windows && self.window == other.window
    }
}

impl WindowView {
    /// The handle shared by every window.
    pub fn windows(&self) -> &Windows {
        &self.windows
    }

    /// This window's identifier.
    pub fn id(&self) -> u64 {
        self.window.id
    }

    /// The panes this window holds, in order.
    pub fn panes(&self) -> &[u64] {
        &self.window.panes
    }

    /// The pane in front.
    pub fn active(&self) -> u64 {
        self.window.active
    }

    /// Asks that `pane`, when the application next declares it, open here.
    pub fn open_next_here(&self, pane: u64) {
        let window = self.window.id;
        self.windows
            .model
            .update(|model| model.place_next_in(pane, window));
    }
}

/// Hosts `panes` in as many OS windows as the user has torn them into.
///
/// `panes` names each pane with its size; naming a pane for the first time
/// opens it, no longer naming it closes it. `chrome` draws one window,
/// composing each pane's content under `movable` wherever it goes and
/// marking grips with [`Windows::grip`]. Which window holds what, tearing,
/// carrying, joining, opening and closing windows are this host's.
#[composable]
#[allow(non_snake_case)]
pub fn TornWindowsHost(
    id: &'static str,
    rules: Rules,
    panes: Vec<(u64, Size)>,
    chrome: impl Fn(&WindowView) + 'static,
) {
    let model = rememberMutableStateOf(TornWindows::new);
    let windows = {
        let rules = rules.clone();
        remember(move || Windows::new(id, model, rules)).with(Windows::clone)
    };
    windows.adopt(rules, Rc::new(chrome));
    windows.reconcile(&panes);
    windows.sync_states();
    let snapshot = model.get().windows().to_vec();
    trace!(
        "windows={}",
        snapshot
            .iter()
            .filter(|window| !window.parked)
            .map(|window| window.id.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    for window in snapshot {
        let windows = windows.clone();
        key(window.id, move || TornWindowNode(windows, window));
    }
}

#[composable]
#[allow(non_snake_case)]
fn TornWindowNode(windows: Windows, window: TornWindow) {
    let rules = windows.rules();
    let size = windows
        .model
        .get_non_reactive()
        .window_size(window.id, &rules);
    let state = rememberWindowStateAt(window.origin.x, window.origin.y, size.width, size.height);
    windows.remember_state(window.id, state);
    let id = window.id;
    if let Some(origin) = state.position() {
        let size = state.size();
        trace!(
            "window id={id} origin=({:.1},{:.1}) size=({:.1},{:.1}) panes={} parked={} presented={}",
            origin.x,
            origin.y,
            size.width,
            size.height,
            window.panes.len(),
            window.parked,
            state.presented_non_reactive()
        );
    }
    let parked = window.parked;
    let view = WindowView { windows, window };
    WindowNode(
        WindowId::from_runtime(view.windows.id, id),
        WindowConfig::borderless_for_state(rules.title, state)
            .with_transparent(true)
            .with_visible(!parked),
        move || {
            let view = view.clone();
            let chrome = view.windows.chrome();
            Box(
                view.windows
                    .drag_session(Modifier::empty().fill_max_size(), id),
                BoxSpec::default(),
                move || {
                    if let Some(chrome) = &chrome {
                        chrome(&view);
                    }
                },
            );
        },
    );
}

fn trace_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_DEMO_TRACE").is_some())
}

fn print_trace(args: std::fmt::Arguments<'_>) {
    println!("demo trace: {args}");
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

    fn tabs() -> Rules {
        Rules::tabs("t", SIZE, 30.0)
    }

    fn stack() -> Rules {
        Rules::stack("s", PANE, Axis::Vertical, 10.0)
    }

    fn declared(panes: &[u64]) -> Vec<(u64, Size)> {
        panes.iter().map(|pane| (*pane, SIZE)).collect()
    }

    fn rect(window: u64, origin: Point) -> Rect {
        Rect {
            window,
            origin,
            size: SIZE,
        }
    }

    fn two_panes_in_one_window() -> (TornWindows, u64) {
        let mut model = TornWindows::new();
        model.reconcile(&declared(&[1, 2]), &tabs());
        let window = model.windows()[0].id;
        (model, window)
    }

    fn second_tab_pressed() -> (TornWindows, u64, [Rect; 1]) {
        let (mut model, window) = two_panes_in_one_window();
        model.press(2, Point::new(80.0, 15.0), Point::new(180.0, 115.0), &tabs());
        (model, window, [rect(window, ORIGIN)])
    }

    #[test]
    fn declared_panes_open_in_one_window() {
        let (model, window) = two_panes_in_one_window();
        assert_eq!(model.windows().len(), 1);
        assert_eq!(
            model.window(window).map(|w| w.panes.clone()),
            Some(vec![1, 2])
        );
    }

    #[test]
    fn an_undeclared_pane_leaves_and_an_empty_window_closes() {
        let (mut model, _) = two_panes_in_one_window();
        assert!(model.reconcile(&declared(&[2]), &tabs()));
        assert_eq!(model.window_of(1), None);
        assert!(model.reconcile(&[], &tabs()));
        assert!(model.windows().is_empty());
    }

    #[test]
    fn reconciling_with_nothing_new_changes_nothing() {
        let (mut model, _) = two_panes_in_one_window();
        assert!(!model.reconcile(&declared(&[1, 2]), &tabs()));
    }

    #[test]
    fn a_placement_hint_sends_a_new_pane_to_that_window() {
        let (mut model, first) = two_panes_in_one_window();
        model.press(2, Point::new(80.0, 15.0), Point::new(180.0, 115.0), &tabs());
        let rects = [rect(first, ORIGIN)];
        model.drag_to(Point::new(180.0, 300.0), &rects, &tabs());
        model.release();
        let second = model.window_of(2).expect("torn window");
        model.place_next_in(3, second);
        model.reconcile(&declared(&[1, 2, 3]), &tabs());
        assert_eq!(model.window_of(3), Some(second));
    }

    #[test]
    fn a_press_on_a_pane_no_window_holds_begins_nothing() {
        let (mut model, _) = two_panes_in_one_window();
        assert!(!model.press(9, Point::new(80.0, 15.0), Point::new(180.0, 115.0), &tabs()));
        assert_eq!(model.drag(), None);
    }

    #[test]
    fn pulling_inside_the_strip_does_not_tear() {
        let (mut model, _, rects) = second_tab_pressed();
        let step = model.drag_to(Point::new(260.0, 120.0), &rects, &tabs());
        assert_eq!(step, Step::Rest);
        assert_eq!(model.windows().len(), 1);
    }

    #[test]
    fn pulling_out_of_the_strip_tears_into_a_carried_window() {
        let (mut model, window, rects) = second_tab_pressed();
        let step = model.drag_to(Point::new(180.0, 300.0), &rects, &tabs());
        let Step::Carry(carried, at) = step else {
            panic!("expected a carry, got {step:?}");
        };
        assert_ne!(carried, window);
        assert_eq!(model.window_of(2), Some(carried));
        assert_eq!(at, minus(Point::new(180.0, 300.0), tabs().tear_grab));
        assert_eq!(model.window(carried).map(|w| w.origin), Some(at));
        assert_eq!(model.drag().and_then(|d| d.carrying), Some(carried));
    }

    #[test]
    fn a_carried_pane_joins_the_window_whose_strip_it_enters() {
        let (mut model, window, rects) = second_tab_pressed();
        let Step::Carry(carried, _) = model.drag_to(Point::new(180.0, 300.0), &rects, &tabs())
        else {
            panic!("expected a carry");
        };
        let rects = [
            rect(window, ORIGIN),
            rect(carried, Point::new(600.0, 600.0)),
        ];
        let step = model.drag_to(Point::new(300.0, 110.0), &rects, &tabs());
        assert_eq!(step, Step::Join(window, Side::After));
        assert_eq!(model.window_of(2), Some(window));
        assert!(model.window(carried).is_none());
        assert_eq!(model.drag().and_then(|d| d.carrying), None);
    }

    #[test]
    fn a_lone_pane_carries_its_own_window_and_parks_it_on_joining() {
        let mut model = TornWindows::new();
        model.reconcile(&declared(&[1]), &tabs());
        let first = model.windows()[0].id;
        model.press(1, Point::new(80.0, 15.0), Point::new(180.0, 115.0), &tabs());
        let rects = [rect(first, ORIGIN)];
        model.drag_to(Point::new(180.0, 300.0), &rects, &tabs());
        model.release();
        let second = model.window_of(1).expect("lone window");
        assert_eq!(second, first);

        model.reconcile(&declared(&[1, 2]), &tabs());
        let other = model.window_of(2).expect("second window");
        assert_eq!(other, first);
        model.drag_to(Point::new(0.0, 0.0), &[], &tabs());
        let step = model.press(1, Point::new(10.0, 10.0), Point::new(110.0, 110.0), &tabs());
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

    fn torn_out_then_carried_back() -> (TornWindows, u64, u64, [Rect; 2]) {
        let mut model = TornWindows::new();
        model.reconcile(&declared(&[1, 2]), &tabs());
        let first = model.windows()[0].id;
        model.press(2, Point::new(80.0, 15.0), Point::new(180.0, 115.0), &tabs());
        let rects = [rect(first, ORIGIN)];
        let Step::Carry(second, _) = model.drag_to(Point::new(180.0, 300.0), &rects, &tabs())
        else {
            panic!("expected a carry");
        };
        model.release();

        model.press(2, Point::new(20.0, 15.0), Point::new(620.0, 615.0), &tabs());
        let rects = [rect(first, ORIGIN), rect(second, Point::new(600.0, 600.0))];
        let step = model.drag_to(Point::new(620.0, 700.0), &rects, &tabs());
        assert_eq!(step, Step::Carry(second, Point::new(600.0, 685.0)));
        (model, first, second, rects)
    }

    #[test]
    fn a_lone_window_that_joins_another_is_parked_until_release() {
        let (mut model, first, second, rects) = torn_out_then_carried_back();
        let step = model.drag_to(Point::new(300.0, 110.0), &rects, &tabs());
        assert_eq!(step, Step::Join(first, Side::After));
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
        let Step::Carry(reused, _) = step else {
            panic!("expected a carry, got {step:?}");
        };
        assert_eq!(reused, second);
        assert_eq!(model.window(second).map(|w| w.parked), Some(false));
    }

    #[test]
    fn closing_the_shown_pane_shows_its_neighbour() {
        let mut model = TornWindows::new();
        model.reconcile(&declared(&[1, 2, 3]), &tabs());
        let window = model.windows()[0].id;
        model.activate(2);
        model.reconcile(&declared(&[1, 3]), &tabs());
        assert_eq!(model.window(window).map(|w| w.active), Some(3));
    }

    fn sized(panes: &[(u64, f32)]) -> Vec<(u64, Size)> {
        panes
            .iter()
            .map(|(n, height)| (*n, Size::new(200.0, *height)))
            .collect()
    }

    fn three_stacked() -> (TornWindows, u64) {
        let mut model = TornWindows::new();
        model.reconcile(&sized(&[(1, 100.0), (2, 100.0), (3, 200.0)]), &stack());
        let window = model.windows()[0].id;
        model.adopt(&[Rect {
            window,
            origin: ORIGIN,
            size: model.window_size(window, &stack()),
        }]);
        (model, window)
    }

    fn stack_rects(model: &TornWindows) -> Vec<Rect> {
        model
            .windows()
            .iter()
            .map(|window| Rect {
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
        assert_eq!(model.pane_offset(3, &stack()), Point::new(0.0, 200.0));
    }

    #[test]
    fn tearing_the_first_pane_leaves_the_others_where_they_were() {
        let (mut model, window) = three_stacked();
        model.press(
            1,
            Point::new(100.0, 10.0),
            Point::new(200.0, 110.0),
            &stack(),
        );
        let step = model.drag_to(Point::new(200.0, 140.0), &stack_rects(&model), &stack());
        let Step::Carry(carried, at) = step else {
            panic!("expected a carry, got {step:?}");
        };
        assert_eq!(
            at,
            Point::new(100.0, 130.0),
            "the pane stays under the pointer"
        );
        assert_eq!(model.window_of(1), Some(carried));
        assert_eq!(
            model.window(window).map(|w| (w.panes.clone(), w.origin)),
            Some((vec![2, 3], Point::new(100.0, 200.0))),
            "the rest of the stack does not jump up into the gap"
        );
    }

    #[test]
    fn tearing_a_middle_pane_splits_the_stack_in_two() {
        let (mut model, window) = three_stacked();
        model.press(
            2,
            Point::new(100.0, 110.0),
            Point::new(200.0, 210.0),
            &stack(),
        );
        let step = model.drag_to(Point::new(230.0, 210.0), &stack_rects(&model), &stack());
        let Step::Carry(carried, at) = step else {
            panic!("expected a carry, got {step:?}");
        };
        assert_eq!(at, Point::new(130.0, 200.0));
        assert_eq!(model.windows().len(), 3);
        assert_eq!(
            model.window(window).map(|w| (w.panes.clone(), w.origin)),
            Some((vec![1], ORIGIN))
        );
        let tail = model.window_of(3).expect("a window for the tail");
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
        assert_eq!(model.window(window).map(|w| w.panes.clone()), Some(vec![1]));
        let tail = model.window_of(3).expect("a window for the tail");
        assert_eq!(
            model.window(tail).map(|w| w.origin),
            Some(Point::new(100.0, 300.0))
        );
    }

    fn a_pane_alone_beside_a_stack() -> (TornWindows, u64, u64) {
        let mut model = TornWindows::new();
        model.reconcile(&sized(&[(1, 100.0)]), &stack());
        let first = model.windows()[0].id;
        model.place_next_in(2, u64::MAX);
        model.reconcile(&sized(&[(1, 100.0), (2, 100.0)]), &stack());
        let second = model.windows()[0].id;
        assert_eq!(
            first, second,
            "a missing placement falls back to the first window"
        );
        model.press(
            2,
            Point::new(100.0, 110.0),
            Point::new(200.0, 210.0),
            &stack(),
        );
        model.adopt(&[rect(first, ORIGIN)]);
        let step = model.drag_to(Point::new(600.0, 610.0), &stack_rects(&model), &stack());
        let Step::Carry(carried, _) = step else {
            panic!("expected a carry, got {step:?}");
        };
        (model, first, carried)
    }

    #[test]
    fn a_carried_pane_snaps_under_the_window_it_nears() {
        let (mut model, first, carried) = a_pane_alone_beside_a_stack();
        let step = model.drag_to(Point::new(205.0, 215.0), &stack_rects(&model), &stack());
        assert_eq!(step, Step::Join(first, Side::After));
        assert_eq!(
            model.window(first).map(|w| (w.panes.clone(), w.origin)),
            Some((vec![1, 2], ORIGIN))
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
        assert_eq!(step, Step::Join(first, Side::Before));
        assert_eq!(
            model.window(first).map(|w| (w.panes.clone(), w.origin)),
            Some((vec![2, 1], Point::new(100.0, 0.0))),
            "the window's top moves up by the pane's height so pane 1 stays put"
        );
    }

    #[test]
    fn a_carried_pane_beside_a_window_does_not_snap() {
        let (mut model, first, carried) = a_pane_alone_beside_a_stack();
        let step = model.drag_to(Point::new(450.0, 215.0), &stack_rects(&model), &stack());
        assert_eq!(step, Step::Carry(carried, Point::new(350.0, 205.0)));
        assert_eq!(model.window(first).map(|w| w.panes.len()), Some(1));
    }

    #[test]
    fn a_carried_pane_overlapping_a_window_by_a_sliver_does_not_snap() {
        let (mut model, first, carried) = a_pane_alone_beside_a_stack();
        let step = model.drag_to(Point::new(390.0, 215.0), &stack_rects(&model), &stack());
        assert_eq!(
            step,
            Step::Carry(carried, Point::new(290.0, 205.0)),
            "joining would drag the pane 190 pixels sideways into the stack"
        );
        assert_eq!(model.window(first).map(|w| w.panes.len()), Some(1));
    }

    #[test]
    fn a_carried_pane_nearly_in_line_with_a_window_still_snaps() {
        let (mut model, first, _) = a_pane_alone_beside_a_stack();
        let step = model.drag_to(Point::new(208.0, 215.0), &stack_rects(&model), &stack());
        assert_eq!(step, Step::Join(first, Side::After));
    }

    #[test]
    fn a_torn_pane_does_not_snap_back_onto_the_stack_it_left() {
        let (mut model, window) = three_stacked();
        model.press(
            2,
            Point::new(100.0, 110.0),
            Point::new(200.0, 210.0),
            &stack(),
        );
        let step = model.drag_to(Point::new(200.0, 225.0), &stack_rects(&model), &stack());
        let Step::Carry(carried, _) = step else {
            panic!("expected a carry, got {step:?}");
        };
        let step = model.drag_to(Point::new(200.0, 226.0), &stack_rects(&model), &stack());
        assert_eq!(
            step,
            Step::Carry(carried, Point::new(100.0, 216.0)),
            "a pane torn off starts edge to edge with the stack and must first get clear of it"
        );
        model.drag_to(Point::new(200.0, 400.0), &stack_rects(&model), &stack());
        let step = model.drag_to(Point::new(200.0, 215.0), &stack_rects(&model), &stack());
        assert_eq!(step, Step::Join(window, Side::After));
    }

    #[test]
    fn a_pane_torn_off_a_second_time_stays_under_the_pointer() {
        let (mut model, window) = three_stacked();
        model.press(
            3,
            Point::new(100.0, 210.0),
            Point::new(200.0, 310.0),
            &stack(),
        );
        model.drag_to(Point::new(200.0, 340.0), &stack_rects(&model), &stack());
        model.drag_to(Point::new(200.0, 500.0), &stack_rects(&model), &stack());
        model.drag_to(Point::new(200.0, 315.0), &stack_rects(&model), &stack());
        assert_eq!(model.window_of(3), Some(window), "snapped back on");
        let step = model.drag_to(Point::new(200.0, 340.0), &stack_rects(&model), &stack());
        assert_eq!(
            step,
            Step::Carry(
                model.window_of(3).expect("carried"),
                Point::new(100.0, 330.0)
            ),
            "the grab is the pointer's place in the pane, whatever window holds it"
        );
    }
}

#[cfg(test)]
mod composition_tests {
    use std::cell::RefCell;

    use cranpose_app_shell::{AppShell, RootId};
    use cranpose_core::{location_key, movable};
    use cranpose_testing::TestRenderer;
    use cranpose_ui::{Box, BoxSpec, LayoutBox, Modifier, Size};

    use super::*;

    const STRIP: Size = Size {
        width: 80.0,
        height: 36.0,
    };
    const BODY: Size = Size {
        width: 90.0,
        height: 30.0,
    };
    const WINDOW: Size = Size {
        width: 200.0,
        height: 100.0,
    };

    #[composable]
    #[allow(non_snake_case)]
    fn PageBody(page: u64) {
        let _ = page;
        Box(Modifier::empty().size(BODY), BoxSpec::default(), || {});
    }

    /// A strip and the active page, behind a composable that skips while the
    /// window around it recomposes.
    #[composable]
    #[allow(non_snake_case)]
    fn TestChrome(view: WindowView) {
        let active = view.active();
        cranpose_ui::Column(
            Modifier::empty(),
            cranpose_ui::ColumnSpec::default(),
            move || {
                Box(Modifier::empty().size(STRIP), BoxSpec::default(), || {});
                movable(("page", active), move || PageBody(active));
            },
        );
    }

    /// Where a box lies in its window, when it is there at all.
    type Placement = Option<(f32, f32)>;

    fn find_box_sized(layout: &LayoutBox, size: Size) -> Placement {
        if layout.rect.width == size.width && layout.rect.height == size.height {
            return Some((layout.rect.x, layout.rect.y));
        }
        layout
            .children
            .iter()
            .find_map(|child| find_box_sized(child, size))
    }

    fn window_root_id(window: u64) -> u64 {
        WindowId::from_runtime("test-tabs", window).raw()
    }

    /// Where the strip and the body lie in `window`'s layout, after the
    /// shell has settled.
    fn strip_and_body_in(
        shell: &mut AppShell<TestRenderer>,
        window: u64,
    ) -> (Placement, Placement) {
        shell.update();
        shell.update();
        let mut surface = shell
            .surface(RootId::Window(window_root_id(window)))
            .expect("window surface");
        surface.with_layout_tree(|tree| {
            let root = tree.expect("window layout").root();
            (find_box_sized(root, STRIP), find_box_sized(root, BODY))
        })
    }

    /// The tabs demo in miniature, driven through the handle instead of the
    /// pointer: a strip and a movable page per window, the first tab torn
    /// into a second window.
    #[test]
    fn a_page_torn_into_a_new_window_lays_out_below_that_windows_strip() {
        let captured: Rc<RefCell<Option<Windows>>> = Rc::new(RefCell::new(None));
        let mut shell = AppShell::new(
            TestRenderer::default(),
            location_key(file!(), line!(), column!()),
            {
                let captured = Rc::clone(&captured);
                move || {
                    let captured = Rc::clone(&captured);
                    TornWindowsHost(
                        "test-tabs",
                        Rules::tabs("t", WINDOW, STRIP.height),
                        vec![(1, WINDOW), (2, WINDOW)],
                        move |view| {
                            *captured.borrow_mut() = Some(view.windows().clone());
                            TestChrome(view.clone());
                        },
                    );
                }
            },
        );
        shell.add_window_surface(
            window_root_id(1),
            TestRenderer::default(),
            (200, 100),
            (200.0, 100.0),
        );
        shell.update();
        let windows = (*captured.borrow()).clone().expect("chrome ran");
        windows.press(1, Point::new(40.0, 15.0), Some(Point::new(240.0, 175.0)));
        windows.drag_step(1, Point::new(40.0, 80.0), Some(Point::new(240.0, 240.0)));
        let model = windows.model().get_non_reactive();
        assert_eq!(
            model.windows().len(),
            2,
            "the tab tore into a second window"
        );
        assert_eq!(model.window_of(1), Some(2));

        shell.update();
        shell.add_window_surface(
            window_root_id(2),
            TestRenderer::default(),
            (200, 100),
            (200.0, 100.0),
        );
        let (strip, body) = strip_and_body_in(&mut shell, 2);
        assert_eq!(strip, Some((0.0, 0.0)), "the strip is at the top");
        assert_eq!(body, Some((0.0, 36.0)), "the page lays out below the strip");

        windows.release();
        let state = *windows
            .shared
            .borrow()
            .states
            .get(&2)
            .expect("the second window remembered its state");
        state.set_size(Size::new(WINDOW.width + 10.0, WINDOW.height));
        let (strip, body) = strip_and_body_in(&mut shell, 2);
        assert_eq!(strip, Some((0.0, 0.0)), "the strip stays at the top");
        assert_eq!(
            body,
            Some((0.0, 36.0)),
            "a window recomposed with its chrome skipped keeps the page below the strip"
        );
    }
}
