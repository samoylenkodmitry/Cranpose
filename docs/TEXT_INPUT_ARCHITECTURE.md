# Text input architecture

This document describes the text-input implementation in the current source.
Public API parity and named validation evidence are tracked in
[`text.md`](text.md) and the
[accessibility validation protocol](accessibility_validation.md).

## Data and edits

`cranpose_foundation::text::TextFieldState` owns the text value, selection,
composition range, undo history and cached line starts. Callers edit through
`TextFieldState::edit`, which gives a temporary `TextFieldBuffer` and publishes
the result as state. Buffer and selection positions are UTF-8 byte offsets;
the buffer checks character boundaries for each edit. See
[`state.rs`](../crates/cranpose-foundation/src/text/state.rs),
[`buffer.rs`](../crates/cranpose-foundation/src/text/buffer.rs) and
[`range.rs`](../crates/cranpose-foundation/src/text/range.rs).

## Composition and input

`BasicTextField` is the simple entry point. `BasicTextFieldWithOptions` exposes
its text style, cursor color, line limits and software-keyboard focus policy.
`BasicTextFieldDecorated` lets a decoration box place labels, placeholders,
icons and other content around the inner field. The decoration box owns the
field's semantics, focus and pointer input. Its scope requires the decoration
to invoke `inner_text_field()` once. These entry points are in
[`basic_text_field.rs`](../crates/cranpose-ui/src/widgets/basic_text_field.rs).

The text-field modifier node measures, lays out and draws the field. Its input
handler maps key events to edits; `TextFieldState` supplies the change observed
by composition. Text entry, deletion, cursor movement, line navigation and
selection extension are handled in
[`text_field_input.rs`](../crates/cranpose-ui/src/text_field_input.rs) and
[`text_field_handler.rs`](../crates/cranpose-ui/src/text_field_handler.rs).
The focused handler is registered and dispatched through
[`text_field_focus.rs`](../crates/cranpose-ui/src/text_field_focus.rs).

Pointer input supports caret placement, drag selection and word selection.
The text-field widget also composes selection handles, a loupe and the text
selection menu when the platform and interaction state call for them. The
modifier node publishes caret geometry for platform input and accessibility;
the node and decoration behavior live in
[`text_field_modifier_node.rs`](../crates/cranpose-ui/src/text_field_modifier_node.rs),
[`text_field_decorator_node.rs`](../crates/cranpose-ui/src/text_field_decorator_node.rs)
and [`text_selection_menu.rs`](../crates/cranpose-ui/src/widgets/text_selection_menu.rs).

IME pre-edit text lives in a composition range in the field buffer. Platform
adapters update or remove the range and commit its text through
the same edit path. Clipboard access goes through the UI clipboard session;
the web adapter requests paste asynchronously because browser clipboard reads
require an event-driven response. See
[`clipboard_session.rs`](../crates/cranpose-ui/src/clipboard_session.rs) and
[`web_clipboard.rs`](../crates/cranpose/src/web_clipboard.rs).

## Layout, text position and frame time

Text, selection and caret positions use the configured text-measurement
service. The UI service has a monospaced fallback, and hosts can install a
different `TextMeasurer`. A field edit schedules layout for the edited field node and its
affected ancestors; caret visibility and blink changes request a draw. The
blink state advances on the frame clock at 500 ms intervals while active,
through [`cursor_animation.rs`](../crates/cranpose-ui/src/cursor_animation.rs).

## Keyboard options and actions

`BasicTextFieldOptions::keyboard_options` takes Compose's `KeyboardOptions`:
keyboard type, capitalization, autocorrection and IME action. The focused
field reports them in `ImeEditorState`, and each platform adapter maps them
to its native editor:

| Platform | Native configuration | Action key |
| --- | --- | --- |
| Android | `EditorInfo.inputType` and `imeOptions`, as Compose's Android text input sets them | `performEditorAction` |
| iOS | `UITextInputTraits` of the key input view, reloaded when they change | the return key's `insertText("\n")` |
| Web | `inputmode`, `enterkeyhint`, `autocapitalize`, `autocorrect` and `spellcheck` on the hidden editor; a password keyboard opens the password input | Enter in the one-line editor |
| Desktop | none | Enter |

Enter in a single-line field and the platform's action key run the field's
resolved action through `on_keyboard_action`, or the action's default
without a handler: Next and Previous move focus, Done hides the software
keyboard and keeps focus, and Go, Search and Send do nothing. In a
multi-line field the action key adds a line unless the field names an
action; a hardware Enter always adds a line there. A web multi-line editor
always adds a line, since the browser does not tell its action key from
Enter. The shared types live in
[`keyboard_options.rs`](../crates/cranpose-ui/src/keyboard_options.rs).

## Input transformations

`BasicTextFieldOptions::input_transformation` takes Compose's
`InputTransformation`, which changes or rejects each edit a person makes:
typing, pasting, deleting, cutting and IME composition. It sees the value
before the edit and the buffer after it. `max_length`, `by_value` and `then`
build the common ones. The field's input goes through
`TextFieldState::edit_as_user`; the app's own `edit` calls and edits that
only move the cursor skip the transformation. `max_length` also lets a
shortening edit through, so text the app put past the limit can be deleted.
See [`input_transformation.rs`](../crates/cranpose-foundation/src/text/input_transformation.rs).

## Current limits

Autofill hints, output transformations and a secure text field remain
outside the current public surface. The default UI measurement fallback
is monospaced; a host can install a font-backed `TextMeasurer` through its text
service.
