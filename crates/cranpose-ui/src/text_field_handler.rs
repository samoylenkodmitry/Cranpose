use std::{cell::Cell, rc::Rc};

use cranpose_foundation::text::{TextFieldLineLimits, TextFieldState};
use cranpose_ui_graphics::Point;

use crate::{
    text::{AnnotatedString, TextStyle, measure_text},
    text_field_focus::ImeCaretGeometry,
    text_field_input::handle_key_event_impl,
};

#[derive(Clone)]
pub(crate) struct CaretGeometryRefs {
    pub node_origin: Rc<Cell<Point>>,
    pub content_offset: Rc<Cell<f32>>,
    pub content_y_offset: Rc<Cell<f32>>,
    pub scroll_offset: Rc<Cell<f32>>,
    pub style: TextStyle,
}

pub(crate) struct TextFieldHandler {
    state: TextFieldState,
    node_id: Option<cranpose_core::NodeId>,
    line_limits: TextFieldLineLimits,
    geometry: CaretGeometryRefs,
}

impl TextFieldHandler {
    pub(crate) fn new(
        state: TextFieldState,
        node_id: Option<cranpose_core::NodeId>,
        line_limits: TextFieldLineLimits,
        geometry: CaretGeometryRefs,
    ) -> Rc<Self> {
        Rc::new(Self {
            state,
            node_id,
            line_limits,
            geometry,
        })
    }

    fn on_text_mutated(&self) {
        crate::cursor_animation::reset_cursor_blink();
        if let Some(node_id) = self.node_id {
            crate::schedule_layout_repass(node_id);
        }
        crate::request_render_invalidation();
    }
}

impl crate::text_field_focus::FocusedTextFieldHandler for TextFieldHandler {
    fn node_id(&self) -> Option<cranpose_core::NodeId> {
        self.node_id
    }

    fn handle_key(&self, event: &crate::key_event::KeyEvent) -> bool {
        use crate::key_event::KeyEventType;

        if event.event_type != KeyEventType::KeyDown {
            return false;
        }

        let consumed = handle_key_event_impl(&self.state, event, self.line_limits);

        if consumed {
            self.on_text_mutated();
        }

        consumed
    }

    fn insert_text(&self, text: &str) {
        self.state.edit(|buffer| {
            buffer.insert(text);
        });
        self.on_text_mutated();
    }

    fn delete_surrounding(&self, before_bytes: usize, after_bytes: usize) {
        if before_bytes == 0 && after_bytes == 0 {
            return;
        }

        self.state.edit(|buffer| {
            buffer.delete_surrounding(before_bytes, after_bytes);
        });
        self.state.set_desired_column(None);
        self.on_text_mutated();
    }

    fn copy_selection(&self) -> Option<String> {
        let value = self.state.value();
        let selection = value.selection;

        if selection.collapsed() {
            return None;
        }

        let text = selection.safe_slice(&value.text);
        if text.is_empty() {
            return None;
        }

        Some(text.to_string())
    }

    fn cut_selection(&self) -> Option<String> {
        let value = self.state.value();
        let selection = value.selection;

        if selection.collapsed() {
            return None;
        }

        let text = selection.safe_slice(&value.text);
        if text.is_empty() {
            return None;
        }

        let text = text.to_string();
        self.state.edit(|buffer| {
            buffer.delete(selection);
        });
        self.on_text_mutated();
        Some(text)
    }

    fn set_composition(&self, text: &str, cursor: Option<(usize, usize)>) {
        self.state.edit(|buffer| {
            if text.is_empty() {
                if let Some(range) = buffer.composition() {
                    buffer.delete(range);
                }
                buffer.set_composition(None);
            } else {
                let target = buffer.composition().unwrap_or_else(|| buffer.selection());
                let insert_pos = target.min();
                let comp_end = insert_pos + text.len();

                buffer.replace(target, text);

                let comp_range = cranpose_foundation::text::TextRange::new(insert_pos, comp_end);
                buffer.set_composition(Some(comp_range));

                if let Some((cursor_start, _cursor_end)) = cursor {
                    let cursor_pos = insert_pos + cursor_start.min(text.len());
                    buffer.place_cursor_before_char(cursor_pos);
                }
            }
        });

        self.on_text_mutated();
    }

    fn finish_composition(&self) {
        if self.state.composition().is_none() {
            return;
        }
        self.state.edit(|buffer| {
            buffer.set_composition(None);
        });
        crate::request_render_invalidation();
    }

    fn set_composing_region(&self, start_bytes: usize, end_bytes: usize) {
        self.state.edit(|buffer| {
            let text = buffer.text();
            let start = floor_char_boundary(text, start_bytes.min(end_bytes));
            let end = floor_char_boundary(text, start_bytes.max(end_bytes));
            if start == end {
                buffer.set_composition(None);
            } else {
                buffer.set_composition(Some(cranpose_foundation::text::TextRange::new(start, end)));
            }
        });
        crate::request_render_invalidation();
    }

    fn select_all(&self) {
        self.state.edit(|buffer| buffer.select_all());
        crate::request_render_invalidation();
    }

    fn set_selection(&self, start_bytes: usize, end_bytes: usize) {
        let text = self.state.value().text;
        let start = floor_char_boundary(&text, start_bytes.min(end_bytes));
        let end = floor_char_boundary(&text, start_bytes.max(end_bytes));
        self.state
            .set_selection(cranpose_foundation::text::TextRange::new(start, end));
        crate::cursor_animation::reset_cursor_blink();
        crate::request_render_invalidation();
    }

    fn editor_state(&self) -> Option<crate::text_field_focus::ImeEditorState> {
        let value = self.state.value();
        Some(crate::text_field_focus::ImeEditorState {
            text: value.text.clone(),
            selection_start: value.selection.min(),
            selection_end: value.selection.max(),
            composition: value.composition.map(|range| (range.min(), range.max())),
            single_line: matches!(self.line_limits, TextFieldLineLimits::SingleLine),
        })
    }

    fn caret_geometry(&self) -> Option<ImeCaretGeometry> {
        let value = self.state.value();
        let text = value.text.as_str();
        if text.contains('\n') {
            return None;
        }
        let g = &self.geometry;
        let origin = g.node_origin.get();
        let base_x = origin.x + g.content_offset.get() - g.scroll_offset.get();
        let top = origin.y + g.content_y_offset.get();
        let line_height = measure_text(&AnnotatedString::from("Ag"), &g.style).line_height;

        let mut caret_xs = Vec::with_capacity(text.len() + 1);
        caret_xs.push(base_x);
        let mut byte = 0usize;
        for ch in text.chars() {
            byte += ch.len_utf8();
            let advance = measure_text(&AnnotatedString::from(&text[..byte]), &g.style).width;
            caret_xs.push(base_x + advance);
        }
        Some(ImeCaretGeometry {
            caret_xs,
            top,
            line_height,
        })
    }
}

fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, sync::Arc};

    use cranpose_core::{DefaultScheduler, Runtime};

    use super::*;
    use crate::{
        key_event::{KeyCode, KeyEvent, KeyEventType, Modifiers},
        text_field_focus,
    };

    fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
        let _runtime = Runtime::new(Arc::new(DefaultScheduler));
        f()
    }

    fn key_down(key_code: KeyCode, text: &str) -> KeyEvent {
        KeyEvent::new(key_code, text, Modifiers::NONE, KeyEventType::KeyDown)
    }

    fn focused_state(
        initial: &str,
        line_limits: TextFieldLineLimits,
    ) -> (TextFieldState, Rc<RefCell<bool>>) {
        let state = TextFieldState::new(initial);
        let handler = TextFieldHandler::new(
            state,
            None,
            line_limits,
            CaretGeometryRefs {
                node_origin: Rc::new(Cell::new(Point { x: 0.0, y: 0.0 })),
                content_offset: Rc::new(Cell::new(0.0)),
                content_y_offset: Rc::new(Cell::new(0.0)),
                scroll_offset: Rc::new(Cell::new(0.0)),
                style: TextStyle::default(),
            },
        );
        let focus = Rc::new(RefCell::new(false));
        text_field_focus::request_focus(focus.clone(), handler, 0);
        (state, focus)
    }

    #[test]
    fn caret_geometry_exposed_for_single_visual_line() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (_state, focus) = focused_state(
                "abc",
                TextFieldLineLimits::MultiLine {
                    min_lines: 1,
                    max_lines: 3,
                },
            );
            let geom = text_field_focus::focused_caret_geometry().expect("caret geometry");
            assert_eq!(geom.caret_xs.len(), "abc".chars().count() + 1);
            drop(focus);
        });

        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (_state, focus) = focused_state(
                "a\nb",
                TextFieldLineLimits::MultiLine {
                    min_lines: 1,
                    max_lines: 3,
                },
            );
            assert!(text_field_focus::focused_caret_geometry().is_none());
            drop(focus);
        });
    }

    #[test]
    fn android_style_key_events_commit_text() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state(
                "",
                TextFieldLineLimits::MultiLine {
                    min_lines: 1,
                    max_lines: usize::MAX,
                },
            );

            assert!(text_field_focus::dispatch_key_event(&key_down(
                KeyCode::H,
                "h"
            )));
            assert!(text_field_focus::dispatch_key_event(&key_down(
                KeyCode::I,
                "i"
            )));
            assert!(text_field_focus::dispatch_key_event(&key_down(
                KeyCode::Unknown,
                "\u{00f6}"
            )));
            assert_eq!(state.text(), "hi\u{00f6}");

            let key_up = KeyEvent::new(KeyCode::H, "h", Modifiers::NONE, KeyEventType::KeyUp);
            assert!(!text_field_focus::dispatch_key_event(&key_up));
            assert_eq!(state.text(), "hi\u{00f6}");

            assert!(text_field_focus::dispatch_key_event(&key_down(
                KeyCode::Backspace,
                ""
            )));
            assert_eq!(state.text(), "hi");

            assert!(text_field_focus::dispatch_key_event(&key_down(
                KeyCode::Enter,
                ""
            )));
            assert_eq!(state.text(), "hi\n");

            text_field_focus::clear_focus();
        });
    }

    #[test]
    fn select_all_dispatch_selects_entire_field() {
        use cranpose_foundation::text::TextRange;
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state("hello world", TextFieldLineLimits::SingleLine);
            state.edit(|buffer| buffer.place_cursor_at_end());

            assert!(text_field_focus::dispatch_select_all());
            assert_eq!(state.selection(), TextRange::new(0, "hello world".len()));

            text_field_focus::clear_focus();
            assert!(!text_field_focus::dispatch_select_all());
        });
    }

    #[test]
    fn clipboard_fallback_round_trips() {
        use crate::clipboard_session::{clipboard_read_text, clipboard_write_text};
        let _app_context = crate::render_state::app_context_test_scope();
        assert_eq!(clipboard_read_text(), None);
        clipboard_write_text("copied text");
        assert_eq!(clipboard_read_text(), Some("copied text".to_string()));
    }

    #[test]
    fn spacebar_swipe_arrow_keys_scrub_caret() {
        use cranpose_foundation::text::TextRange;
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state("hello", TextFieldLineLimits::SingleLine);
            state.edit(|buffer| buffer.place_cursor_at_end());
            assert_eq!(state.selection(), TextRange::new(5, 5));

            assert!(text_field_focus::dispatch_key_event(&key_down(
                KeyCode::ArrowLeft,
                ""
            )));
            assert_eq!(state.selection(), TextRange::new(4, 4));
            assert!(text_field_focus::dispatch_key_event(&key_down(
                KeyCode::ArrowLeft,
                ""
            )));
            assert_eq!(state.selection(), TextRange::new(3, 3));

            assert!(text_field_focus::dispatch_key_event(&key_down(
                KeyCode::ArrowRight,
                ""
            )));
            assert_eq!(state.selection(), TextRange::new(4, 4));

            assert_eq!(state.text(), "hello");
            text_field_focus::clear_focus();
        });
    }

    #[test]
    fn cursor_move_via_set_selection_resets_blink_to_solid() {
        use crate::cursor_animation::{BLINK_INTERVAL_MS, is_cursor_visible, start_cursor_blink};
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state("hello world", TextFieldLineLimits::SingleLine);
            state.edit(|buffer| buffer.place_cursor_at_end());

            start_cursor_blink();
            let hidden_at =
                web_time::Instant::now() + std::time::Duration::from_millis(BLINK_INTERVAL_MS + 1);
            crate::render_state::with_cursor_animation(|state| state.tick(hidden_at));
            assert!(!is_cursor_visible(), "caret should be hidden mid-blink");

            assert!(text_field_focus::dispatch_ime_set_selection(2, 2));
            assert!(
                is_cursor_visible(),
                "a cursor-move must reset the blink phase to solid (visible)"
            );

            text_field_focus::clear_focus();
        });
    }

    #[test]
    fn ime_set_selection_scrubs_caret_without_editing() {
        use cranpose_foundation::text::TextRange;
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state("hello world", TextFieldLineLimits::SingleLine);
            state.edit(|buffer| buffer.place_cursor_at_end());

            assert!(text_field_focus::dispatch_ime_set_selection(2, 2));
            assert_eq!(state.selection(), TextRange::new(2, 2));
            assert_eq!(state.text(), "hello world");

            assert!(text_field_focus::dispatch_ime_set_selection(0, 5));
            assert_eq!(state.selection(), TextRange::new(0, 5));

            text_field_focus::clear_focus();
            assert!(!text_field_focus::dispatch_ime_set_selection(1, 1));
        });
    }

    #[test]
    fn successive_preedit_updates_replace_previous_composition() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state("ab", TextFieldLineLimits::SingleLine);
            state.edit(|buffer| buffer.place_cursor_at_end());

            assert!(text_field_focus::dispatch_ime_preedit("k", Some((1, 1))));
            assert_eq!(state.text(), "abk");
            assert!(text_field_focus::dispatch_ime_preedit("ka", Some((2, 2))));
            assert_eq!(state.text(), "abka");
            assert!(text_field_focus::dispatch_ime_preedit(
                "\u{304b}",
                Some((3, 3))
            ));
            assert_eq!(state.text(), "ab\u{304b}");
            let comp = state.composition().expect("composition active");
            assert_eq!((comp.min(), comp.max()), (2, 2 + "\u{304b}".len()));

            assert!(text_field_focus::dispatch_ime_preedit("", None));
            assert_eq!(state.text(), "ab");
            assert!(text_field_focus::dispatch_paste("\u{304b}\u{306a}"));
            assert_eq!(state.text(), "ab\u{304b}\u{306a}");
            assert_eq!(state.composition(), None);

            text_field_focus::clear_focus();
        });
    }

    #[test]
    fn finish_composition_keeps_text() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state("", TextFieldLineLimits::SingleLine);

            assert!(text_field_focus::dispatch_ime_preedit("hello", None));
            assert_eq!(state.text(), "hello");
            assert!(state.composition().is_some());

            assert!(text_field_focus::dispatch_ime_finish_composing());
            assert_eq!(state.text(), "hello");
            assert_eq!(state.composition(), None);

            assert!(text_field_focus::dispatch_ime_finish_composing());
            assert_eq!(state.text(), "hello");

            text_field_focus::clear_focus();
        });
    }

    #[test]
    fn set_composing_region_marks_text_without_changing_it() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state("hello world", TextFieldLineLimits::SingleLine);

            assert!(text_field_focus::dispatch_ime_set_composing_region(0, 5));
            assert_eq!(state.text(), "hello world");
            let comp = state.composition().expect("composition active");
            assert_eq!((comp.min(), comp.max()), (0, 5));

            assert!(text_field_focus::dispatch_ime_preedit("Hello", None));
            assert_eq!(state.text(), "Hello world");

            assert!(text_field_focus::dispatch_ime_preedit("", None));
            state.edit(|buffer| {
                buffer.clear();
                buffer.insert("\u{00e9}x");
            });
            assert!(text_field_focus::dispatch_ime_set_composing_region(1, 3));
            let comp = state.composition().expect("composition active");
            assert_eq!((comp.min(), comp.max()), (0, 3));

            assert!(text_field_focus::dispatch_ime_set_composing_region(1, 1));
            assert_eq!(state.composition(), None);
            assert_eq!(state.text(), "\u{00e9}x");

            text_field_focus::clear_focus();
        });
    }

    #[test]
    fn editor_state_reflects_text_selection_and_composition() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state("hi", TextFieldLineLimits::SingleLine);
            state.edit(|buffer| buffer.place_cursor_at_end());

            let snapshot = text_field_focus::focused_editor_state().expect("focused field");
            assert_eq!(snapshot.text, "hi");
            assert_eq!(
                (snapshot.selection_start, snapshot.selection_end),
                (2usize, 2usize)
            );
            assert_eq!(snapshot.composition, None);
            assert!(snapshot.single_line);

            assert!(text_field_focus::dispatch_ime_preedit("ab", None));
            let snapshot = text_field_focus::focused_editor_state().expect("focused field");
            assert_eq!(snapshot.text, "hiab");
            assert_eq!(snapshot.composition, Some((2, 4)));

            text_field_focus::clear_focus();
            assert_eq!(text_field_focus::focused_editor_state(), None);
        });
    }

    #[test]
    fn enter_on_single_line_field_is_not_consumed() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state("abc", TextFieldLineLimits::SingleLine);

            assert!(!text_field_focus::dispatch_key_event(&key_down(
                KeyCode::Enter,
                ""
            )));
            assert_eq!(state.text(), "abc");

            text_field_focus::clear_focus();
        });
    }
}
