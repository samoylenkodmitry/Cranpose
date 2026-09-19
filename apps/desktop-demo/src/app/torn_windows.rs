use std::{cell::RefCell, collections::HashMap, rc::Rc};

use cranpose::{rememberWindowStateAt, WindowConfig, WindowId, WindowNode, WindowState};
use cranpose_core::{key, remember, rememberMutableStateOf, MutableState};
use cranpose_ui::{
    composable, Box, BoxSpec, Modifier, Point, PointerEventKind, PointerInputScope, Size,
};

macro_rules! trace {
    ($($arg:tt)*) => {
        if trace_enabled() {
            print_trace(format_args!($($arg)*));
        }
    };
}

#[derive(Clone, PartialEq, Debug)]
pub struct TornWindow {
    pub id: u64,
    pub panes: Vec<u64>,
    pub active: u64,
    pub origin: Point,
    pub parked: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    Vertical,
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

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum JoinRule {
    Strip { height: f32 },
    Edge { axis: Axis, reach: f32 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Before,
    After,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Rules {
    pub title: String,
    pub window_size: Size,
    pub join: JoinRule,
    pub tear_distance: f32,
    pub first_origin: Point,
    pub tear_grab: Point,
}

impl Rules {
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

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub window: u64,
    pub origin: Point,
    pub size: Size,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Drag {
    pub pane: u64,
    pub source: u64,
    pub grab: Point,
    pub anchor: Point,
    pub carrying: Option<u64>,
    pub clear: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Step {
    Rest,
    Carry(u64, Point),
    Join(u64, Side),
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct TornWindows {
    windows: Vec<TornWindow>,
    sizes: Vec<(u64, Size)>,
    drag: Option<Drag>,
    placements: Vec<(u64, u64)>,
    next_id: u64,
}

impl TornWindows {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn windows(&self) -> &[TornWindow] {
        &self.windows
    }

    pub fn window(&self, id: u64) -> Option<&TornWindow> {
        self.windows.iter().find(|window| window.id == id)
    }

    pub fn window_of(&self, pane: u64) -> Option<u64> {
        self.windows
            .iter()
            .find(|window| window.panes.contains(&pane))
            .map(|window| window.id)
    }

    pub fn drag(&self) -> Option<Drag> {
        self.drag
    }

    pub fn pane_size(&self, pane: u64) -> Size {
        self.sizes
            .iter()
            .find(|(held, _)| *held == pane)
            .map(|(_, size)| *size)
            .unwrap_or(Size::new(0.0, 0.0))
    }

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

    pub fn place_next_in(&mut self, pane: u64, window: u64) {
        self.placements.push((pane, window));
    }

    pub fn activate(&mut self, pane: u64) {
        if let Some(window) = self
            .windows
            .iter_mut()
            .find(|window| window.panes.contains(&pane))
        {
            window.active = pane;
        }
    }

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

    pub fn model(&self) -> MutableState<TornWindows> {
        self.model
    }

    pub fn activate(&self, pane: u64) {
        self.model.update(|model| model.activate(pane));
    }

    pub fn size_of(&self, pane: u64) -> Size {
        self.model.get_non_reactive().pane_size(pane)
    }

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
    pub fn windows(&self) -> &Windows {
        &self.windows
    }

    pub fn id(&self) -> u64 {
        self.window.id
    }

    pub fn panes(&self) -> &[u64] {
        &self.window.panes
    }

    pub fn active(&self) -> u64 {
        self.window.active
    }

    pub fn open_next_here(&self, pane: u64) {
        let window = self.window.id;
        self.windows
            .model
            .update(|model| model.place_next_in(pane, window));
    }
}

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
#[path = "tests/torn_windows_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/torn_windows_composition_tests.rs"]
mod composition_tests;
