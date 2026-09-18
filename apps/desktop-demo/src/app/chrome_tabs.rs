//! Browser-style tabs on top of the dock.
//!
//! The application owns its pages and draws its own strip with ordinary
//! composables. Everything about windows — which one holds which tab, tearing
//! a tab out under the pointer, dropping it onto another strip, opening and
//! closing windows — belongs to [`Dock`]. The click count on each page is the
//! proof that a torn tab keeps its state: it lives in `pages`, above every
//! window, so the composition a tab is torn out of has nothing to lose.

#![allow(non_snake_case)]

use cranpose::{Dock, DockHost, DockKey, DockModifierExt, DockPolicy, Pane, WindowModifierExt};
use cranpose_core::{key, rememberMutableStateOf, MutableState};
use cranpose_ui::{
    composable, text::TextUnit, Box, BoxSpec, Color, Column, ColumnSpec, HorizontalAlignment,
    Modifier, Row, RowSpec, Size, Text, TextStyle, VerticalAlignment,
};

/// What one tab holds.
#[derive(Clone, PartialEq, Debug)]
pub struct Page {
    /// Identity of the page, stable across every window it is torn into.
    pub id: u64,
    /// The title the strip shows.
    pub title: String,
    /// How often the body's button was pressed.
    pub clicks: u32,
    /// The tint of the body, so one page stays recognisable across windows.
    pub tint: Color,
}

impl Page {
    /// Creates the `nth` page, numbered and tinted so it can be told apart.
    pub fn new(nth: u64) -> Self {
        let hue = (nth as f32 * 0.37).fract();
        Self {
            id: nth,
            title: format!("Tab {nth}"),
            clicks: 0,
            tint: tint_for(hue),
        }
    }

    /// The dock key of this page.
    pub fn key(&self) -> DockKey {
        DockKey::from_runtime("page", self.id)
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

/// The whole demo: pages declared as panes, windows left to the dock.
#[composable]
pub fn chrome_tabs_app() {
    let pages = rememberMutableStateOf(|| vec![Page::new(1)]);
    let next = rememberMutableStateOf(|| 2u64);
    Dock(
        "chrome-tabs",
        DockPolicy::tabs(
            "Cranpose Tabs",
            Size::new(WINDOW_WIDTH, WINDOW_HEIGHT),
            STRIP_HEIGHT,
        ),
        move |host| BrowserWindow(host.clone(), pages, next),
        move || {
            for page in pages.get() {
                Pane(page.key(), move || PageBody(pages, page.clone()));
            }
        },
    );
}

#[composable]
fn BrowserWindow(host: DockHost, pages: MutableState<Vec<Page>>, next: MutableState<u64>) {
    let active = host.active();
    Column(
        Modifier::empty().fill_max_size().background(CHROME),
        ColumnSpec::default(),
        move || {
            TabStrip(host.clone(), pages, next);
            host.content(active);
        },
    );
}

#[composable]
fn TabStrip(host: DockHost, pages: MutableState<Vec<Page>>, next: MutableState<u64>) {
    let panes = host.panes().to_vec();
    let active = host.active();
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
            for pane in &panes {
                let pane = *pane;
                key(pane.raw(), || StripTab(pages, pane, pane == active));
            }
            NewTabButton(host.clone(), pages, next);
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
fn StripTab(pages: MutableState<Vec<Page>>, pane: DockKey, selected: bool) {
    let title = pages
        .get()
        .iter()
        .find(|page| page.key() == pane)
        .map(|page| page.title.clone())
        .unwrap_or_default();
    let background = if selected { ACTIVE_TAB } else { IDLE_TAB };
    Row(
        Modifier::empty()
            .dock_handle(pane)
            .width(TAB_WIDTH)
            .height(TAB_HEIGHT)
            .background(background),
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
                        pages.update(|held| held.retain(|page| page.key() != pane))
                    }),
                label_style(14.0, FADED_INK),
            );
        },
    );
}

#[composable]
fn NewTabButton(host: DockHost, pages: MutableState<Vec<Page>>, next: MutableState<u64>) {
    Text(
        "+",
        Modifier::empty()
            .width(NEW_TAB_WIDTH)
            .height(TAB_HEIGHT)
            .background(IDLE_TAB)
            .padding(4.0)
            .clickable(move |_| {
                let id = next.get_non_reactive();
                next.set(id + 1);
                let page = Page::new(id);
                host.open_next_here(page.key());
                pages.update(|held| held.push(page));
            }),
        label_style(16.0, INK),
    );
}

#[composable]
fn PageBody(pages: MutableState<Vec<Page>>, page: Page) {
    let id = page.id;
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
                        pages.update(|held| {
                            if let Some(found) = held.iter_mut().find(|page| page.id == id) {
                                found.clicks += 1;
                            }
                        });
                    }),
                label_style(14.0, INK),
            );
        },
    );
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
const TAB_WIDTH: f32 = 140.0;
const TAB_HEIGHT: f32 = 30.0;
const NEW_TAB_WIDTH: f32 = 36.0;
const CHROME: Color = Color(0.13, 0.14, 0.17, 1.0);
const IDLE_TAB: Color = Color(0.20, 0.21, 0.25, 1.0);
const ACTIVE_TAB: Color = Color(0.32, 0.34, 0.40, 1.0);
const INK: Color = Color(0.96, 0.97, 1.0, 1.0);
const FADED_INK: Color = Color(0.70, 0.72, 0.80, 1.0);
