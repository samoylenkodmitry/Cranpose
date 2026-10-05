//! [`SelectionContainer`] and [`DisableSelection`]: read-only text a person
//! can select and copy.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{
    CompositionLocal, CompositionLocalProvider, DisposableEffect, remember, rememberMutableStateOf,
    with_current_composer,
};
use cranpose_foundation::{PointerButton, modifier_element};

use crate::{
    PointerEventKind, PointerInputScope, composable,
    modifier::{Brush, Modifier},
    modifier_nodes::SelectableTextElement,
    selection_container::{
        SelectableGeometry, SelectionGesture, SelectionRegistrar, TextSelection,
    },
    text::{AnnotatedString, TextLayoutOptions, TextStyle},
    widgets::{Box, BoxSpec, LiquidTextMenu, MenuAnchor, TextMenuItem},
};

/// The container the `Text` composed here selects in: set by
/// [`SelectionContainer`], cleared by [`DisableSelection`].
pub fn local_selection_registrar() -> CompositionLocal<Option<SelectionRegistrar>> {
    crate::environment_locals::cached_local(
        |locals| &locals.selection_registrar,
        || cranpose_core::compositionLocalOf(|| None),
    )
}

/// Lets users select and copy text across the content. A mouse drag selects a
/// range; double and triple clicks select a word and a line. A touch hold starts
/// selection and a release opens the text menu. Ctrl+C and Ctrl+A use the copy and
/// select-all actions; macOS uses Cmd. Escape clears the selection.
///
/// Wrap excluded content in [`DisableSelection`].
///
/// # Example
///
/// ```rust
/// use cranpose_ui::{widgets::SelectionContainer, *};
///
/// #[composable]
/// fn Article() {
///     SelectionContainer(Modifier::empty(), || {
///         Text(
///             "Select and copy this text.",
///             Modifier::empty(),
///             TextStyle::default(),
///         );
///     });
/// }
/// ```
#[composable]
pub fn SelectionContainer(modifier: Modifier, content: impl FnMut() + 'static) {
    let selection = rememberMutableStateOf(|| None::<TextSelection>);
    let menu_open = rememberMutableStateOf(|| false);
    let registrar = remember(move || SelectionRegistrar::new(selection)).with(Clone::clone);
    let frame_clock = with_current_composer(|composer| composer.runtime_handle()).frame_clock();
    let gesture = {
        let registrar = registrar.clone();
        remember(move || Rc::new(SelectionGesture::new(registrar, frame_clock, menu_open)))
            .with(Rc::clone)
    };
    {
        let gesture = Rc::clone(&gesture);
        crate::UnhandledKeyEvents(move |event| gesture.on_key(event));
    }
    let content = Rc::new(RefCell::new(content));
    let pointer = Rc::clone(&gesture);
    CompositionLocalProvider(
        [local_selection_registrar().provides(Some(registrar.clone()))],
        move || {
            let content = Rc::clone(&content);
            Box(
                modifier.pointer_input((), move |scope| {
                    select_with_pointer(scope, Rc::clone(&pointer))
                }),
                BoxSpec::default(),
                move || (content.borrow_mut())(),
            );
        },
    );
    if menu_open.get() {
        selection_menu(&registrar, &gesture);
    }
}

/// Feeds the container's pointer input to its selection gesture.
async fn select_with_pointer(scope: PointerInputScope, gesture: Rc<SelectionGesture>) {
    scope
        .await_pointer_event_scope(|events| async move {
            loop {
                let event = events.await_pointer_event().await;
                if event.is_consumed() {
                    continue;
                }
                match event.kind {
                    PointerEventKind::Down => {
                        let secondary_only = event.buttons.contains(PointerButton::Secondary)
                            && !event.buttons.contains(PointerButton::Primary);
                        if !secondary_only {
                            gesture.on_down(&event);
                        }
                    }
                    PointerEventKind::Move => {
                        if gesture.on_move(&event) {
                            event.consume();
                        }
                    }
                    PointerEventKind::Up => gesture.on_up(),
                    PointerEventKind::Cancel => gesture.on_cancel(),
                    _ => {}
                }
            }
        })
        .await;
}

/// The Copy / Select all menu over a selection a finger made.
fn selection_menu(registrar: &SelectionRegistrar, gesture: &Rc<SelectionGesture>) {
    let Some(line) = registrar.first_highlight_in_window() else {
        return;
    };
    let copy = Rc::clone(gesture);
    let everything = registrar.clone();
    let items = vec![
        TextMenuItem::new(crate::UiString::Copy.resolve(), move || {
            copy.copy();
            copy.dismiss();
        }),
        TextMenuItem::new(crate::UiString::SelectAll.resolve(), move || {
            everything.select_all();
        }),
    ];
    let anchor = MenuAnchor {
        center_x: line.x + line.width * 0.5,
        line_top: line.y,
        line_bottom: line.y + line.height,
    };
    LiquidTextMenu(anchor, true, None, items);
}

/// Leaves the `Text` inside `content` out of the enclosing
/// [`SelectionContainer`], as Jetpack Compose's `DisableSelection` does.
#[composable]
pub fn DisableSelection(content: impl FnOnce()) {
    CompositionLocalProvider([local_selection_registrar().provides(None)], content);
}

/// What a `Text` in a [`SelectionContainer`] adds in front of its text: it
/// tells the container what it shows and where, and draws its part of the
/// selection behind its glyphs.
#[composable]
pub(crate) fn selectable_text(
    registrar: SelectionRegistrar,
    text: Rc<AnnotatedString>,
    style: TextStyle,
    options: TextLayoutOptions,
) -> Modifier {
    let key = {
        let registrar = registrar.clone();
        remember(move || registrar.subscribe()).with(|key| *key)
    };
    let geometry = remember(|| Rc::new(SelectableGeometry::default())).with(Rc::clone);
    registrar.update(key, text, style, options, Rc::clone(&geometry));
    {
        let registrar = registrar.clone();
        DisposableEffect(key, move |scope| {
            scope.on_dispose(move || registrar.unsubscribe(key))
        });
    }
    let reporter = Modifier::from_parts(&[modifier_element(SelectableTextElement::new(
        Rc::clone(&geometry),
    ))]);
    reporter.draw_behind(move |scope| {
        geometry.set_content_size(scope.size());
        for line in registrar.highlight(key) {
            scope.draw_rect_at(
                line,
                Brush::solid(crate::text_field_modifier_node::DEFAULT_SELECTION_COLOR),
            );
        }
    })
}

#[cfg(test)]
#[path = "tests/selection_container_tests.rs"]
mod tests;
