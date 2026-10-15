//! A menu bar that Cranpose draws inside the window, for Linux, where the
//! system has no menu bar outside GTK windows. Compose Desktop draws its
//! Linux menu bar in the window too.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use cranpose_core::{LaunchedEffect, MutableState, SideEffect, remember, rememberMutableStateOf};
use cranpose_ui::{
    Box, BoxSpec, Color, Column, ColumnSpec, FocusRequester, IntrinsicSize, KeyCode, KeyEvent,
    KeyEventType, Modifier, Point, Rect, RoundedCornerShape, Row, RowSpec, Text, TextStyle,
    UnhandledKeyEvents, VerticalAlignment, composable, rememberMutableInteractionSource,
    text::SpanStyle, widgets::PopupDismissable,
};

use super::{KeyShortcut, Menu, MenuEntry};

const BAR: Color = Color::from_rgb_u8(240, 240, 240);
const HIGHLIGHT: Color = Color::from_rgb_u8(214, 220, 232);
const PANEL: Color = Color::WHITE;
const LINE: Color = Color::from_rgb_u8(200, 200, 200);
const DIMMED: Color = Color::from_rgb_u8(150, 150, 150);
const HINT: Color = Color::from_rgb_u8(110, 110, 110);
const PANEL_MIN_WIDTH: f32 = 200.0;
const MARK_WIDTH: f32 = 20.0;

/// The open menus: the bar's menu, then each open submenu's index in the
/// menu above it. Empty while every menu is closed.
type OpenPath = MutableState<Vec<usize>>;

/// A cell per bar title that receives its window rect.
type TitleRects = Rc<RefCell<Vec<Rc<Cell<Rect>>>>>;

/// The window rect of each menu line by its path, where a submenu opens.
type RowRects = Rc<RefCell<HashMap<Vec<usize>, Rc<Cell<Rect>>>>>;

/// Shows `menus` as a bar above `content`: a title opens its menu below it,
/// a click on an item runs its callback, a submenu opens beside its item,
/// and Escape or a click elsewhere closes the menus. While the pointer is
/// on the bar with a menu open, the menu under it opens. The items'
/// shortcuts run them for key presses in `content`. The desktop shell puts
/// [`super::MenuBar`]'s menus here on Linux.
#[composable(no_skip)]
pub fn WindowMenuBar<F>(menus: Vec<Menu>, content: F)
where
    F: FnMut() + 'static,
{
    bar_frame(Rc::new(menus), content);
}

/// The bar above `content`, with the key handling for both.
pub(super) fn bar_frame(menus: Rc<Vec<Menu>>, content: impl FnMut() + 'static) {
    let open: OpenPath = rememberMutableStateOf(Vec::new);
    let shortcuts = Rc::clone(&menus);
    let unfocused = Rc::clone(&menus);
    // A key a control in the content does not take, or one pressed with
    // nothing focused, runs the item with that shortcut, as Swing's menu
    // accelerators do.
    UnhandledKeyEvents(move |event| run_shortcut(&unfocused, open, event));
    let content = Rc::new(RefCell::new(content));
    Column(
        Modifier::empty()
            .fill_max_size()
            .on_preview_key_event(move |event| close_on_escape(open, event))
            .on_key_event(move |event| run_shortcut(&shortcuts, open, event)),
        ColumnSpec::default(),
        move || {
            if !menus.is_empty() {
                Bar(Rc::clone(&menus), open);
            }
            let content = Rc::clone(&content);
            Box(
                Modifier::empty().weight(1.0).fill_max_width(),
                BoxSpec::default(),
                move || (content.borrow_mut())(),
            );
        },
    );
}

/// Closes the open menus on Escape.
fn close_on_escape(open: OpenPath, event: &KeyEvent) -> bool {
    let closes = event.event_type == KeyEventType::KeyDown
        && event.key_code == KeyCode::Escape
        && open.with(|path| !path.is_empty());
    if closes {
        open.set(Vec::new());
    }
    closes
}

/// Runs the enabled item whose shortcut a key press matches.
fn run_shortcut(menus: &[Menu], open: OpenPath, event: &KeyEvent) -> bool {
    if event.event_type != KeyEventType::KeyDown {
        return false;
    }
    let pressed = KeyShortcut::new(event.key_code, event.modifiers);
    let Some(action) = menus
        .iter()
        .find_map(|menu| shortcut_action(&menu.entries, pressed))
    else {
        return false;
    };
    open.set(Vec::new());
    action();
    true
}

fn shortcut_action(entries: &[MenuEntry], pressed: KeyShortcut) -> Option<Rc<dyn Fn()>> {
    entries.iter().find_map(|entry| match entry {
        MenuEntry::Item(item) if item.enabled && item.shortcut == Some(pressed) => {
            Some(Rc::clone(&item.on_click))
        }
        MenuEntry::Submenu(menu) => shortcut_action(&menu.entries, pressed),
        MenuEntry::Item(_) | MenuEntry::Check(_) | MenuEntry::Separator => None,
    })
}

/// The bar, and while a menu is open, an overlay above the scrim that
/// closes the menus: a copy of the bar, whose titles still answer the
/// pointer, and every open menu, the bar's under its title and each
/// submenu beside its item. One overlay holds them all, so readers see
/// every open menu.
#[composable(no_skip)]
fn Bar(menus: Rc<Vec<Menu>>, open: OpenPath) {
    let bar_rect = remember(|| Rc::new(Cell::new(Rect::EMPTY))).with(Rc::clone);
    let title_rects: TitleRects = remember(|| Rc::new(RefCell::new(Vec::new()))).with(Rc::clone);
    let row_rects: RowRects = remember(|| Rc::new(RefCell::new(HashMap::new()))).with(Rc::clone);
    title_rects
        .borrow_mut()
        .resize_with(menus.len(), || Rc::new(Cell::new(Rect::EMPTY)));
    Titles(
        Rc::clone(&menus),
        open,
        Modifier::empty().report_window_rect(Rc::clone(&bar_rect)),
        Some(Rc::clone(&title_rects)),
    );
    let path = open.value();
    let Some(&index) = path.first() else {
        return;
    };
    let anchor = bar_rect.get();
    let title_x = title_rects
        .borrow()
        .get(index)
        .map_or(0.0, |title| title.get().x - anchor.x);
    PopupDismissable(
        anchor,
        Point::default(),
        move || open.set(Vec::new()),
        move || {
            let (menus, rows, path) = (Rc::clone(&menus), Rc::clone(&row_rects), path.clone());
            Box(
                Modifier::empty().fill_max_size(),
                BoxSpec::default(),
                move || {
                    Titles(
                        Rc::clone(&menus),
                        open,
                        Modifier::empty().width(anchor.width),
                        None,
                    );
                    let Some(menu) = menus.get(index) else {
                        return;
                    };
                    let mut entries = &menu.entries;
                    let mut position = Point::new(title_x, anchor.height);
                    for depth in 1..=path.len() {
                        let at = path[..depth].to_vec();
                        Panel(
                            Rc::new(entries.clone()),
                            at.clone(),
                            open,
                            Rc::clone(&rows),
                            position,
                        );
                        let Some(MenuEntry::Submenu(submenu)) =
                            path.get(depth).and_then(|&next| entries.get(next))
                        else {
                            break;
                        };
                        let mut item = at;
                        item.push(path[depth]);
                        let row = rows
                            .borrow()
                            .get(&item)
                            .map_or(Rect::EMPTY, |rect| rect.get());
                        position = Point::new(row.x + row.width - anchor.x, row.y - 4.0 - anchor.y);
                        entries = &submenu.entries;
                    }
                },
            );
        },
    );
}

/// The row of menu titles. `rects`, when given, holds a cell per title that
/// receives its window rect.
#[composable(no_skip)]
fn Titles(menus: Rc<Vec<Menu>>, open: OpenPath, modifier: Modifier, rects: Option<TitleRects>) {
    Row(
        modifier
            .fill_max_width()
            .background(BAR)
            .padding_horizontal(4.0),
        RowSpec::default(),
        move || {
            for (index, menu) in menus.iter().enumerate() {
                let rect = rects
                    .as_ref()
                    .and_then(|rects| rects.borrow().get(index).cloned());
                Title(index, menu.title.clone(), open, rect);
            }
        },
    );
}

/// One menu title of the bar.
#[composable(no_skip)]
fn Title(index: usize, title: String, open: OpenPath, rect: Option<Rc<Cell<Rect>>>) {
    let source = rememberMutableInteractionSource();
    let hovered = source.collectIsHoveredAsState().get();
    let is_open = open.with(|path| path.first() == Some(&index));
    if hovered && !is_open {
        // With a menu open, the pointer on another title opens that menu.
        SideEffect(move || {
            if open.with(|path| !path.is_empty() && path.first() != Some(&index)) {
                open.set(vec![index]);
            }
        });
    }
    let modifier = match rect {
        Some(rect) => Modifier::empty().report_window_rect(rect),
        None => Modifier::empty(),
    };
    Box(
        modifier
            .hoverable(source, true)
            .background(if is_open || hovered {
                HIGHLIGHT
            } else {
                Color::TRANSPARENT
            })
            .clickable(move |_| {
                open.set(if is_open { Vec::new() } else { vec![index] });
            })
            .padding_symmetric(10.0, 5.0),
        BoxSpec::default(),
        move || {
            Text(title.clone(), Modifier::empty(), TextStyle::default());
        },
    );
}

/// The entries of one open menu, at `path`, placed at `position` in the
/// overlay. Each line reports its window rect to `rows`.
#[composable(no_skip)]
fn Panel(
    entries: Rc<Vec<MenuEntry>>,
    path: Vec<usize>,
    open: OpenPath,
    rows: RowRects,
    position: Point,
) {
    // A menu that opens takes the keyboard focus on its first line that can
    // be chosen, as a native one does: Escape closes the menus, the arrow
    // keys move between the lines and Return chooses one.
    let first_line = remember(FocusRequester::new).with(Clone::clone);
    let focus = first_line.clone();
    LaunchedEffect(path.clone(), move |_| {
        let _ = focus.request_focus();
    });
    Column(
        Modifier::empty()
            .absolute_offset(position.x, position.y)
            .width_in(PANEL_MIN_WIDTH, f32::INFINITY)
            .width_intrinsic(IntrinsicSize::Max)
            .selectable_group()
            .background(PANEL)
            .border(1.0, LINE, RoundedCornerShape::uniform(0.0))
            .padding_symmetric(0.0, 4.0),
        ColumnSpec::default(),
        move || {
            let mut first_line = Some(first_line.clone());
            for (index, entry) in entries.iter().enumerate() {
                let mut at = path.clone();
                at.push(index);
                let (line, action): (Line, Rc<dyn Fn()>) = match entry {
                    MenuEntry::Item(item) => (
                        Line {
                            label: item.label.clone(),
                            hint: item.shortcut.map(shortcut_label),
                            enabled: item.enabled,
                            checked: false,
                            opens_submenu: false,
                        },
                        Rc::clone(&item.on_click),
                    ),
                    MenuEntry::Check(item) => {
                        let (change, checked) = (Rc::clone(&item.on_change), item.checked);
                        (
                            Line {
                                label: item.label.clone(),
                                hint: None,
                                enabled: item.enabled,
                                checked,
                                opens_submenu: false,
                            },
                            Rc::new(move || change(!checked)),
                        )
                    }
                    MenuEntry::Submenu(menu) => (
                        Line {
                            label: menu.title.clone(),
                            hint: Some("\u{25b8}".to_owned()),
                            enabled: true,
                            checked: false,
                            opens_submenu: true,
                        },
                        Rc::new(|| {}),
                    ),
                    MenuEntry::Separator => {
                        Box(
                            Modifier::empty()
                                .fill_max_width()
                                .padding_symmetric(0.0, 4.0)
                                .height(1.0)
                                .background(LINE),
                            BoxSpec::default(),
                            || {},
                        );
                        continue;
                    }
                };
                let focus = if line.enabled {
                    first_line.take()
                } else {
                    None
                };
                EntryRow(line, at, open, Rc::clone(&rows), action, focus);
            }
        },
    );
}

/// What one line of a menu shows.
#[derive(Clone)]
struct Line {
    label: String,
    hint: Option<String>,
    enabled: bool,
    /// Shows the check mark in the line's leading column.
    checked: bool,
    opens_submenu: bool,
}

/// One line of a menu at `path`: an item that runs `action` and closes the
/// menus, or a line that opens its submenu beside it. Its window rect goes
/// to `rows`, where the submenu finds its place.
#[composable(no_skip)]
fn EntryRow(
    line: Line,
    path: Vec<usize>,
    open: OpenPath,
    rows: RowRects,
    action: Rc<dyn Fn()>,
    focus: Option<FocusRequester>,
) {
    let Line {
        label,
        hint,
        enabled,
        checked,
        opens_submenu,
    } = line;
    let source = rememberMutableInteractionSource();
    let hovered = enabled && source.collectIsHoveredAsState().get();
    let rect = remember(|| Rc::new(Cell::new(Rect::EMPTY))).with(Rc::clone);
    rows.borrow_mut().insert(path.clone(), Rc::clone(&rect));
    let is_open = opens_submenu && open.with(|current| current.starts_with(&path));
    if hovered && !is_open {
        // The pointer on a line closes the submenus below its menu, and on
        // a submenu's line opens it, while the line's menu is open.
        let parent = path.len() - 1;
        let depth = if opens_submenu { path.len() } else { parent };
        let target = path[..depth].to_vec();
        let menu = path[..parent].to_vec();
        SideEffect(move || {
            if open.with(|current| current.starts_with(&menu) && *current != target) {
                open.set(target.clone());
            }
        });
    }
    let clicked = move |_| {
        if !enabled {
            return;
        }
        if opens_submenu {
            open.set(path.clone());
        } else {
            open.set(Vec::new());
            action();
        }
    };
    let text = if enabled { Color::BLACK } else { DIMMED };
    let modifier = match &focus {
        Some(focus) => Modifier::empty().focus_requester(focus),
        None => Modifier::empty(),
    };
    Row(
        modifier
            .fill_max_width()
            .report_window_rect(rect)
            .hoverable(source, enabled)
            .background(if hovered || is_open {
                HIGHLIGHT
            } else {
                Color::TRANSPARENT
            })
            .clickable(clicked)
            .padding_symmetric(12.0, 5.0),
        RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            // Every line keeps the leading column check marks show in.
            Box(
                Modifier::empty().width(MARK_WIDTH),
                BoxSpec::default(),
                move || {
                    if checked {
                        Text("\u{2713}", Modifier::empty(), text_style(text));
                    }
                },
            );
            Text(
                label.clone(),
                Modifier::empty().weight(1.0),
                text_style(text),
            );
            if let Some(hint) = &hint {
                Text(
                    hint.clone(),
                    Modifier::empty().padding_each(24.0, 0.0, 0.0, 0.0),
                    text_style(HINT),
                );
            }
        },
    );
}

fn text_style(color: Color) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            ..SpanStyle::default()
        },
        ..TextStyle::default()
    }
}

/// The text a menu shows for `shortcut`, as `Ctrl+Shift+K`.
fn shortcut_label(shortcut: KeyShortcut) -> String {
    let modifiers = shortcut.modifiers;
    let mut label = String::new();
    for (held, name) in [
        (modifiers.ctrl, "Ctrl+"),
        (modifiers.shift, "Shift+"),
        (modifiers.alt, "Alt+"),
        (modifiers.meta, "Super+"),
    ] {
        if held {
            label.push_str(name);
        }
    }
    let key = format!("{:?}", shortcut.key);
    label.push_str(key.strip_prefix("Digit").unwrap_or(&key));
    label
}
