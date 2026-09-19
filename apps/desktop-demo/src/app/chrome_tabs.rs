#![allow(non_snake_case)]

use cranpose::WindowModifierExt;
use cranpose_core::{forget_movable, key, movable, rememberMutableStateOf, MutableState};
use cranpose_ui::{
    composable, text::TextUnit, Box, BoxSpec, Color, Column, ColumnSpec, HorizontalAlignment,
    Modifier, Row, RowSpec, Size, Text, TextStyle, VerticalAlignment,
};

use super::torn_windows::{Rules, TornWindowsHost, WindowView};

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

#[composable]
pub fn chrome_tabs_app() {
    let pages = rememberMutableStateOf(|| vec![Page::new(1)]);
    let next = rememberMutableStateOf(|| 2u64);
    let rules = Rules::tabs(
        "Cranpose Tabs",
        Size::new(WINDOW_WIDTH, WINDOW_HEIGHT),
        STRIP_HEIGHT,
    );
    let panes = pages
        .get()
        .iter()
        .map(|page| (page.id, rules.window_size))
        .collect();
    TornWindowsHost("chrome-tabs", rules, panes, move |view| {
        BrowserWindow(view.clone(), pages, next)
    });
}

#[composable]
fn BrowserWindow(view: WindowView, pages: MutableState<Vec<Page>>, next: MutableState<u64>) {
    let active = view.active();
    let page = view
        .panes()
        .contains(&active)
        .then(|| pages.get().iter().find(|page| page.id == active).cloned())
        .flatten();
    Column(
        Modifier::empty().fill_max_size().background(CHROME),
        ColumnSpec::default(),
        move || {
            TabStrip(view.clone(), pages, next);
            if let Some(page) = page.clone() {
                movable(page_key(page.id), move || PageBody(page));
            }
        },
    );
}

#[composable]
fn TabStrip(view: WindowView, pages: MutableState<Vec<Page>>, next: MutableState<u64>) {
    let panes = view.panes().to_vec();
    let active = view.active();
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
                let view = view.clone();
                key(pane, move || StripTab(view, pages, pane, pane == active));
            }
            NewTabButton(view.clone(), pages, next);
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
fn StripTab(view: WindowView, pages: MutableState<Vec<Page>>, pane: u64, selected: bool) {
    let title = pages
        .get()
        .iter()
        .find(|page| page.id == pane)
        .map(|page| page.title.clone())
        .unwrap_or_default();
    let background = if selected { ACTIVE_TAB } else { IDLE_TAB };
    Row(
        view.windows()
            .grip(Modifier::empty(), pane)
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
                        pages.update(|held| held.retain(|page| page.id != pane));
                        forget_movable(page_key(pane));
                    }),
                label_style(14.0, FADED_INK),
            );
        },
    );
}

#[composable]
fn NewTabButton(view: WindowView, pages: MutableState<Vec<Page>>, next: MutableState<u64>) {
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
                view.open_next_here(id);
                pages.update(|held| held.push(Page::new(id)));
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
                "press me, then drag this tab out of the strip",
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
pub(crate) const CHROME: Color = Color(0.13, 0.14, 0.17, 1.0);
const IDLE_TAB: Color = Color(0.20, 0.21, 0.25, 1.0);
const ACTIVE_TAB: Color = Color(0.32, 0.34, 0.40, 1.0);
pub(crate) const INK: Color = Color(0.96, 0.97, 1.0, 1.0);
const FADED_INK: Color = Color(0.70, 0.72, 0.80, 1.0);
