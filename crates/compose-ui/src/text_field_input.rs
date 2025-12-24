//! Keyboard input handling for text fields.
//!
//! This module contains the shared keyboard event handling logic used by both
//! `TextFieldHandler` (O(1) dispatch) and `TextFieldModifierNode::handle_key_event`.
//!
//! Having a single implementation prevents behavioral drift between the two paths.

use crate::key_event::{KeyCode, KeyEvent};
use crate::word_boundaries::{find_word_end, find_word_start};
use compose_foundation::text::{TextFieldLineLimits, TextFieldState, TextRange};

/// Shared keyboard event handling implementation.
///
/// This function contains the core keyboard handling logic used by both
/// `TextFieldHandler::handle_key` and `TextFieldModifierNode::handle_key_event`.
/// Having a single implementation prevents behavioral drift between the two paths.
///
/// The `line_limits` parameter controls whether newlines are allowed (MultiLine)
/// or blocked (SingleLine).
pub(crate) fn handle_key_event_impl(
    state: &TextFieldState,
    event: &KeyEvent,
    line_limits: TextFieldLineLimits,
) -> bool {
    match event.key_code {
        // Enter - insert newline (only for MultiLine mode)
        KeyCode::Enter => {
            if line_limits.is_single_line() {
                // In SingleLine mode, Enter does NOT insert newline
                // Could be used for submit action in the future
                false
            } else {
                state.edit(|buffer| buffer.insert("\n"));
                state.set_desired_column(None);
                true
            }
        }

        // Character input (most common case)
        _ if !event.text.is_empty() && !event.modifiers.command_or_ctrl() => {
            state.edit(|buffer| buffer.insert(&event.text));
            state.set_desired_column(None);
            true
        }

        // Backspace
        KeyCode::Backspace => {
            state.edit(|buffer| buffer.delete_before_cursor());
            state.set_desired_column(None);
            true
        }

        // Delete
        KeyCode::Delete => {
            state.edit(|buffer| buffer.delete_after_cursor());
            state.set_desired_column(None);
            true
        }

        // Arrow Left
        KeyCode::ArrowLeft => {
            state.set_desired_column(None);
            if event.modifiers.command_or_ctrl() && !event.modifiers.shift {
                let text = state.text();
                let target = find_word_start(&text, state.selection().start);
                state.edit(|buffer| buffer.place_cursor_before_char(target));
            } else if event.modifiers.shift {
                state.edit(|buffer| buffer.extend_selection_left());
            } else {
                let text = state.text();
                let sel = state.selection();
                let target = if !sel.collapsed() {
                    sel.min()
                } else if sel.start > 0 {
                    text[..sel.start]
                        .char_indices()
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(0)
                } else {
                    0
                };
                state.edit(|buffer| buffer.place_cursor_before_char(target));
            }
            true
        }

        // Arrow Right
        KeyCode::ArrowRight => {
            state.set_desired_column(None);
            if event.modifiers.command_or_ctrl() && !event.modifiers.shift {
                let text = state.text();
                let target = find_word_end(&text, state.selection().end);
                state.edit(|buffer| buffer.place_cursor_before_char(target));
            } else if event.modifiers.shift {
                state.edit(|buffer| buffer.extend_selection_right());
            } else {
                let text = state.text();
                let sel = state.selection();
                let target = if !sel.collapsed() {
                    sel.max()
                } else if sel.end < text.len() {
                    text[sel.end..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| sel.end + i)
                        .unwrap_or(text.len())
                } else {
                    text.len()
                };
                state.edit(|buffer| buffer.place_cursor_before_char(target));
            }
            true
        }

        // Arrow Up (previous line)
        KeyCode::ArrowUp => {
            let text = state.text();
            let sel = state.selection();
            let cursor = sel.end;
            let text_before = &text[..cursor.min(text.len())];
            let line_start = text_before.rfind('\n').map(|i| i + 1).unwrap_or(0);
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
                let prev_start = text[..prev_end].rfind('\n').map(|i| i + 1).unwrap_or(0);
                prev_start + column.min(prev_end - prev_start)
            };
            if event.modifiers.shift {
                state.edit(|buffer| buffer.select(TextRange::new(sel.start, target)));
            } else {
                state.edit(|buffer| buffer.place_cursor_before_char(target));
            }
            true
        }

        // Arrow Down (next line)
        KeyCode::ArrowDown => {
            let text = state.text();
            let sel = state.selection();
            let cursor = sel.end;
            let text_before = &text[..cursor.min(text.len())];
            let line_start = text_before.rfind('\n').map(|i| i + 1).unwrap_or(0);
            let col = cursor - line_start;
            let column = state.desired_column().unwrap_or_else(|| {
                state.set_desired_column(Some(col));
                col
            });
            let line_end = text[cursor..]
                .find('\n')
                .map(|i| cursor + i)
                .unwrap_or(text.len());
            let target = if line_end >= text.len() {
                state.set_desired_column(None);
                text.len()
            } else {
                let next_start = line_end + 1;
                let next_end = text[next_start..]
                    .find('\n')
                    .map(|i| next_start + i)
                    .unwrap_or(text.len());
                next_start + column.min(next_end - next_start)
            };
            if event.modifiers.shift {
                state.edit(|buffer| buffer.select(TextRange::new(sel.start, target)));
            } else {
                state.edit(|buffer| buffer.place_cursor_before_char(target));
            }
            true
        }

        // Home
        KeyCode::Home => {
            state.set_desired_column(None);
            if event.modifiers.command_or_ctrl() {
                state.edit(|buffer| buffer.place_cursor_at_start());
            } else {
                let text = state.text();
                let pos = state.selection().start;
                let line_start = text[..pos.min(text.len())]
                    .rfind('\n')
                    .map(|i| i + 1)
                    .unwrap_or(0);
                state.edit(|buffer| buffer.place_cursor_before_char(line_start));
            }
            true
        }

        // End
        KeyCode::End => {
            state.set_desired_column(None);
            if event.modifiers.command_or_ctrl() {
                state.edit(|buffer| buffer.place_cursor_at_end());
            } else {
                let text = state.text();
                let pos = state.selection().start;
                let line_end = text[pos..]
                    .find('\n')
                    .map(|i| pos + i)
                    .unwrap_or(text.len());
                state.edit(|buffer| buffer.place_cursor_before_char(line_end));
            }
            true
        }

        // Select all (Ctrl+A)
        KeyCode::A if event.modifiers.command_or_ctrl() => {
            state.edit(|buffer| buffer.select_all());
            true
        }

        // Undo (Ctrl+Z)
        KeyCode::Z if event.modifiers.command_or_ctrl() && !event.modifiers.shift => {
            state.undo();
            true
        }

        // Redo (Ctrl+Shift+Z or Ctrl+Y)
        KeyCode::Z if event.modifiers.command_or_ctrl() && event.modifiers.shift => {
            state.redo();
            true
        }
        KeyCode::Y if event.modifiers.command_or_ctrl() => {
            state.redo();
            true
        }

        // Ctrl+C/X/V - DO NOT handle here! Let platform handle clipboard
        _ => false,
    }
}
