#![allow(non_snake_case)]

use std::{cell::Cell, rc::Rc};

use cranpose::{rememberWindowStateAt, WindowConfig, WindowModifierExt};
use cranpose_core::{forget_movable, key, movable, remember, rememberMutableStateOf, MutableState};
use cranpose_ui::{
    composable, text::TextUnit, Box, BoxSpec, Color, Column, ColumnSpec, DragAndDropOutcome,
    DragAndDropSource, DragAndDropTarget, HorizontalAlignment, Modifier, Point, Row, RowSpec, Text,
    TextStyle, VerticalAlignment,
};

use super::demo_trace::trace;

#[derive(Clone, PartialEq, Debug)]
pub struct Page {
    pub id: u64,
    pub title: String,
    pub tint: Color,
}

impl Page {
    pub fn new(nth: u64) -> Self {
        let hue = (nth as f32 * 0.37).fract();
        Self {
            id: nth,
            title: format!("Tab {nth}"),
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

fn page_key(id: u64) -> (&'static str, u64) {
    ("page", id)
}

#[derive(Clone, PartialEq, Debug)]
pub struct TabWindow {
    pub id: u64,
    pub pages: Vec<u64>,
    pub active: u64,
    pub origin: Point,
}

#[derive(Clone, PartialEq, Debug)]
pub struct TabWindows {
    windows: Vec<TabWindow>,
    next_window: u64,
}

impl TabWindows {
    pub fn new(first_page: u64) -> Self {
        Self {
            windows: vec![TabWindow {
                id: 1,
                pages: vec![first_page],
                active: first_page,
                origin: FIRST_ORIGIN,
            }],
            next_window: 2,
        }
    }

    pub fn windows(&self) -> &[TabWindow] {
        &self.windows
    }

    pub fn window_of(&self, page: u64) -> Option<u64> {
        self.windows
            .iter()
            .find(|window| window.pages.contains(&page))
            .map(|window| window.id)
    }

    pub fn active_page(&self, window: u64) -> Option<u64> {
        self.windows
            .iter()
            .find(|held| held.id == window)
            .map(|held| held.active)
    }

    pub fn add_page(&mut self, window: u64, page: u64) {
        if let Some(window) = self.windows.iter_mut().find(|held| held.id == window) {
            window.pages.push(page);
            window.active = page;
        }
    }

    pub fn activate(&mut self, page: u64) {
        if let Some(window) = self
            .windows
            .iter_mut()
            .find(|held| held.pages.contains(&page))
        {
            window.active = page;
        }
    }

    pub fn moved(&mut self, window: u64, origin: Point) {
        if let Some(window) = self.windows.iter_mut().find(|held| held.id == window) {
            window.origin = origin;
        }
    }

    /// The window whose strip this one's strip has been laid over, which is
    /// how a window carrying a single tab asks to be taken back in. A window
    /// being dragged is always under the pointer, so where its tab could be
    /// dropped is a question about the windows themselves, not about what
    /// the pointer is over.
    pub fn strip_laid_over(&self, window: u64) -> Option<u64> {
        let mine = self.windows.iter().find(|held| held.id == window)?;
        self.windows
            .iter()
            .find(|other| other.id != window && strips_overlap(mine.origin, other.origin))
            .map(|other| other.id)
    }

    pub fn close_page(&mut self, page: u64) {
        self.take_page(page);
    }

    pub fn move_page(&mut self, page: u64, into: u64) -> bool {
        if self.window_of(page) == Some(into) {
            return true;
        }
        if !self.windows.iter().any(|window| window.id == into) {
            return false;
        }
        self.take_page(page);
        self.add_page(into, page);
        true
    }

    pub fn tear_page(&mut self, page: u64, origin: Point) -> Option<u64> {
        if self.windows.iter().any(|window| window.pages == [page]) {
            return None;
        }
        self.take_page(page);
        let id = self.next_window;
        self.next_window += 1;
        self.windows.push(TabWindow {
            id,
            pages: vec![page],
            active: page,
            origin,
        });
        Some(id)
    }

    fn take_page(&mut self, page: u64) {
        for window in &mut self.windows {
            let Some(index) = window.pages.iter().position(|held| *held == page) else {
                continue;
            };
            window.pages.remove(index);
            if window.active == page {
                window.active = window
                    .pages
                    .get(index)
                    .or_else(|| window.pages.last())
                    .copied()
                    .unwrap_or(0);
            }
        }
        self.windows.retain(|window| !window.pages.is_empty());
    }
}

fn strips_overlap(mine: Point, other: Point) -> bool {
    (mine.x - other.x).abs() < WINDOW_WIDTH && (mine.y - other.y).abs() < STRIP_HEIGHT
}

#[composable]
pub fn chrome_tabs_app() {
    let pages = rememberMutableStateOf(|| vec![Page::new(1)]);
    let next_page = rememberMutableStateOf(|| 2u64);
    let windows = rememberMutableStateOf(|| TabWindows::new(1));
    let snapshot = windows.get();
    trace!(
        "windows={}",
        snapshot
            .windows()
            .iter()
            .map(|window| window.id.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    for window in snapshot.windows().to_vec() {
        key(window.id, move || {
            TabWindowFrame(window, windows, pages, next_page);
        });
    }
}

#[composable]
fn TabWindowFrame(
    window: TabWindow,
    windows: MutableState<TabWindows>,
    pages: MutableState<Vec<Page>>,
    next_page: MutableState<u64>,
) {
    let id = window.id;
    let state = rememberWindowStateAt(
        window.origin.x,
        window.origin.y,
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
    );
    if let Some(origin) = state.position() {
        let size = state.size();
        trace!(
            "window id={} origin=({:.1},{:.1}) size=({:.1},{:.1}) panes={} parked=false",
            window.id,
            origin.x,
            origin.y,
            size.width,
            size.height,
            window.pages.len()
        );
    }
    Box(
        Modifier::empty().window(
            WindowConfig::borderless_for_state("Cranpose Tabs", state)
                .with_transparent(true)
                .on_moved(move |x, y| {
                    windows.update(|held| held.moved(id, Point::new(x, y)));
                }),
        ),
        BoxSpec::default(),
        move || BrowserWindow(window.clone(), windows, pages, next_page),
    );
}

#[composable]
fn BrowserWindow(
    window: TabWindow,
    windows: MutableState<TabWindows>,
    pages: MutableState<Vec<Page>>,
    next_page: MutableState<u64>,
) {
    let page = pages
        .get()
        .iter()
        .find(|page| page.id == window.active)
        .cloned();
    Column(
        Modifier::empty().fill_max_size().background(CHROME),
        ColumnSpec::default(),
        move || {
            TabStrip(window.clone(), windows, pages, next_page);
            if let Some(page) = page.clone() {
                movable(page_key(page.id), move || PageBody(page));
            }
        },
    );
}

#[composable]
fn TabStrip(
    window: TabWindow,
    windows: MutableState<TabWindows>,
    pages: MutableState<Vec<Page>>,
    next_page: MutableState<u64>,
) {
    let id = window.id;
    let active = window.active;
    let target = DragAndDropTarget::new().on_drop(move |payload, _| {
        let Some(page) = payload.downcast_ref::<u64>().copied() else {
            return false;
        };
        let moved = windows.update(|held| held.move_page(page, id));
        trace!("transfer dropped page={page} onto={id} moved={moved}");
        moved
    });
    Row(
        Modifier::empty()
            .fill_max_width()
            .height(STRIP_HEIGHT)
            .background(CHROME)
            .drag_and_drop_target(target),
        RowSpec {
            vertical_alignment: VerticalAlignment::CenterVertically,
            ..RowSpec::default()
        },
        move || {
            for page in window.pages.clone() {
                let alone = window.pages.len() == 1;
                key(page, move || {
                    StripTab(id, page, page == active, alone, windows, pages);
                });
            }
            NewTabButton(id, windows, pages, next_page);
            Box(
                Modifier::empty()
                    .weight(1.0)
                    .fill_max_height()
                    .window_drag_area(),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[composable]
fn StripTab(
    window: u64,
    page: u64,
    selected: bool,
    alone: bool,
    windows: MutableState<TabWindows>,
    pages: MutableState<Vec<Page>>,
) {
    let title = pages
        .get()
        .iter()
        .find(|held| held.id == page)
        .map(|held| held.title.clone())
        .unwrap_or_default();
    let last_screen = remember(|| Rc::new(Cell::new(None::<Point>))).with(Rc::clone);
    let torn = remember(|| Rc::new(Cell::new(false))).with(Rc::clone);
    let source = tab_source(window, page, windows, last_screen, torn);
    let background = if selected { ACTIVE_TAB } else { IDLE_TAB };
    Row(
        tab_grip(
            Modifier::empty()
                .width(TAB_WIDTH)
                .height(TAB_HEIGHT)
                .background(background)
                .clickable(move |_| windows.update(|held| held.activate(page))),
            alone.then_some((window, windows)),
            source,
        ),
        RowSpec {
            vertical_alignment: VerticalAlignment::CenterVertically,
            ..RowSpec::default()
        },
        move || {
            Text(
                title.clone(),
                Modifier::empty().width(TAB_WIDTH - TAB_HEIGHT).padding(8.0),
                label_style(13.0, if selected { INK } else { FADED_INK }),
            );
            Text(
                "×",
                Modifier::empty()
                    .width(TAB_HEIGHT)
                    .padding(6.0)
                    .clickable(move |_| {
                        pages.update(|held| held.retain(|held| held.id != page));
                        windows.update(|held| held.close_page(page));
                        forget_movable(page_key(page));
                    }),
                label_style(14.0, FADED_INK),
            );
        },
    );
}

fn tab_source(
    window: u64,
    page: u64,
    windows: MutableState<TabWindows>,
    last_screen: Rc<Cell<Option<Point>>>,
    torn: Rc<Cell<bool>>,
) -> DragAndDropSource {
    let moved = Rc::clone(&last_screen);
    let moved_torn = Rc::clone(&torn);
    let started_torn = Rc::clone(&torn);
    DragAndDropSource::new(page)
        .on_started(move |_| {
            started_torn.set(false);
            trace!("transfer started page={page} from={window}");
        })
        .on_moved(move |point| {
            moved.set(point.screen);
            if moved_torn.get() || !tab_left_the_strip(point.local) {
                return;
            }
            let Some(screen) = point.screen else {
                return;
            };
            if tear_into_a_window_of_its_own(windows, page, screen) {
                moved_torn.set(true);
            }
        })
        .on_ended(move |outcome| match outcome {
            DragAndDropOutcome::Missed if !torn.get() => tear(windows, page, last_screen.get()),
            DragAndDropOutcome::Missed => trace!("transfer let go of a torn page={page}"),
            DragAndDropOutcome::Dropped => trace!("transfer done page={page}"),
            DragAndDropOutcome::Cancelled => trace!("transfer cancelled page={page}"),
        })
}

/// Whether the pointer has carried the tab clear of the strip it sits in,
/// which is when the tab becomes a window of its own and takes the press
/// with it, the way a tool pane does.
pub(crate) fn tab_left_the_strip(local: Point) -> bool {
    local.y > STRIP_HEIGHT + TEAR_DEPTH || local.y < -TEAR_DEPTH
}

/// A tab sharing a strip is dragged out of it; a tab alone in its window
/// drags the window it already has, and goes back into another strip when it
/// is let go over one. A window being dragged is under the pointer the whole
/// time, so that last question is about where the windows are, not about
/// what the pointer is over.
fn tab_grip(
    modifier: Modifier,
    alone: Option<(u64, MutableState<TabWindows>)>,
    source: DragAndDropSource,
) -> Modifier {
    let Some((window, windows)) = alone else {
        return modifier.drag_and_drop_source(source);
    };
    modifier.window_drag_area_with_callbacks(
        || {},
        move || {
            let Some(onto) = windows.get_non_reactive().strip_laid_over(window) else {
                return;
            };
            let page = windows.get_non_reactive().active_page(window);
            let Some(page) = page else { return };
            let moved = windows.update(|held| held.move_page(page, onto));
            trace!("transfer dropped page={page} onto={onto} moved={moved}");
        },
    )
}

fn tear_into_a_window_of_its_own(
    windows: MutableState<TabWindows>,
    page: u64,
    screen: Point,
) -> bool {
    let origin = Point::new(screen.x - TEAR_GRAB.x, screen.y - TEAR_GRAB.y);
    let torn = windows.update(|held| held.tear_page(page, origin));
    trace!(
        "tab left the strip page={page} torn={torn:?} at=({:.1},{:.1})",
        origin.x,
        origin.y
    );
    torn.is_some()
}

fn tear(windows: MutableState<TabWindows>, page: u64, screen: Option<Point>) {
    let Some(screen) = screen else {
        trace!("transfer missed page={page} without a screen position");
        return;
    };
    let origin = Point::new(screen.x - TEAR_GRAB.x, screen.y - TEAR_GRAB.y);
    let torn = windows.update(|held| held.tear_page(page, origin));
    trace!(
        "transfer missed page={page} torn={torn:?} at=({:.1},{:.1})",
        origin.x,
        origin.y
    );
}

#[composable]
fn NewTabButton(
    window: u64,
    windows: MutableState<TabWindows>,
    pages: MutableState<Vec<Page>>,
    next_page: MutableState<u64>,
) {
    Text(
        "+",
        Modifier::empty()
            .width(NEW_TAB_WIDTH)
            .height(TAB_HEIGHT)
            .background(IDLE_TAB)
            .padding(4.0)
            .clickable(move |_| {
                let id = next_page.get_non_reactive();
                next_page.set(id + 1);
                pages.update(|held| held.push(Page::new(id)));
                windows.update(|held| held.add_page(window, id));
            }),
        label_style(16.0, INK),
    );
}

#[composable]
fn PageBody(page: Page) {
    let clicks = rememberMutableStateOf(|| 0u32);
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
                format!("pressed {} times", clicks.get()),
                Modifier::empty().padding(8.0),
                label_style(15.0, INK),
            );
            Text(
                "press me, then drag this tab out of the strip or onto another window's strip",
                Modifier::empty()
                    .padding(12.0)
                    .background(ACTIVE_TAB)
                    .clickable(move |_| clicks.set(clicks.get_non_reactive() + 1)),
                label_style(14.0, INK),
            );
        },
    );
}

pub(crate) fn label_style(size: f32, color: Color) -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.font_size = TextUnit::Sp(size);
    style.span_style.color = Some(color);
    style
}

const WINDOW_WIDTH: f32 = 520.0;
const WINDOW_HEIGHT: f32 = 360.0;
const STRIP_HEIGHT: f32 = 36.0;
const TAB_WIDTH: f32 = 140.0;
const TAB_HEIGHT: f32 = 30.0;
const NEW_TAB_WIDTH: f32 = 36.0;
const FIRST_ORIGIN: Point = Point { x: 200.0, y: 160.0 };
const TEAR_GRAB: Point = Point { x: 56.0, y: 18.0 };
const TEAR_DEPTH: f32 = 24.0;
pub(crate) const CHROME: Color = Color(0.13, 0.14, 0.17, 1.0);
const IDLE_TAB: Color = Color(0.20, 0.21, 0.25, 1.0);
const ACTIVE_TAB: Color = Color(0.32, 0.34, 0.40, 1.0);
pub(crate) const INK: Color = Color(0.96, 0.97, 1.0, 1.0);
const FADED_INK: Color = Color(0.70, 0.72, 0.80, 1.0);

#[cfg(test)]
#[path = "tests/chrome_tabs_tests.rs"]
mod tests;
