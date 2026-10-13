use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_foundation::text::{
    InputTransformation, TextFieldBuffer, TextFieldLineLimits, TextFieldState,
};
use cranpose_ui_graphics::ProjectiveTransform;

use crate::{
    text::{AnnotatedString, TextStyle, measure_text},
    text_field_focus::ImeCaretGeometry,
    text_field_input::{KeyInput, handle_key_event_impl},
};

/// The field's keyboard and input filter, as its latest composition set
/// them.
pub(crate) struct InputRefs {
    pub keyboard_options: Rc<Cell<crate::KeyboardOptions>>,
    pub keyboard_action: Rc<RefCell<Option<crate::KeyboardActionHandler>>>,
    pub transformation: Rc<RefCell<Option<InputTransformation>>>,
}

#[derive(Clone)]
pub(crate) struct CaretGeometryRefs {
    pub local_to_window: Rc<Cell<ProjectiveTransform>>,
    pub content_origin: Rc<RefCell<crate::modifier::CoordinatorRect>>,
    pub scroll_offset: Rc<Cell<f32>>,
    pub wrap_width: Rc<Cell<Option<f32>>>,
    pub style: TextStyle,
}

pub(crate) struct TextFieldHandler {
    state: TextFieldState,
    node_id: Option<cranpose_core::NodeId>,
    target_node: Option<cranpose_core::NodeId>,
    line_limits: TextFieldLineLimits,
    show_keyboard_on_focus: bool,
    input: InputRefs,
    geometry: CaretGeometryRefs,
}

impl TextFieldHandler {
    pub(crate) fn new(
        state: TextFieldState,
        node_id: Option<cranpose_core::NodeId>,
        target_node: Option<cranpose_core::NodeId>,
        line_limits: TextFieldLineLimits,
        show_keyboard_on_focus: bool,
        input: InputRefs,
        geometry: CaretGeometryRefs,
    ) -> Rc<Self> {
        Rc::new(Self {
            state,
            node_id,
            target_node,
            line_limits,
            show_keyboard_on_focus,
            input,
            geometry,
        })
    }

    fn single_line(&self) -> bool {
        matches!(self.line_limits, TextFieldLineLimits::SingleLine)
    }

    /// Edits as the person's input: through the field's input
    /// transformation.
    fn edit_as_user(&self, f: impl FnOnce(&mut TextFieldBuffer)) {
        self.state
            .edit_as_user(self.input.transformation.borrow().as_ref(), f);
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

    fn target_node(&self) -> Option<cranpose_core::NodeId> {
        self.target_node
    }

    fn show_keyboard_on_focus(&self) -> bool {
        self.show_keyboard_on_focus
    }

    fn handle_key(&self, event: &crate::key_event::KeyEvent) -> bool {
        use crate::key_event::KeyEventType;

        if event.event_type != KeyEventType::KeyDown {
            return false;
        }

        if event.key_code == crate::key_event::KeyCode::Enter && self.single_line() {
            return self.perform_ime_action(crate::ImeAction::Default);
        }

        let consumed = handle_key_event_impl(
            &self.state,
            event,
            &KeyInput {
                line_limits: self.line_limits,
                transformation: self.input.transformation.borrow().as_ref(),
                style: &self.geometry.style,
                node_id: self.node_id,
                wrap_width: self.geometry.wrap_width.get(),
            },
        );

        if consumed {
            self.on_text_mutated();
        }

        consumed
    }

    fn insert_text(&self, text: &str) {
        self.edit_as_user(|buffer| buffer.insert(text));
        self.on_text_mutated();
    }

    fn delete_surrounding(&self, before_bytes: usize, after_bytes: usize) {
        if before_bytes == 0 && after_bytes == 0 {
            return;
        }

        self.edit_as_user(|buffer| buffer.delete_surrounding(before_bytes, after_bytes));
        self.state.set_desired_x(None);
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
        self.edit_as_user(|buffer| buffer.delete(selection));
        self.on_text_mutated();
        Some(text)
    }

    fn set_composition(&self, text: &str, cursor: Option<(usize, usize)>) {
        self.edit_as_user(|buffer| {
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
            let start = text.floor_char_boundary(start_bytes.min(end_bytes));
            let end = text.floor_char_boundary(start_bytes.max(end_bytes));
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
        let start = text.floor_char_boundary(start_bytes.min(end_bytes));
        let end = text.floor_char_boundary(start_bytes.max(end_bytes));
        self.state
            .set_selection(cranpose_foundation::text::TextRange::new(start, end));
        crate::cursor_animation::reset_cursor_blink();
        crate::request_render_invalidation();
    }

    fn editor_state(&self) -> Option<crate::text_field_focus::ImeEditorState> {
        let value = self.state.value();
        Some(crate::text_field_focus::ImeEditorState {
            selection_start: value.selection.min(),
            selection_end: value.selection.max(),
            composition: value.composition.map(|range| (range.min(), range.max())),
            text: value.text,
            single_line: self.single_line(),
            keyboard_options: self.input.keyboard_options.get(),
        })
    }

    /// Finishes any composition, then runs the field's resolved action: a
    /// new line where that is [`ImeAction::None`](crate::ImeAction::None) in a
    /// multi-line field, else its handler or default. A single-line field
    /// whose action is `None` does nothing and leaves the press unhandled.
    fn perform_ime_action(&self, action: crate::ImeAction) -> bool {
        let field_action = self.input.keyboard_options.get().ime_action;
        let action = match action {
            crate::ImeAction::Default => field_action,
            action => action,
        }
        .resolve(self.single_line());
        self.finish_composition();
        if action == crate::ImeAction::None {
            if self.single_line() {
                return false;
            }
            self.insert_text("\n");
            return true;
        }
        let handler = self.input.keyboard_action.borrow().clone();
        crate::keyboard_options::run_keyboard_action(action, handler.as_ref());
        true
    }

    fn caret_geometry(&self) -> Option<ImeCaretGeometry> {
        let value = self.state.value();
        let text = value.text.as_str();
        if text.contains('\n') {
            return None;
        }
        let g = &self.geometry;
        let content = g.content_origin.borrow().origin();
        let base_x = content.x - g.scroll_offset.get();
        let top = content.y;
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
            local_to_window: g.local_to_window.get(),
        })
    }
}

#[cfg(test)]
#[path = "tests/text_field_handler_tests.rs"]
mod tests;
