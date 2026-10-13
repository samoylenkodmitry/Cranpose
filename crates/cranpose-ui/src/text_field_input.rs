use cranpose_foundation::text::{
    InputTransformation, TextFieldLineLimits, TextFieldState, TextRange,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    key_event::{KeyCode, KeyEvent},
    word_boundaries::{find_word_end, find_word_start},
};

pub(crate) fn handle_key_event_impl(
    state: &TextFieldState,
    event: &KeyEvent,
    line_limits: TextFieldLineLimits,
    transformation: Option<&InputTransformation>,
) -> bool {
    match event.key_code {
        KeyCode::Enter => {
            if line_limits.is_single_line() {
                false
            } else {
                state.edit_as_user(transformation, |buffer| buffer.insert("\n"));
                state.set_desired_column(None);
                true
            }
        }

        _ if !event.text.is_empty() && !event.modifiers.command_or_ctrl() => {
            state.edit_as_user(transformation, |buffer| buffer.insert(&event.text));
            state.set_desired_column(None);
            true
        }

        KeyCode::Backspace => {
            state.edit_as_user(
                transformation,
                cranpose_foundation::text::TextFieldBuffer::delete_before_cursor,
            );
            state.set_desired_column(None);
            true
        }

        KeyCode::Delete => {
            state.edit_as_user(
                transformation,
                cranpose_foundation::text::TextFieldBuffer::delete_after_cursor,
            );
            state.set_desired_column(None);
            true
        }

        KeyCode::ArrowLeft => {
            state.set_desired_column(None);
            state.edit(|buffer| {
                if event.modifiers.command_or_ctrl() && !event.modifiers.shift {
                    let target = find_word_start(buffer.text(), buffer.selection().start);
                    buffer.place_cursor_before_char(target);
                } else if event.modifiers.shift {
                    buffer.extend_selection_left();
                } else {
                    buffer.move_cursor_left();
                }
            });
            true
        }

        KeyCode::ArrowRight => {
            state.set_desired_column(None);
            state.edit(|buffer| {
                if event.modifiers.command_or_ctrl() && !event.modifiers.shift {
                    let target = find_word_end(buffer.text(), buffer.selection().end);
                    buffer.place_cursor_before_char(target);
                } else if event.modifiers.shift {
                    buffer.extend_selection_right();
                } else {
                    buffer.move_cursor_right();
                }
            });
            true
        }

        KeyCode::ArrowUp => {
            move_line(state, event.modifiers.shift, LineStep::Up);
            true
        }

        KeyCode::ArrowDown => {
            move_line(state, event.modifiers.shift, LineStep::Down);
            true
        }

        KeyCode::Home => {
            state.set_desired_column(None);
            if event.modifiers.command_or_ctrl() {
                state.edit(cranpose_foundation::text::TextFieldBuffer::place_cursor_at_start);
            } else {
                state.edit(|buffer| {
                    let text = buffer.text();
                    let pos = buffer.selection().start.min(text.len());
                    let line_start = text[..pos].rfind('\n').map_or(0, |i| i + 1);
                    buffer.place_cursor_before_char(line_start);
                });
            }
            true
        }

        KeyCode::End => {
            state.set_desired_column(None);
            if event.modifiers.command_or_ctrl() {
                state.edit(cranpose_foundation::text::TextFieldBuffer::place_cursor_at_end);
            } else {
                state.edit(|buffer| {
                    let text = buffer.text();
                    let pos = buffer.selection().start.min(text.len());
                    let line_end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
                    buffer.place_cursor_before_char(line_end);
                });
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

#[derive(Clone, Copy)]
enum LineStep {
    Up,
    Down,
}

/// Moves the cursor to the line above or below, keeping the column it had
/// before the first vertical move. Columns count characters, so the cursor
/// lands on a character boundary in any script.
fn move_line(state: &TextFieldState, extend_selection: bool, step: LineStep) {
    state.edit(|buffer| {
        let text = buffer.text();
        let selection = buffer.selection();
        let cursor = selection.end.min(text.len());
        let line_start = text[..cursor].rfind('\n').map_or(0, |i| i + 1);
        let column = state.desired_column().unwrap_or_else(|| {
            let column = text[line_start..cursor].graphemes(true).count();
            state.set_desired_column(Some(column));
            column
        });
        let line = match step {
            LineStep::Up => line_start.checked_sub(1).map(|end| {
                let start = text[..end].rfind('\n').map_or(0, |i| i + 1);
                (start, end)
            }),
            LineStep::Down => text[cursor..].find('\n').map(|i| {
                let start = cursor + i + 1;
                let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
                (start, end)
            }),
        };
        let target = match line {
            Some((start, end)) => text[start..end]
                .grapheme_indices(true)
                .nth(column)
                .map_or(end, |(i, _)| start + i),
            None => {
                state.set_desired_column(None);
                match step {
                    LineStep::Up => 0,
                    LineStep::Down => text.len(),
                }
            }
        };
        if extend_selection {
            buffer.select(TextRange::new(selection.start, target));
        } else {
            buffer.place_cursor_before_char(target);
        }
    });
}
