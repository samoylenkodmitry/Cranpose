//! Selecting the read-only text inside a
//! [`SelectionContainer`](crate::widgets::SelectionContainer).
//!
//! A container keeps a [`SelectionRegistrar`]. Every `Text` composed inside it
//! registers what it shows and where its content sits in the window, and draws
//! its part of the selection behind its glyphs. The container turns pointer
//! and key input into one [`TextSelection`] across those texts, in the order a
//! person reads them.

use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::{Rc, Weak},
};

use cranpose_core::MutableState;
use cranpose_ui_graphics::{Point, Rect, Size};

use crate::{
    text::{
        AnnotatedString, TextLayoutOptions, TextStyle, get_offset_for_position, measure_text,
        prepare_text_layout, text_align_fraction, wrapped_line_ranges,
    },
    text_selection::{
        MULTI_TAP_SLOP_PX, MULTI_TAP_TIMEOUT_MS, SelectionAnchor, SelectionGranularity,
        classify_tap_count, granularity_boundaries, tap_selection_granularity,
    },
};

/// Hold before a resting touch selects the word under it.
const LONG_PRESS_MS: u64 = 500;
/// Travel before the hold ends that makes a touch a scroll rather than a
/// long press.
const LONG_PRESS_SLOP: f32 = 12.0;

/// One end of a selection: a byte offset into one registered text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionEdge {
    /// The registered text the edge is in.
    pub selectable: u64,
    /// Byte offset into that text.
    pub offset: usize,
}

/// A selection across the texts of one container. `anchor` stays where the
/// gesture started and `focus` follows the pointer, so either may come first
/// in reading order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSelection {
    /// Where the selection started.
    pub anchor: SelectionEdge,
    /// Where the selection ends now.
    pub focus: SelectionEdge,
}

/// Where a registered text's content sits in the window: layout writes the
/// node's origin, slice collection the padding in front of the text, and the
/// text's highlight the size its content draws in.
#[derive(Default)]
pub(crate) struct SelectableGeometry {
    node_origin: Rc<Cell<Point>>,
    content_offset: Cell<Point>,
    content_size: Cell<Size>,
}

impl SelectableGeometry {
    pub(crate) fn node_origin_sink(&self) -> Rc<Cell<Point>> {
        Rc::clone(&self.node_origin)
    }

    pub(crate) fn set_content_offset(&self, offset: Point) {
        self.content_offset.set(offset);
    }

    pub(crate) fn set_content_size(&self, size: Size) {
        self.content_size.set(size);
    }

    fn rect(&self) -> Rect {
        let origin = self.node_origin.get();
        let offset = self.content_offset.get();
        let size = self.content_size.get();
        Rect {
            x: origin.x + offset.x,
            y: origin.y + offset.y,
            width: size.width,
            height: size.height,
        }
    }
}

/// One visual line of a registered text, in its content's coordinates.
struct SelectableLine {
    range: Range<usize>,
    left: f32,
    top: f32,
}

struct Selectable {
    key: u64,
    text: Rc<AnnotatedString>,
    style: TextStyle,
    options: TextLayoutOptions,
    geometry: Rc<SelectableGeometry>,
}

impl Selectable {
    fn len(&self) -> usize {
        self.text.text.len()
    }

    fn wrap_width(&self) -> Option<f32> {
        let width = self.geometry.content_size.get().width;
        (width > 0.0).then_some(width)
    }

    fn line_height(&self) -> f32 {
        prepare_text_layout(&self.text, &self.style, self.options, self.wrap_width())
            .metrics
            .line_height
    }

    /// The visual lines as the renderer lays them out: wrapped at the content
    /// width and aligned line by line.
    fn lines(&self, line_height: f32) -> Vec<SelectableLine> {
        let width = self.geometry.content_size.get().width;
        let fraction = text_align_fraction(&self.style, &self.text.text);
        wrapped_line_ranges(
            None,
            &self.text,
            &self.style,
            self.options,
            self.wrap_width(),
        )
        .into_iter()
        .enumerate()
        .map(|(index, range)| {
            let line_width = self.prefix_width(range.start, range.end);
            SelectableLine {
                left: (width - line_width).max(0.0) * fraction,
                top: index as f32 * line_height,
                range,
            }
        })
        .collect()
    }

    fn prefix_width(&self, start: usize, end: usize) -> f32 {
        measure_text(
            &AnnotatedString::from(&self.text.text[start..end]),
            &self.style,
        )
        .width
    }

    /// The byte offset nearest `local`, a point in the content's coordinates.
    fn offset_at(&self, local: Point) -> usize {
        let line_height = self.line_height();
        let lines = self.lines(line_height);
        let Some(last) = lines.len().checked_sub(1) else {
            return 0;
        };
        let index = if line_height > 0.0 {
            ((local.y / line_height).floor().max(0.0) as usize).min(last)
        } else {
            0
        };
        let line = &lines[index];
        let text = &self.text.text[line.range.clone()];
        let within = get_offset_for_position(
            &AnnotatedString::from(text),
            &self.style,
            local.x - line.left,
            0.0,
        );
        line.range.start + within.min(text.len())
    }

    /// The highlight for bytes `start..end`, a rectangle per visual line, in
    /// the content's coordinates.
    fn highlight(&self, start: usize, end: usize) -> Vec<Rect> {
        let line_height = self.line_height();
        self.lines(line_height)
            .into_iter()
            .filter(|line| start < line.range.end && end > line.range.start)
            .filter_map(|line| {
                let from = start.max(line.range.start);
                let to = end.min(line.range.end);
                let x0 = line.left + self.prefix_width(line.range.start, from);
                let x1 = line.left + self.prefix_width(line.range.start, to);
                (x1 > x0).then_some(Rect {
                    x: x0,
                    y: line.top,
                    width: x1 - x0,
                    height: line_height,
                })
            })
            .collect()
    }
}

struct RegistrarInner {
    next_key: Cell<u64>,
    selectables: RefCell<Vec<Selectable>>,
    selection: MutableState<Option<TextSelection>>,
}

/// The texts of one [`SelectionContainer`](crate::widgets::SelectionContainer)
/// and the selection across them.
#[derive(Clone)]
pub struct SelectionRegistrar {
    inner: Rc<RegistrarInner>,
}

impl PartialEq for SelectionRegistrar {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl SelectionRegistrar {
    pub(crate) fn new(selection: MutableState<Option<TextSelection>>) -> Self {
        Self {
            inner: Rc::new(RegistrarInner {
                next_key: Cell::new(1),
                selectables: RefCell::new(Vec::new()),
                selection,
            }),
        }
    }

    /// A key for a text joining the container.
    pub(crate) fn subscribe(&self) -> u64 {
        let key = self.inner.next_key.get();
        self.inner.next_key.set(key + 1);
        key
    }

    /// What the text under `key` shows and where it draws.
    pub(crate) fn update(
        &self,
        key: u64,
        text: Rc<AnnotatedString>,
        style: TextStyle,
        options: TextLayoutOptions,
        geometry: Rc<SelectableGeometry>,
    ) {
        let entry = Selectable {
            key,
            text,
            style,
            options,
            geometry,
        };
        let mut selectables = self.inner.selectables.borrow_mut();
        match selectables.iter_mut().find(|held| held.key == key) {
            Some(held) => *held = entry,
            None => selectables.push(entry),
        }
    }

    /// The text under `key` left the container; a selection with an end in it
    /// goes with it.
    pub(crate) fn unsubscribe(&self, key: u64) {
        self.inner
            .selectables
            .borrow_mut()
            .retain(|held| held.key != key);
        let touched = self
            .inner
            .selection
            .get_non_reactive()
            .is_some_and(|selection| {
                selection.anchor.selectable == key || selection.focus.selectable == key
            });
        if touched {
            self.inner.selection.set(None);
        }
    }

    /// The current selection. Reading it subscribes the reader.
    pub fn selection(&self) -> Option<TextSelection> {
        self.inner.selection.get()
    }

    pub(crate) fn set_selection(&self, selection: Option<TextSelection>) {
        self.inner.selection.set(selection);
    }

    /// Keys in reading order: top to bottom, then left to right, in the order
    /// the texts joined where their places tie.
    fn reading_order(&self) -> Vec<u64> {
        let selectables = self.inner.selectables.borrow();
        let mut order: Vec<(u64, Rect)> = selectables
            .iter()
            .map(|held| (held.key, held.geometry.rect()))
            .collect();
        order.sort_by(|(_, a), (_, b)| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
        order.into_iter().map(|(key, _)| key).collect()
    }

    fn with_selectable<R>(&self, key: u64, read: impl FnOnce(&Selectable) -> R) -> Option<R> {
        self.inner
            .selectables
            .borrow()
            .iter()
            .find(|held| held.key == key)
            .map(read)
    }

    /// The edge a pointer at window point `point` selects to: inside a text,
    /// the offset under it; beside the texts of a row, the nearest of them;
    /// between rows, the start of the next text; past every text, the end of
    /// the last.
    pub(crate) fn edge_at(&self, point: Point) -> Option<SelectionEdge> {
        let order = self.reading_order();
        let selectables = self.inner.selectables.borrow();
        let find = |key: u64| selectables.iter().find(|held| held.key == key);
        let row: Vec<&Selectable> = order
            .iter()
            .filter_map(|key| find(*key))
            .filter(|held| {
                let rect = held.geometry.rect();
                point.y >= rect.y && point.y <= rect.y + rect.height
            })
            .collect();
        let horizontal_gap = |held: &&Selectable| {
            let rect = held.geometry.rect();
            (rect.x - point.x)
                .max(point.x - (rect.x + rect.width))
                .max(0.0)
        };
        if let Some(held) = row
            .iter()
            .min_by(|a, b| horizontal_gap(a).total_cmp(&horizontal_gap(b)))
        {
            let rect = held.geometry.rect();
            return Some(SelectionEdge {
                selectable: held.key,
                offset: held.offset_at(Point {
                    x: point.x - rect.x,
                    y: point.y - rect.y,
                }),
            });
        }
        let next = order
            .iter()
            .filter_map(|key| find(*key))
            .find(|held| point.y < held.geometry.rect().y);
        if let Some(held) = next {
            return Some(SelectionEdge {
                selectable: held.key,
                offset: 0,
            });
        }
        order
            .last()
            .and_then(|key| find(*key))
            .map(|held| SelectionEdge {
                selectable: held.key,
                offset: held.len(),
            })
    }

    /// The selection's two edges in reading order.
    fn ordered(&self, selection: TextSelection) -> Option<(SelectionEdge, SelectionEdge)> {
        let order = self.reading_order();
        let rank = |edge: SelectionEdge| {
            order
                .iter()
                .position(|key| *key == edge.selectable)
                .map(|index| (index, edge.offset))
        };
        let (anchor, focus) = (rank(selection.anchor)?, rank(selection.focus)?);
        Some(if anchor <= focus {
            (selection.anchor, selection.focus)
        } else {
            (selection.focus, selection.anchor)
        })
    }

    /// The bytes of the text under `key` that `selection` covers.
    fn covered(&self, selection: TextSelection, key: u64) -> Option<Range<usize>> {
        let (first, last) = self.ordered(selection)?;
        let order = self.reading_order();
        let index = |key: u64| order.iter().position(|held| *held == key);
        let (at, from, to) = (
            index(key)?,
            index(first.selectable)?,
            index(last.selectable)?,
        );
        if at < from || at > to {
            return None;
        }
        let len = self.with_selectable(key, Selectable::len)?;
        let start = if key == first.selectable {
            first.offset
        } else {
            0
        };
        let end = if key == last.selectable {
            last.offset
        } else {
            len
        };
        (start < end).then_some(start..end.min(len))
    }

    /// The highlight the text under `key` draws for the current selection, in
    /// its content's coordinates. Reading it subscribes the reader.
    pub(crate) fn highlight(&self, key: u64) -> Vec<Rect> {
        let Some(selection) = self.selection() else {
            return Vec::new();
        };
        let Some(range) = self.covered(selection, key) else {
            return Vec::new();
        };
        self.with_selectable(key, |held| held.highlight(range.start, range.end))
            .unwrap_or_default()
    }

    /// The window rectangle of the selection's first highlighted line, where a
    /// menu for it belongs.
    pub(crate) fn first_highlight_in_window(&self) -> Option<Rect> {
        let selection = self.inner.selection.get_non_reactive()?;
        let (first, _) = self.ordered(selection)?;
        let range = self.covered(selection, first.selectable)?;
        self.with_selectable(first.selectable, |held| {
            let rect = held.geometry.rect();
            held.highlight(range.start, range.end)
                .first()
                .map(|line| Rect {
                    x: rect.x + line.x,
                    y: rect.y + line.y,
                    width: line.width,
                    height: line.height,
                })
        })
        .flatten()
    }

    /// What is selected, the texts joined by line breaks in reading order.
    pub fn selected_text(&self) -> String {
        let Some(selection) = self.inner.selection.get_non_reactive() else {
            return String::new();
        };
        self.reading_order()
            .into_iter()
            .filter_map(|key| {
                let range = self.covered(selection, key)?;
                self.with_selectable(key, |held| held.text.text[range].to_string())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Selects every registered text from the first to the last.
    pub(crate) fn select_all(&self) {
        let order = self.reading_order();
        let (Some(first), Some(last)) = (order.first(), order.last()) else {
            return;
        };
        let Some(len) = self.with_selectable(*last, Selectable::len) else {
            return;
        };
        self.set_selection(Some(TextSelection {
            anchor: SelectionEdge {
                selectable: *first,
                offset: 0,
            },
            focus: SelectionEdge {
                selectable: *last,
                offset: len,
            },
        }));
    }

    /// The unit under `edge` that `granularity` selects.
    fn unit_at(&self, edge: SelectionEdge, granularity: SelectionGranularity) -> Option<Unit> {
        self.with_selectable(edge.selectable, |held| Unit {
            selectable: edge.selectable,
            anchor: SelectionAnchor::at(&held.text.text, edge.offset, granularity),
        })
    }

    /// The selection a drag from the pressed `unit` to `edge` makes: the
    /// pressed unit together with the unit under `edge`, anchored at the far
    /// side of the pressed unit.
    fn dragged(&self, unit: Unit, edge: SelectionEdge) -> Option<TextSelection> {
        let granularity = unit.anchor.granularity;
        if edge.selectable == unit.selectable {
            let range = self.with_selectable(edge.selectable, |held| {
                unit.anchor.dragged_to(&held.text.text, edge.offset)
            })?;
            return Some(TextSelection {
                anchor: SelectionEdge {
                    selectable: edge.selectable,
                    offset: range.start,
                },
                focus: SelectionEdge {
                    selectable: edge.selectable,
                    offset: range.end,
                },
            });
        }
        let (unit_start, unit_end) = self.with_selectable(edge.selectable, |held| {
            granularity_boundaries(&held.text.text, edge.offset, granularity)
        })?;
        let pressed_start = SelectionEdge {
            selectable: unit.selectable,
            offset: unit.anchor.start,
        };
        let forward = self
            .ordered(TextSelection {
                anchor: pressed_start,
                focus: edge,
            })
            .is_some_and(|(first, _)| first == pressed_start);
        Some(if forward {
            TextSelection {
                anchor: pressed_start,
                focus: SelectionEdge {
                    selectable: edge.selectable,
                    offset: unit_end,
                },
            }
        } else {
            TextSelection {
                anchor: SelectionEdge {
                    selectable: unit.selectable,
                    offset: unit.anchor.end,
                },
                focus: SelectionEdge {
                    selectable: edge.selectable,
                    offset: unit_start,
                },
            }
        })
    }
}

/// What a press selected in one text, grown by a drag in the same unit.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Unit {
    selectable: u64,
    anchor: SelectionAnchor,
}

impl Unit {
    fn selection(self) -> TextSelection {
        TextSelection {
            anchor: SelectionEdge {
                selectable: self.selectable,
                offset: self.anchor.start,
            },
            focus: SelectionEdge {
                selectable: self.selectable,
                offset: self.anchor.end,
            },
        }
    }
}

/// A pointer held down on the container.
#[derive(Clone, Copy)]
struct Press {
    start: Point,
    touch: bool,
    /// The unit the press selected, once it selects: at once for a mouse,
    /// after the hold for a touch.
    unit: Option<Unit>,
    /// A touch that moved before its hold ended: the finger is scrolling.
    released_to_scroll: bool,
}

thread_local! {
    /// The container a person last pressed: its selection is the one a copy
    /// or a select-all acts on, and a press in another container clears it.
    static ACTIVE: RefCell<Weak<RegistrarInner>> = const { RefCell::new(Weak::new()) };
}

/// Turns a container's pointer input into its selection.
pub(crate) struct SelectionGesture {
    registrar: SelectionRegistrar,
    press: Cell<Option<Press>>,
    last_tap: Cell<Option<(web_time::Instant, Point, u8)>>,
    frame_clock: cranpose_core::internal::FrameClock,
    long_press: RefCell<Option<cranpose_core::internal::FrameCallbackRegistration>>,
    long_press_start: Cell<Option<u64>>,
    menu_open: MutableState<bool>,
}

impl SelectionGesture {
    pub(crate) fn new(
        registrar: SelectionRegistrar,
        frame_clock: cranpose_core::internal::FrameClock,
        menu_open: MutableState<bool>,
    ) -> Self {
        Self {
            registrar,
            press: Cell::new(None),
            last_tap: Cell::new(None),
            frame_clock,
            long_press: RefCell::new(None),
            long_press_start: Cell::new(None),
            menu_open,
        }
    }

    /// Makes this container the one a copy acts on, clearing the selection of
    /// the container pressed before it.
    fn activate(&self) {
        ACTIVE.with(|active| {
            let previous = active.borrow().upgrade();
            if let Some(previous) = previous
                && !Rc::ptr_eq(&previous, &self.registrar.inner)
            {
                previous.selection.set(None);
            }
            *active.borrow_mut() = Rc::downgrade(&self.registrar.inner);
        });
    }

    fn is_active(&self) -> bool {
        ACTIVE.with(|active| {
            active
                .borrow()
                .upgrade()
                .is_some_and(|held| Rc::ptr_eq(&held, &self.registrar.inner))
        })
    }

    pub(crate) fn on_down(self: &Rc<Self>, event: &crate::PointerEvent) {
        self.activate();
        crate::text_field_focus::clear_focus();
        self.menu_open.set(false);
        let touch = event.source.is_touch_like();
        let point = event.global_position;
        let mut press = Press {
            start: point,
            touch,
            unit: None,
            released_to_scroll: false,
        };
        if touch {
            self.registrar.set_selection(None);
            self.press.set(Some(press));
            self.arm_long_press();
            return;
        }
        let granularity = tap_selection_granularity(self.tap_count(point));
        press.unit = self
            .registrar
            .edge_at(point)
            .and_then(|edge| self.registrar.unit_at(edge, granularity));
        let selection = press
            .unit
            .filter(|unit| unit.anchor.start < unit.anchor.end)
            .map(Unit::selection);
        self.registrar.set_selection(selection);
        self.press.set(Some(press));
    }

    fn tap_count(&self, point: Point) -> u8 {
        let now = web_time::Instant::now();
        let previous = self.last_tap.get();
        let count = classify_tap_count(
            previous.map(|(_, at, count)| (count, at.x, at.y)),
            previous.map_or(u128::MAX, |(time, _, _)| {
                now.duration_since(time).as_millis()
            }),
            point.x,
            point.y,
            MULTI_TAP_TIMEOUT_MS,
            MULTI_TAP_SLOP_PX,
        );
        self.last_tap.set(Some((now, point, count)));
        count
    }

    /// Grows the selection with the pointer. Says whether the move belongs to
    /// the selection, so the container can keep it from scrolling.
    pub(crate) fn on_move(&self, event: &crate::PointerEvent) -> bool {
        let Some(mut press) = self.press.get() else {
            return false;
        };
        let point = event.global_position;
        let Some(unit) = press.unit else {
            if press.touch && !press.released_to_scroll {
                let moved = (point.x - press.start.x)
                    .abs()
                    .max((point.y - press.start.y).abs());
                if moved > LONG_PRESS_SLOP {
                    press.released_to_scroll = true;
                    self.press.set(Some(press));
                    self.cancel_long_press();
                }
            }
            return false;
        };
        if let Some(selection) = self
            .registrar
            .edge_at(point)
            .and_then(|edge| self.registrar.dragged(unit, edge))
        {
            self.registrar.set_selection(Some(selection));
        }
        true
    }

    pub(crate) fn on_up(&self) {
        self.cancel_long_press();
        let press = self.press.take();
        let selected = self
            .registrar
            .selection()
            .is_some_and(|selection| selection.anchor != selection.focus);
        if press.is_some_and(|press| press.touch && press.unit.is_some()) && selected {
            self.menu_open.set(true);
        }
    }

    pub(crate) fn on_cancel(&self) {
        self.cancel_long_press();
        self.press.set(None);
    }

    fn arm_long_press(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let registration = self.frame_clock.with_frame_nanos(move |now| {
            if let Some(gesture) = weak.upgrade() {
                gesture.long_press_tick(now);
            }
        });
        *self.long_press.borrow_mut() = Some(registration);
        crate::request_render_invalidation();
    }

    fn cancel_long_press(&self) {
        self.long_press.borrow_mut().take();
        self.long_press_start.set(None);
    }

    fn long_press_tick(self: Rc<Self>, now: u64) {
        self.long_press.borrow_mut().take();
        let Some(mut press) = self.press.get() else {
            return;
        };
        if press.released_to_scroll || press.unit.is_some() {
            return;
        }
        let start = self.long_press_start.get().unwrap_or(now);
        self.long_press_start.set(Some(start));
        if now.saturating_sub(start) < LONG_PRESS_MS * 1_000_000 {
            self.arm_long_press();
            return;
        }
        press.unit = self
            .registrar
            .edge_at(press.start)
            .and_then(|edge| self.registrar.unit_at(edge, SelectionGranularity::Word));
        self.press.set(Some(press));
        self.registrar
            .set_selection(press.unit.map(Unit::selection));
        crate::request_render_invalidation();
    }

    /// Copy, select all and dismiss for the container a person last pressed.
    /// Says whether the key was one of them.
    pub(crate) fn on_key(&self, event: &crate::KeyEvent) -> bool {
        if !event.is_key_down() || !self.is_active() {
            return false;
        }
        let command = event.modifiers.command_or_ctrl();
        match event.key_code {
            crate::KeyCode::C if command => self.copy(),
            crate::KeyCode::A if command => {
                self.registrar.select_all();
                true
            }
            crate::KeyCode::Escape if self.registrar.selection().is_some() => {
                self.dismiss();
                true
            }
            _ => false,
        }
    }

    /// Puts the selection on the clipboard. Says whether there was one.
    pub(crate) fn copy(&self) -> bool {
        let text = self.registrar.selected_text();
        if text.is_empty() {
            return false;
        }
        crate::clipboard_session::clipboard_write_text(&text);
        true
    }

    /// Clears the selection and closes its menu.
    pub(crate) fn dismiss(&self) {
        self.registrar.set_selection(None);
        self.menu_open.set(false);
    }
}

#[cfg(test)]
#[path = "tests/selection_container_tests.rs"]
mod tests;
