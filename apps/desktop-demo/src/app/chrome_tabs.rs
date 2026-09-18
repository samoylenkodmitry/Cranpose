//! A Chromium-style tabbed window whose tabs tear off into windows of their own.
//!
//! Every native window in Cranpose owns a whole `AppShell`: its own composition,
//! its own renderer, its own pointer state. Two windows are therefore two
//! compositions running in parallel, and merging them into one window means one
//! composition again. Nothing can be carried across that boundary, because the
//! composition a tab is living in is destroyed the moment the tab leaves it.
//!
//! So this demo keeps every byte a tab needs in [`TabsModel`], which is
//! remembered once in the root composition and outlives every window. A window's
//! content is then a pure function of that model, and moving a tab is only an
//! edit to the model: the window it left rebuilds without it, and the window it
//! arrived in is declared into existence by the same loop that declares all the
//! others. No renderer change, no composer change.

#![allow(non_snake_case)]

use std::collections::BTreeMap;

use cranpose::{rememberWindowState, WindowConfig, WindowId, WindowNode, WindowState};
use cranpose_core::{key, rememberMutableStateOf, MutableState};
use cranpose_ui::{
    composable, text::TextUnit, Color, Column, ColumnSpec, HorizontalAlignment, Modifier, Point,
    PointerEventKind, PointerInputScope, Row, RowSpec, Text, TextStyle, VerticalAlignment,
};

/// Identifies one tab for as long as it exists, in whichever window holds it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct TabId(u64);

/// Identifies one window that shows a strip of tabs.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct TabWindowId(u64);

impl TabWindowId {
    /// The raw key, for deriving a [`WindowId`] for this window.
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// One window: the tabs it holds in strip order, and which one it is showing.
#[derive(Clone, PartialEq, Debug)]
pub struct TabWindow {
    /// Identifier of this window.
    pub id: TabWindowId,
    /// The tabs this window holds, in the order the strip draws them.
    pub tabs: Vec<TabId>,
    /// The tab whose body the window currently shows.
    pub active: TabId,
    /// Where the window is first placed, in screen coordinates.
    pub origin: (f32, f32),
}

/// Which tabs exist, what each holds, and how they are spread over windows.
///
/// This is the only owner of anything a tab must keep. A tab that moves to
/// another window is composed from scratch there, so state kept inside a
/// window's composition would be lost; state kept here is simply read again.
#[derive(Clone, PartialEq, Debug)]
pub struct TabsModel<T> {
    payloads: BTreeMap<TabId, T>,
    windows: Vec<TabWindow>,
    next_id: u64,
}

impl<T: Clone + PartialEq> TabsModel<T> {
    /// Creates a model with a single window holding a single tab.
    pub fn new(first: T) -> Self {
        let tab = TabId(0);
        let window = TabWindow {
            id: TabWindowId(1),
            tabs: vec![tab],
            active: tab,
            origin: (200.0, 160.0),
        };
        let mut payloads = BTreeMap::new();
        payloads.insert(tab, first);
        Self {
            payloads,
            windows: vec![window],
            next_id: 2,
        }
    }

    /// Every window this model asks the platform to show.
    pub fn windows(&self) -> &[TabWindow] {
        &self.windows
    }

    /// The window with the given identifier, if it is still open.
    pub fn window(&self, id: TabWindowId) -> Option<&TabWindow> {
        self.windows.iter().find(|window| window.id == id)
    }

    /// What the given tab holds, or `None` once it is closed.
    pub fn payload(&self, tab: TabId) -> Option<&T> {
        self.payloads.get(&tab)
    }

    /// What the given tab holds, for editing in place.
    pub fn payload_mut(&mut self, tab: TabId) -> Option<&mut T> {
        self.payloads.get_mut(&tab)
    }

    /// The window currently holding the given tab.
    pub fn window_of(&self, tab: TabId) -> Option<TabWindowId> {
        self.windows
            .iter()
            .find(|window| window.tabs.contains(&tab))
            .map(|window| window.id)
    }

    /// Appends a tab to the given window and shows it.
    pub fn open(&mut self, window: TabWindowId, payload: T) -> TabId {
        let tab = TabId(self.take_id());
        self.payloads.insert(tab, payload);
        if let Some(found) = self.windows.iter_mut().find(|held| held.id == window) {
            found.tabs.push(tab);
            found.active = tab;
        }
        tab
    }

    /// Shows the given tab in whichever window holds it.
    pub fn activate(&mut self, tab: TabId) {
        if let Some(found) = self
            .windows
            .iter_mut()
            .find(|window| window.tabs.contains(&tab))
        {
            found.active = tab;
        }
    }

    /// Closes one tab, closing its window when it held nothing else.
    pub fn close(&mut self, tab: TabId) {
        self.detach(tab);
        self.payloads.remove(&tab);
    }

    /// Closes a window and every tab it holds.
    pub fn close_window(&mut self, window: TabWindowId) {
        let Some(index) = self.windows.iter().position(|held| held.id == window) else {
            return;
        };
        for tab in std::mem::take(&mut self.windows[index].tabs) {
            self.payloads.remove(&tab);
        }
        self.windows.remove(index);
    }

    /// Moves a tab out of its window into a new window placed at `origin`.
    ///
    /// Returns `None` when the tab is the only one in its window, because
    /// tearing it out would replace that window with an identical one.
    pub fn tear_out(&mut self, tab: TabId, origin: (f32, f32)) -> Option<TabWindowId> {
        let from = self.window_of(tab)?;
        if self.window(from)?.tabs.len() < 2 {
            return None;
        }
        self.detach(tab);
        let id = TabWindowId(self.take_id());
        self.windows.push(TabWindow {
            id,
            tabs: vec![tab],
            active: tab,
            origin,
        });
        Some(id)
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn detach(&mut self, tab: TabId) {
        let Some(index) = self
            .windows
            .iter()
            .position(|window| window.tabs.contains(&tab))
        else {
            return;
        };
        let window = &mut self.windows[index];
        let removed = window
            .tabs
            .iter()
            .position(|held| *held == tab)
            .unwrap_or_default();
        window.tabs.retain(|held| *held != tab);
        let Some(last) = window.tabs.len().checked_sub(1) else {
            self.windows.remove(index);
            return;
        };
        if window.active == tab {
            window.active = window.tabs[removed.min(last)];
        }
    }
}

/// What one tab of this demo holds.
#[derive(Clone, PartialEq, Debug)]
pub struct Page {
    /// The title the strip shows.
    pub title: String,
    /// How often the body's button was pressed.
    ///
    /// This is the demo's proof that a torn tab keeps its state: the count lives
    /// in the model, not in the composition the tab is torn out of.
    pub clicks: u32,
    /// The tint of the body, so one tab stays recognisable across windows.
    pub tint: Color,
}

impl Page {
    /// Creates the `nth` page, numbered and tinted so it can be told apart.
    pub fn new(nth: u32) -> Self {
        let hue = (nth as f32 * 0.37).fract();
        Self {
            title: format!("Tab {nth}"),
            clicks: 0,
            tint: tint_for(hue),
        }
    }
}

fn tint_for(hue: f32) -> Color {
    let wrap = |offset: f32| ((hue + offset).fract() * 6.0 - 3.0).abs().clamp(0.0, 1.0);
    Color(
        0.25 + 0.45 * wrap(0.0),
        0.25 + 0.45 * wrap(0.33),
        0.25 + 0.45 * wrap(0.66),
        1.0,
    )
}

/// The whole demo: one window per entry in the model, declared from state.
#[composable]
pub fn chrome_tabs_app() {
    let model = rememberMutableStateOf(|| TabsModel::new(Page::new(1)));
    let snapshot = model.get();
    let open: Vec<TabWindow> = snapshot.windows().to_vec();
    for window in open {
        key(window.id.raw(), || {
            TornWindow(model, window);
        });
    }
}

#[composable]
fn TornWindow(model: MutableState<TabsModel<Page>>, window: TabWindow) {
    let state = rememberWindowState(WINDOW_WIDTH, WINDOW_HEIGHT);
    let id = window.id;
    WindowNode(
        WindowId::from_runtime("chrome-tabs", id.raw()),
        WindowConfig::new_for_state(window_title(&window), state)
            .with_position(window.origin.0, window.origin.1)
            .on_close_requested(move || model.update(|held| held.close_window(id))),
        move || {
            TabbedWindow(model, id, state);
        },
    );
}

fn window_title(window: &TabWindow) -> String {
    match window.tabs.len() {
        1 => "Cranpose Tabs".to_owned(),
        count => format!("Cranpose Tabs ({count})"),
    }
}

#[composable]
fn TabbedWindow(model: MutableState<TabsModel<Page>>, window: TabWindowId, state: WindowState) {
    let snapshot = model.get();
    let Some(current) = snapshot.window(window) else {
        return;
    };
    let tabs = current.tabs.clone();
    let active = current.active;
    let body = snapshot.payload(active).cloned();
    Column(
        Modifier::empty().fill_max_size().background(CHROME),
        ColumnSpec::default(),
        move || {
            TabStrip(model, window, state, tabs.clone(), active);
            if let Some(page) = body.clone() {
                TabBody(model, active, page);
            }
        },
    );
}

#[composable]
fn TabStrip(
    model: MutableState<TabsModel<Page>>,
    window: TabWindowId,
    state: WindowState,
    tabs: Vec<TabId>,
    active: TabId,
) {
    Row(
        Modifier::empty()
            .fill_max_width()
            .height(STRIP_HEIGHT)
            .background(CHROME),
        RowSpec {
            vertical_alignment: VerticalAlignment::CenterVertically,
            ..RowSpec::default()
        },
        move || {
            for tab in &tabs {
                let tab = *tab;
                key(tab.0, || {
                    StripTab(model, state, tab, tab == active);
                });
            }
            NewTabButton(model, window);
        },
    );
}

#[composable]
fn StripTab(model: MutableState<TabsModel<Page>>, state: WindowState, tab: TabId, selected: bool) {
    let title = model
        .get()
        .payload(tab)
        .map(|page| page.title.clone())
        .unwrap_or_default();
    let background = if selected { ACTIVE_TAB } else { IDLE_TAB };
    Text(
        title,
        tear_gesture(model, state, tab)
            .width(TAB_WIDTH)
            .height(TAB_HEIGHT)
            .background(background)
            .padding(8.0),
        label_style(13.0, if selected { INK } else { FADED_INK }),
    );
}

#[composable]
fn NewTabButton(model: MutableState<TabsModel<Page>>, window: TabWindowId) {
    Text(
        "+",
        Modifier::empty()
            .width(NEW_TAB_WIDTH)
            .height(TAB_HEIGHT)
            .background(IDLE_TAB)
            .padding(4.0)
            .clickable(move |_| {
                model.update(|held| {
                    let nth = held.windows().iter().map(|w| w.tabs.len()).sum::<usize>() + 1;
                    held.open(window, Page::new(nth as u32));
                });
            }),
        label_style(16.0, INK),
    );
}

#[composable]
fn TabBody(model: MutableState<TabsModel<Page>>, tab: TabId, page: Page) {
    Column(
        Modifier::empty().fill_max_size().background(page.tint),
        ColumnSpec {
            horizontal_alignment: HorizontalAlignment::CenterHorizontally,
            ..ColumnSpec::default()
        },
        move || {
            Text(
                page.title.clone(),
                Modifier::empty().padding(20.0),
                label_style(28.0, INK),
            );
            Text(
                format!("pressed {} times", page.clicks),
                Modifier::empty().padding(8.0),
                label_style(15.0, INK),
            );
            Text(
                "press me, then drag this tab out of the strip",
                Modifier::empty()
                    .padding(12.0)
                    .background(ACTIVE_TAB)
                    .clickable(move |_| {
                        model.update(|held| {
                            if let Some(found) = held.payload_mut(tab) {
                                found.clicks += 1;
                            }
                        });
                    }),
                label_style(14.0, INK),
            );
        },
    );
}

fn tear_gesture(model: MutableState<TabsModel<Page>>, state: WindowState, tab: TabId) -> Modifier {
    Modifier::empty().pointer_input(tab.0, move |scope: PointerInputScope| async move {
        scope
            .await_pointer_event_scope(|await_scope| async move {
                let mut grab: Option<Point> = None;
                loop {
                    let event = await_scope.await_pointer_event().await;
                    match event.kind {
                        PointerEventKind::Down => {
                            grab = Some(event.position);
                            model.update(|held| held.activate(tab));
                            event.consume();
                        }
                        PointerEventKind::Move => {
                            let Some(start) = grab else { continue };
                            if event.buttons == Default::default() {
                                grab = None;
                                continue;
                            }
                            if (event.position.y - start.y).abs() < TEAR_DISTANCE {
                                continue;
                            }
                            grab = None;
                            tear_out_at(model, state, tab, start, event.global_position);
                        }
                        PointerEventKind::Up | PointerEventKind::Cancel => grab = None,
                        _ => {}
                    }
                }
            })
            .await;
    })
}

fn tear_out_at(
    model: MutableState<TabsModel<Page>>,
    state: WindowState,
    tab: TabId,
    grab: Point,
    cursor: Point,
) {
    let origin = state
        .position_non_reactive()
        .unwrap_or(Point::new(0.0, 0.0));
    let placed = (
        origin.x + cursor.x - grab.x,
        origin.y + cursor.y - grab.y - STRIP_HEIGHT,
    );
    model.update(|held| {
        held.tear_out(tab, placed);
    });
}

fn label_style(size: f32, color: Color) -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.font_size = TextUnit::Sp(size);
    style.span_style.color = Some(color);
    style
}

const WINDOW_WIDTH: f32 = 520.0;
const WINDOW_HEIGHT: f32 = 360.0;
const STRIP_HEIGHT: f32 = 36.0;
const TAB_WIDTH: f32 = 116.0;
const TAB_HEIGHT: f32 = 30.0;
const NEW_TAB_WIDTH: f32 = 36.0;
const TEAR_DISTANCE: f32 = 26.0;
const CHROME: Color = Color(0.13, 0.14, 0.17, 1.0);
const IDLE_TAB: Color = Color(0.20, 0.21, 0.25, 1.0);
const ACTIVE_TAB: Color = Color(0.32, 0.34, 0.40, 1.0);
const INK: Color = Color(0.96, 0.97, 1.0, 1.0);
const FADED_INK: Color = Color(0.70, 0.72, 0.80, 1.0);

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> TabsModel<u32> {
        TabsModel::new(10)
    }

    #[test]
    fn a_new_model_shows_one_window_holding_one_tab() {
        let model = model();
        assert_eq!(model.windows().len(), 1);
        assert_eq!(model.windows()[0].tabs.len(), 1);
    }

    #[test]
    fn opening_a_tab_appends_it_and_shows_it() {
        let mut model = model();
        let window = model.windows()[0].id;
        let opened = model.open(window, 20);
        assert_eq!(model.window(window).map(|held| held.tabs.len()), Some(2));
        assert_eq!(model.window(window).map(|held| held.active), Some(opened));
    }

    #[test]
    fn tearing_a_tab_out_moves_it_into_a_window_of_its_own() {
        let mut model = model();
        let source = model.windows()[0].id;
        let torn = model.open(source, 20);
        let created = model.tear_out(torn, (40.0, 50.0)).expect("a new window");
        assert_eq!(model.window_of(torn), Some(created));
        assert_eq!(model.window(source).map(|held| held.tabs.len()), Some(1));
        assert_eq!(
            model.window(created).map(|held| held.origin),
            Some((40.0, 50.0))
        );
    }

    #[test]
    fn a_torn_tab_keeps_what_it_held() {
        let mut model = model();
        let source = model.windows()[0].id;
        let torn = model.open(source, 77);
        model.tear_out(torn, (0.0, 0.0));
        assert_eq!(model.payload(torn), Some(&77));
    }

    #[test]
    fn a_lone_tab_does_not_tear_out_of_its_own_window() {
        let mut model = model();
        let lone = model.windows()[0].tabs[0];
        assert_eq!(model.tear_out(lone, (0.0, 0.0)), None);
        assert_eq!(model.windows().len(), 1);
    }

    #[test]
    fn closing_the_last_tab_closes_its_window() {
        let mut model = model();
        let window = model.windows()[0].id;
        let lone = model.windows()[0].tabs[0];
        model.close(lone);
        assert!(model.window(window).is_none());
        assert!(model.windows().is_empty());
    }

    #[test]
    fn closing_the_shown_tab_shows_the_one_that_took_its_place() {
        let mut model = model();
        let window = model.windows()[0].id;
        let first = model.windows()[0].tabs[0];
        let second = model.open(window, 20);
        let third = model.open(window, 30);
        model.activate(second);
        model.close(second);
        assert_eq!(model.window(window).map(|held| held.active), Some(third));
        assert_eq!(
            model.window(window).map(|held| held.tabs.clone()),
            Some(vec![first, third])
        );
    }

    #[test]
    fn closing_a_window_closes_every_tab_it_held() {
        let mut model = model();
        let window = model.windows()[0].id;
        let second = model.open(window, 20);
        model.close_window(window);
        assert!(model.windows().is_empty());
        assert_eq!(model.payload(second), None);
    }

    #[test]
    fn every_window_gets_its_own_identifier() {
        let mut model = model();
        let source = model.windows()[0].id;
        let a = model.open(source, 20);
        let b = model.open(source, 30);
        let first = model.tear_out(a, (0.0, 0.0)).expect("a window");
        let second = model.tear_out(b, (0.0, 0.0)).expect("a window");
        assert_ne!(first, second);
        assert_ne!(first, source);
    }
}
