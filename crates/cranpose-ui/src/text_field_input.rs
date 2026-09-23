use cranpose_foundation::text::{TextFieldLineLimits, TextFieldState, TextRange};

use crate::{
    key_event::{KeyCode, KeyEvent},
    word_boundaries::{find_word_end, find_word_start},
};

pub(crate) fn handle_key_event_impl(
    state: &TextFieldState,
    event: &KeyEvent,
    line_limits: TextFieldLineLimits,
) -> bool {
    match event.key_code {
        KeyCode::Enter => {
            if line_limits.is_single_line() {
                false
            } else {
                state.edit(|buffer| buffer.insert("\n"));
                state.set_desired_column(None);
                true
            }
        }

        _ if !event.text.is_empty() && !event.modifiers.command_or_ctrl() => {
            state.edit(|buffer| buffer.insert(&event.text));
            state.set_desired_column(None);
            true
        }

        KeyCode::Backspace => {
            state.edit(cranpose_foundation::text::TextFieldBuffer::delete_before_cursor);
            state.set_desired_column(None);
            true
        }

        KeyCode::Delete => {
            state.edit(cranpose_foundation::text::TextFieldBuffer::delete_after_cursor);
            state.set_desired_column(None);
            true
        }

        KeyCode::ArrowLeft => {
            state.set_desired_column(None);
            if event.modifiers.command_or_ctrl() && !event.modifiers.shift {
                let text = state.text();
                let target = find_word_start(&text, state.selection().start);
                state.edit(|buffer| buffer.place_cursor_before_char(target));
            } else if event.modifiers.shift {
                state.edit(cranpose_foundation::text::TextFieldBuffer::extend_selection_left);
            } else {
                let text = state.text();
                let sel = state.selection();
                let target = if !sel.collapsed() {
                    sel.min()
                } else if sel.start > 0 {
                    text[..sel.start]
                        .char_indices()
                        .last()
                        .map_or(0, |(i, _)| i)
                } else {
                    0
                };
                state.edit(|buffer| buffer.place_cursor_before_char(target));
            }
            true
        }

        KeyCode::ArrowRight => {
            state.set_desired_column(None);
            if event.modifiers.command_or_ctrl() && !event.modifiers.shift {
                let text = state.text();
                let target = find_word_end(&text, state.selection().end);
                state.edit(|buffer| buffer.place_cursor_before_char(target));
            } else if event.modifiers.shift {
                state.edit(cranpose_foundation::text::TextFieldBuffer::extend_selection_right);
            } else {
                let text = state.text();
                let sel = state.selection();
                let target = if !sel.collapsed() {
                    sel.max()
                } else if sel.end < text.len() {
                    text[sel.end..]
                        .char_indices()
                        .nth(1)
                        .map_or(text.len(), |(i, _)| sel.end + i)
                } else {
                    text.len()
                };
                state.edit(|buffer| buffer.place_cursor_before_char(target));
            }
            true
        }

        KeyCode::ArrowUp => {
            let text = state.text();
            let sel = state.selection();
            let cursor = sel.end;
            let text_before = &text[..cursor.min(text.len())];
            let line_start = text_before.rfind('\n').map_or(0, |i| i + 1);
            let col = cursor - line_start;
            let column = state.desired_column().unwrap_or_else(|| {
                state.set_desired_column(Some(col));
                col
            });
            let target = if line_start == 0 {
                state.set_desired_column(None);
                0
            } else {
                let prev_end = line_start - 1;
                let prev_start = text[..prev_end].rfind('\n').map_or(0, |i| i + 1);
                prev_start + column.min(prev_end - prev_start)
            };
            if event.modifiers.shift {
                state.edit(|buffer| buffer.select(TextRange::new(sel.start, target)));
            } else {
                state.edit(|buffer| buffer.place_cursor_before_char(target));
            }
            true
        }

        KeyCode::ArrowDown => {
            let text = state.text();
            let sel = state.selection();
            let cursor = sel.end;
            let text_before = &text[..cursor.min(text.len())];
            let line_start = text_before.rfind('\n').map_or(0, |i| i + 1);
            let col = cursor - line_start;
            let column = state.desired_column().unwrap_or_else(|| {
                state.set_desired_column(Some(col));
                col
            });
            let line_end = text[cursor..].find('\n').map_or(text.len(), |i| cursor + i);
            let target = if line_end >= text.len() {
                state.set_desired_column(None);
                text.len()
            } else {
                let next_start = line_end + 1;
                let next_end = text[next_start..]
                    .find('\n')
                    .map_or(text.len(), |i| next_start + i);
                next_start + column.min(next_end - next_start)
            };
            if event.modifiers.shift {
                state.edit(|buffer| buffer.select(TextRange::new(sel.start, target)));
            } else {
                state.edit(|buffer| buffer.place_cursor_before_char(target));
            }
            true
        }

        KeyCode::Home => {
            state.set_desired_column(None);
            if event.modifiers.command_or_ctrl() {
                state.edit(cranpose_foundation::text::TextFieldBuffer::place_cursor_at_start);
            } else {
                let text = state.text();
                let pos = state.selection().start;
                let line_start = text[..pos.min(text.len())].rfind('\n').map_or(0, |i| i + 1);
                state.edit(|buffer| buffer.place_cursor_before_char(line_start));
            }
            true
        }

        KeyCode::End => {
            state.set_desired_column(None);
            if event.modifiers.command_or_ctrl() {
                state.edit(cranpose_foundation::text::TextFieldBuffer::place_cursor_at_end);
            } else {
                let text = state.text();
                let pos = state.selection().start;
                let line_end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
                state.edit(|buffer| buffer.place_cursor_before_char(line_end));
            }
            true
        }

        KeyCode::A if event.modifiers.command_or_ctrl() => {
            state.edit(cranpose_foundation::text::TextFieldBuffer::select_all);
            true
        }

        KeyCode::Z if event.modifiers.command_or_ctrl() && !event.modifiers.shift => {
            state.undo();
            true
        }

        KeyCode::Z if event.modifiers.command_or_ctrl() && event.modifiers.shift => {
            state.redo();
            true
        }
        KeyCode::Y if event.modifiers.command_or_ctrl() => {
            state.redo();
            true
        }

        _ => false,
    }
}
