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
        self.state
            .edit(cranpose_foundation::text::TextFieldBuffer::select_all);
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
#[path = "tests/text_field_handler_tests.rs"]
mod tests;
