//! Compose's `BasicSecureTextField`: a text field for a secret.

use std::time::Duration;

use cranpose_core::{LaunchedEffectAsync, NodeId, mutableStateOf, remember, rememberKeyed};
use cranpose_foundation::text::{
    InputTransformation, OutputTransformation, TextFieldLineLimits, TextFieldState,
    previous_grapheme_boundary,
};
use unicode_segmentation::UnicodeSegmentation;

use super::{BasicTextFieldOptions, basic_text_field::TextFieldNode};
use crate::{
    composable,
    keyboard_options::{KeyboardOptions, KeyboardType},
    modifier::Modifier,
};

/// How long [`TextObfuscationMode::RevealLastTyped`] shows a typed
/// character, as Compose's secure field does.
const REVEAL_DURATION: Duration = Duration::from_millis(1500);

/// How a secure field hides its text: Compose's `TextObfuscationMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextObfuscationMode {
    /// Every character hidden, but the last one typed shows for a moment.
    #[default]
    RevealLastTyped,
    /// Every character hidden.
    Hidden,
    /// The text shown as it is, for a "show password" toggle.
    Visible,
}

/// Options for a [`BasicSecureTextField`].
#[derive(Clone, Debug, PartialEq)]
pub struct SecureTextFieldOptions {
    /// The field's options. Their output transformation is replaced by the
    /// obfuscation.
    pub field: BasicTextFieldOptions,
    pub obfuscation_mode: TextObfuscationMode,
    /// The character every hidden character shows as.
    pub obfuscation_character: char,
}

impl Default for SecureTextFieldOptions {
    fn default() -> Self {
        Self {
            field: BasicTextFieldOptions {
                line_limits: TextFieldLineLimits::SingleLine,
                keyboard_options: KeyboardOptions {
                    keyboard_type: KeyboardType::Password,
                    auto_correct: false,
                    ..KeyboardOptions::default()
                },
                ..BasicTextFieldOptions::default()
            },
            obfuscation_mode: TextObfuscationMode::default(),
            obfuscation_character: '\u{2022}',
        }
    }
}

/// A text field for a password or another secret, as Compose's
/// `BasicSecureTextField`: it shows its characters as
/// [`SecureTextFieldOptions::obfuscation_character`], asks for the
/// platform's secure keyboard, leaves the text out of copy, cut and
/// accessibility, and keeps it out of debug output.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::*;
///
/// #[composable]
/// fn PasswordField() {
///     let password = cranpose_core::remember(|| TextFieldState::new("")).with(|state| *state);
///     BasicSecureTextField(
///         password,
///         Modifier::empty(),
///         SecureTextFieldOptions::default(),
///     );
/// }
/// ```
#[composable]
pub fn BasicSecureTextField(
    state: TextFieldState,
    modifier: Modifier,
    options: SecureTextFieldOptions,
) -> NodeId {
    let SecureTextFieldOptions {
        field,
        obfuscation_mode,
        obfuscation_character,
    } = options;
    // The start of the character typed last, while it shows.
    let revealed = remember(|| mutableStateOf(None::<usize>)).with(|state| *state);
    let reveal = obfuscation_mode == TextObfuscationMode::RevealLastTyped;
    let shown_character = revealed.get().filter(|_| reveal);
    let output_transformation = rememberKeyed(
        (obfuscation_mode, obfuscation_character, shown_character),
        |&(mode, mask, shown)| {
            (mode != TextObfuscationMode::Visible).then(|| obfuscation(mask, shown))
        },
    );
    let input_transformation = rememberKeyed(
        (reveal, field.input_transformation.clone()),
        move |(reveal, filter)| {
            if *reveal {
                Some(note_typed_character(filter.clone(), revealed))
            } else {
                filter.clone()
            }
        },
    );
    LaunchedEffectAsync(shown_character, move |_| {
        Box::pin(async move {
            if shown_character.is_some() {
                cranpose_core::delay(REVEAL_DURATION).await;
                revealed.set(None);
            }
        })
    });
    TextFieldNode(
        state,
        modifier,
        BasicTextFieldOptions {
            input_transformation,
            output_transformation,
            ..field
        },
        None,
        true,
    )
}

/// Shows every character as `mask` but the one that starts at `shown`.
fn obfuscation(mask: char, shown: Option<usize>) -> OutputTransformation {
    let mut buffer = [0; 4];
    let mask: std::rc::Rc<str> = std::rc::Rc::from(&*mask.encode_utf8(&mut buffer));
    OutputTransformation::new(move |text, changes| {
        for (start, grapheme) in text.grapheme_indices(true) {
            if Some(start) != shown {
                changes.replace(start..start + grapheme.len(), &mask);
            }
        }
    })
}

/// `filter`, then a note of where a single typed character landed, which
/// the field shows for a moment. Any other edit hides it again.
fn note_typed_character(
    filter: Option<InputTransformation>,
    revealed: cranpose_core::MutableState<Option<usize>>,
) -> InputTransformation {
    let note = InputTransformation::new(move |original, buffer| {
        let selection = buffer.selection();
        let caret = selection.end;
        let start = previous_grapheme_boundary(buffer.text(), caret);
        let grown = buffer.len().saturating_sub(original.text.len());
        let typed_one = selection.collapsed() && grown > 0 && grown == caret - start;
        revealed.set(typed_one.then_some(start));
    });
    match filter {
        Some(filter) => filter.then(note),
        None => note,
    }
}
