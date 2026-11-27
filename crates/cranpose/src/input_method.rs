//! The application as the system keyboard.
//!
//! [`AppLauncher::with_input_method`](crate::AppLauncher::with_input_method)
//! names the board. On Android it runs in an input method service (the
//! `input-method` service of the Gradle plugin) and draws into the service's
//! surface on the GPU. On iOS a keyboard extension's main function calls
//! [`AppLauncher::run_keyboard_extension`](crate::AppLauncher), and the
//! extension's `NSExtensionPrincipalClass` is `CranposeInputViewController`.
//! The board reads [`local_input_connection`] to type into the focused text
//! field and to learn what that field asks for.

use cranpose_core::{CompositionLocal, compositionLocalOf};
use cranpose_ui::{ImeAction, KeyboardCapitalization, KeyboardType, Size};

/// The board of the system keyboard.
#[derive(Clone, Copy, Debug)]
pub struct InputMethod {
    /// The root composable of the keyboard.
    pub content: fn(),
    /// The keyboard's height in logical pixels on a screen of the given
    /// logical size.
    pub height: fn(Size) -> f32,
}

/// The text field that has the keyboard, as the platform describes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditorInfo {
    /// What the field takes: text, digits, a phone number and so on.
    pub keyboard_type: KeyboardType,
    /// What the field's action key does.
    pub ime_action: ImeAction,
    /// Whether the field holds one line, so its action key does not start a
    /// new line.
    pub single_line: bool,
    /// Which letters the field wants as capitals.
    pub capitalization: KeyboardCapitalization,
    /// Whether the field wants a capital letter next, as at the start of a
    /// sentence.
    pub capitalize_next: bool,
}

/// A field's turn with the keyboard. It takes a new value each time a field
/// takes the keyboard, the same field again included, so a board resets its
/// shift and its page for the field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct InputSession(u32);

impl InputSession {
    #[cfg(all(
        feature = "renderer-wgpu",
        any(
            all(feature = "android", target_os = "android"),
            all(feature = "ios", target_os = "ios")
        )
    ))]
    pub(crate) fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// The keyboard's connection to the focused text field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputConnection {
    editor: EditorInfo,
    session: InputSession,
}

impl InputConnection {
    #[cfg(all(
        feature = "renderer-wgpu",
        any(
            all(feature = "android", target_os = "android"),
            all(feature = "ios", target_os = "ios")
        )
    ))]
    pub(crate) fn new(editor: EditorInfo, session: InputSession) -> Self {
        Self { editor, session }
    }

    /// What the focused field asks for.
    pub fn editor(self) -> EditorInfo {
        self.editor
    }

    /// The focused field's turn with the keyboard.
    pub fn session(self) -> InputSession {
        self.session
    }

    /// Inserts `text` at the cursor, in place of the selection.
    pub fn commit_text(self, text: &str) {
        platform::commit_text(text);
    }

    /// Deletes the selection, or the character before the cursor.
    pub fn delete_backward(self) {
        platform::delete_backward();
    }

    /// Runs the field's action, such as search or send, or starts a new line
    /// in a field of several lines or with no action.
    pub fn perform_ime_action(self) {
        platform::perform_ime_action();
    }

    /// Hands over to the next keyboard the person enabled.
    pub fn switch_to_next_keyboard(self) {
        platform::switch_to_next_keyboard();
    }

    /// The text before the cursor, its last `max` characters at most, or
    /// `None` when the field shows the keyboard none.
    pub fn text_before_cursor(self, max: usize) -> Option<String> {
        platform::text_before_cursor(max)
    }

    /// Deletes `before` characters before the cursor and `after` characters
    /// after it.
    pub fn delete_surrounding_text(self, before: usize, after: usize) {
        platform::delete_surrounding_text(before, after);
    }

    /// Moves the cursor by `offset` characters: back when negative.
    pub fn move_cursor(self, offset: i32) {
        platform::move_cursor(offset);
    }

    /// Hides the keyboard, as Compose's `SoftwareKeyboardController.hide`.
    pub fn hide(self) {
        platform::hide();
    }
}

/// The keyboard's connection while the composition runs as the system
/// keyboard, and `None` in an ordinary window.
pub fn local_input_connection() -> CompositionLocal<Option<InputConnection>> {
    crate::thread_state::with(|state| {
        state
            .input_connection_local
            .get_or_init(|| compositionLocalOf(|| None))
            .clone()
    })
}

// Provides `connection` to `content`.
#[cfg(all(
    feature = "renderer-wgpu",
    any(
        all(feature = "android", target_os = "android"),
        all(feature = "ios", target_os = "ios")
    )
))]
pub(crate) fn provide(connection: InputConnection, content: impl FnOnce()) {
    cranpose_core::CompositionLocalProvider(
        [local_input_connection().provides(Some(connection))],
        content,
    );
}

#[cfg(all(feature = "android", feature = "renderer-wgpu", target_os = "android"))]
mod platform {
    pub(super) use crate::android::input_method::{
        commit_text, delete_backward, delete_surrounding_text, hide, move_cursor,
        perform_ime_action, switch_to_next_keyboard, text_before_cursor,
    };
}

#[cfg(all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"))]
mod platform {
    pub(super) use crate::ios::input_method::{
        commit_text, delete_backward, delete_surrounding_text, hide, move_cursor,
        perform_ime_action, switch_to_next_keyboard, text_before_cursor,
    };
}

#[cfg(not(all(
    feature = "renderer-wgpu",
    any(
        all(feature = "android", target_os = "android"),
        all(feature = "ios", target_os = "ios")
    )
)))]
mod platform {
    // A composition that is not a system keyboard has no connection to
    // reach these, so they have nothing to do.
    pub(super) fn commit_text(_text: &str) {}
    pub(super) fn delete_backward() {}
    pub(super) fn perform_ime_action() {}
    pub(super) fn switch_to_next_keyboard() {}
    pub(super) fn text_before_cursor(_max: usize) -> Option<String> {
        None
    }
    pub(super) fn delete_surrounding_text(_before: usize, _after: usize) {}
    pub(super) fn move_cursor(_offset: i32) {}
    pub(super) fn hide() {}
}
