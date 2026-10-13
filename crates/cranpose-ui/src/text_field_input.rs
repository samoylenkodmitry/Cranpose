use cranpose_foundation::text::{
    InputTransformation, TextFieldBuffer, TextFieldLineLimits, TextFieldState, TextRange,
    floor_grapheme_boundary,
};

use crate::{
    key_event::{KeyCode, KeyEvent},
    text::{
        AnnotatedString, ResolvedTextDirection, TextLayoutOptions, TextStyle,
        get_offset_for_position, measure_text, resolve_text_direction, wrapped_line_ranges,
    },
    text_selection::{LineAffinity, caret_visual_line},
    word_boundaries::{find_word_end, find_word_start},
};

/// What a field's key handling reads besides its state: how it filters
/// edits, and how its text is laid out.
pub(crate) struct KeyInput<'a> {
    pub line_limits: TextFieldLineLimits,
    pub transformation: Option<&'a InputTransformation>,
    pub style: &'a TextStyle,
    pub node_id: Option<cranpose_core::NodeId>,
    pub wrap_width: Option<f32>,
}

pub(crate) fn handle_key_event_impl(
    state: &TextFieldState,
    event: &KeyEvent,
    input: &KeyInput<'_>,
) -> bool {
    let transformation = input.transformation;
    match event.key_code {
        KeyCode::Enter => {
            if input.line_limits.is_single_line() {
                false
            } else {
                state.edit_as_user(transformation, |buffer| buffer.insert("\n"));
                state.set_desired_x(None);
                true
            }
        }

        _ if !event.text.is_empty() && !event.modifiers.command_or_ctrl() => {
            state.edit_as_user(transformation, |buffer| buffer.insert(&event.text));
            state.set_desired_x(None);
            true
        }

        KeyCode::Backspace => {
            state.edit_as_user(transformation, TextFieldBuffer::delete_before_cursor);
            state.set_desired_x(None);
            true
        }

        KeyCode::Delete => {
            state.edit_as_user(transformation, TextFieldBuffer::delete_after_cursor);
            state.set_desired_x(None);
            true
        }

        KeyCode::ArrowLeft | KeyCode::ArrowRight => {
            state.set_desired_x(None);
            let direction = input.style.paragraph_style.text_direction;
            state.edit(|buffer| {
                // Left goes back in a left-to-right paragraph and forward in a
                // right-to-left one, as Compose's `moveCursorLeft` does.
                let rtl = paragraph_is_rtl(buffer.text(), buffer.selection().end, direction);
                let back = (event.key_code == KeyCode::ArrowLeft) != rtl;
                let by_word = event.modifiers.command_or_ctrl() && !event.modifiers.shift;
                match (back, by_word, event.modifiers.shift) {
                    (true, true, _) => {
                        let target = find_word_start(buffer.text(), buffer.selection().start);
                        buffer.place_cursor_before_char(target);
                    }
                    (false, true, _) => {
                        let target = find_word_end(buffer.text(), buffer.selection().end);
                        buffer.place_cursor_before_char(target);
                    }
                    (true, false, true) => buffer.extend_selection_left(),
                    (false, false, true) => buffer.extend_selection_right(),
                    (true, false, false) => buffer.move_cursor_left(),
                    (false, false, false) => buffer.move_cursor_right(),
                }
            });
            true
        }

        KeyCode::ArrowUp => {
            move_line(state, event.modifiers.shift, LineStep::Up, input);
            true
        }

        KeyCode::ArrowDown => {
            move_line(state, event.modifiers.shift, LineStep::Down, input);
            true
        }

        KeyCode::Home | KeyCode::End => {
            state.set_desired_x(None);
            let to_start = event.key_code == KeyCode::Home;
            if event.modifiers.command_or_ctrl() {
                state.edit(if to_start {
                    TextFieldBuffer::place_cursor_at_start
                } else {
                    TextFieldBuffer::place_cursor_at_end
                });
            } else {
                state.edit(|buffer| {
                    let line =
                        LaidOutLines::of(buffer.text(), input).line_at(buffer.selection().end);
                    buffer.place_cursor_before_char(if to_start { line.start } else { line.end });
                });
            }
            true
        }

        KeyCode::A if event.modifiers.command_or_ctrl() => {
            state.edit(TextFieldBuffer::select_all);
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

/// Whether the paragraph around `offset` reads right to left.
fn paragraph_is_rtl(text: &str, offset: usize, direction: crate::text::TextDirection) -> bool {
    let offset = text.floor_char_boundary(offset);
    let start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
    resolve_text_direction(&text[start..end], Some(direction)) == ResolvedTextDirection::Rtl
}

/// A field's text as layout breaks it into lines: wrapped where the field
/// wraps, else at its line breaks.
struct LaidOutLines<'a> {
    text: &'a str,
    style: &'a TextStyle,
    ranges: Vec<std::ops::Range<usize>>,
}

impl<'a> LaidOutLines<'a> {
    fn of(text: &'a str, input: &KeyInput<'a>) -> Self {
        let ranges = wrapped_line_ranges(
            input.node_id,
            &AnnotatedString::from(text),
            input.style,
            TextLayoutOptions::default(),
            input.wrap_width,
        );
        Self {
            text,
            style: input.style,
            ranges,
        }
    }

    /// The index of the line the caret at `offset` sits on: the upper line
    /// at a wrap, where the drawn caret is.
    fn index_at(&self, offset: usize) -> usize {
        caret_visual_line(&self.ranges, offset, LineAffinity::Upstream).0
    }

    fn line_at(&self, offset: usize) -> std::ops::Range<usize> {
        self.ranges
            .get(self.index_at(offset))
            .cloned()
            .unwrap_or(0..self.text.len())
    }

    /// The caret's x after `offset` on its line.
    fn x_at(&self, offset: usize) -> f32 {
        let start = self.line_at(offset).start.min(offset);
        measure_text(
            &AnnotatedString::from(&self.text[start..offset]),
            self.style,
        )
        .width
    }

    /// The character boundary on line `index` nearest `x`.
    fn offset_at(&self, index: usize, x: f32) -> Option<usize> {
        let range = self.ranges.get(index)?;
        let line = &self.text[range.clone()];
        let within = get_offset_for_position(&AnnotatedString::from(line), self.style, x, 0.0);
        Some(floor_grapheme_boundary(
            self.text,
            range.start + within.min(line.len()),
        ))
    }
}

#[derive(Clone, Copy)]
enum LineStep {
    Up,
    Down,
}

/// Moves the cursor to the laid-out line above or below, keeping the x it
/// had before the first of a run of vertical moves, as Compose's
/// `moveCursorUpByLine` does. Past the first or last line it goes to the
/// start or end of the text.
fn move_line(state: &TextFieldState, extend_selection: bool, step: LineStep, input: &KeyInput<'_>) {
    state.edit(|buffer| {
        let lines = LaidOutLines::of(buffer.text(), input);
        let selection = buffer.selection();
        let cursor = selection.end.min(buffer.len());
        let x = state.desired_x().unwrap_or_else(|| {
            let x = lines.x_at(cursor);
            state.set_desired_x(Some(x));
            x
        });
        let line = lines.index_at(cursor);
        let target_line = match step {
            LineStep::Up => line.checked_sub(1),
            LineStep::Down => Some(line + 1),
        };
        let target = target_line
            .and_then(|index| lines.offset_at(index, x))
            .unwrap_or_else(|| {
                state.set_desired_x(None);
                match step {
                    LineStep::Up => 0,
                    LineStep::Down => buffer.len(),
                }
            });
        if extend_selection {
            buffer.select(TextRange::new(selection.start, target));
        } else {
            buffer.place_cursor_before_char(target);
        }
    });
}
