//! The panel of the keyboard that a headset draws.
//!
//! The panel is an ordinary `SpatialPanel` that opens while the keyboard is open and stands below
//! the panel of the focused text field. Its keys are buttons. A key press pushes the key event
//! that a hardware keyboard sends for the key.

use cranpose::{composable, prelude::*, rememberMutableInteractionSource, text::TextUnit};
use cranpose_core::key;

use crate::{
    SpatialPanel, SubspaceModifier,
    key_layout::{self, KeyKind, PlacedKey},
    keyboard_state::{KeyboardState, Pressed},
    soft_keyboard::SoftKeyboard,
    space::Space,
};

const PANEL: Color = Color(0.09, 0.1, 0.13, 0.96);
const KEY: Color = Color(0.22, 0.24, 0.3, 1.0);
const ACTION: Color = Color(0.15, 0.17, 0.22, 1.0);
const LOCKED: Color = Color(0.3, 0.45, 0.85, 1.0);
const PRESSED: Color = Color(0.45, 0.62, 0.98, 1.0);
const TEXT: Color = Color(0.95, 0.96, 0.98, 1.0);

// The keyboard's panel while the keyboard is open, and nothing while it is closed.
#[composable]
pub(crate) fn SoftKeyboardPanel(space: Space) {
    let keyboard = space.keyboard();
    if !keyboard.is_open() {
        return;
    }
    let [width, height] = key_layout::SIZE;
    SpatialPanel(
        SubspaceModifier::new().size(width, height).below_field(),
        move || KeyCaps(keyboard.clone()),
    );
}

// The keys of the page that the keyboard shows. A press of a key changes the keyboard's state,
// which reaches the caps through the composition.
#[composable]
fn KeyCaps(keyboard: SoftKeyboard) {
    let state = rememberMutableStateOf(KeyboardState::default);
    let shown = state.get();
    Box(
        Modifier::empty()
            .fill_max_size()
            .background(PANEL)
            .rounded_corners(16.0),
        BoxSpec::default(),
        move || {
            for (index, placed) in key_layout::keys(shown.page).enumerate() {
                let keyboard = keyboard.clone();
                // The other page's keys are other nodes
                key((shown.page, index), move || {
                    KeyCap(placed, shown, state, keyboard);
                });
            }
        },
    );
}

#[composable]
fn KeyCap(
    key: PlacedKey,
    shown: KeyboardState,
    state: MutableState<KeyboardState>,
    keyboard: SoftKeyboard,
) {
    let source = rememberMutableInteractionSource();
    let pressed = collect_is_pressed_as_state(&source).get();
    let [left, top, width, height] = key.rect;
    let color = if pressed {
        PRESSED
    } else if key.kind == KeyKind::Shift && shown.capitals() {
        LOCKED
    } else if matches!(key.kind, KeyKind::Char { .. } | KeyKind::Space) {
        KEY
    } else {
        ACTION
    };
    let kind = key.kind;
    Button(
        Modifier::empty()
            .offset(left, top)
            .size_points(width, height)
            .background(color)
            .rounded_corners(8.0),
        ButtonSpec::default().interaction_source(source),
        move || {
            let mut next = state.get();
            match next.press(kind) {
                Pressed::Typed(event) => keyboard.type_key(event),
                Pressed::Hide => keyboard.hide(),
                Pressed::Nothing => {}
            }
            state.set(next);
        },
        move || {
            Box(
                Modifier::empty().fill_max_size(),
                BoxSpec::default().content_alignment(Alignment::CENTER),
                move || {
                    Text(
                        kind.label(shown.capitals()),
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(TEXT),
                                font_size: TextUnit::Sp(if matches!(kind, KeyKind::Char { .. }) {
                                    22.0
                                } else {
                                    15.0
                                }),
                                ..SpanStyle::default()
                            },
                            ..TextStyle::default()
                        },
                    );
                },
            );
        },
    );
}
