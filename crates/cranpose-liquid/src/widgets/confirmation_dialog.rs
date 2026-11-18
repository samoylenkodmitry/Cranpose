use std::rc::Rc;

use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, Rect,
    widgets::{Column, ColumnSpec, DismissReason, Text},
};

use super::{
    alert_dialog::{
        AlertAction, LiquidAlertRole, LiquidAlertScope, StackedActions, collect_actions,
        text_styles,
    },
    popover::{PopoverAnchor, PopoverCard},
};

// The iOS 27 confirmation dialog on iPhone, measured on the simulator at 3x:
// a popover card 240 points wide holds the semibold title and the message 30
// points in from its sides, then the actions as capsules 48 points tall, 16
// in from the card's sides and its bottom. The cancel action is not drawn.
const CARD_WIDTH: f32 = 240.0;
const BUTTON_INSET: f32 = 16.0;
const TEXT_INSET: f32 = 30.0 - BUTTON_INSET;
const TOP_PADDING: f32 = 22.0;
const MESSAGE_GAP: f32 = 23.0 / 3.0;
const BUTTONS_GAP: f32 = 61.0 / 3.0;

/// A confirmation dialog: `title` and `message` over the actions declared in
/// `actions`, on a popover that points at `anchor_content` while `presented`.
/// The cancel action is not drawn: a tap outside the card or the back gesture
/// runs it. Every action calls `on_dismiss_request` after its own, as the
/// dialog closes.
///
/// SwiftUI's `confirmationDialog`; UIKit's `UIAlertController` with the
/// action sheet style.
#[composable]
pub fn LiquidConfirmationDialog<A>(
    modifier: Modifier,
    presented: bool,
    on_dismiss_request: impl Fn() + 'static,
    title: &str,
    message: &str,
    actions: impl FnOnce(&LiquidAlertScope),
    anchor_content: A,
) where
    A: FnMut() + 'static,
{
    let anchor = PopoverAnchor(modifier, anchor_content);
    if presented {
        ConfirmationCard(
            anchor,
            Rc::new(on_dismiss_request),
            (title.to_owned(), message.to_owned()),
            collect_actions(actions),
        );
    }
}

// The popover with the title, the message and every action but the cancel
// one, which a dismissal runs.
#[composable(no_skip)]
fn ConfirmationCard(
    anchor: Rect,
    on_dismiss: Rc<dyn Fn()>,
    (title, message): (String, String),
    actions: Vec<AlertAction>,
) {
    let (cancel, shown): (Vec<AlertAction>, Vec<AlertAction>) = actions
        .into_iter()
        .partition(|action| action.role == LiquidAlertRole::Cancel);
    let cancel = cancel.into_iter().next().map(|action| action.on_click);
    let dismiss: Rc<dyn Fn(DismissReason)> = {
        let on_dismiss = Rc::clone(&on_dismiss);
        Rc::new(move |_| {
            if let Some(cancel) = &cancel {
                cancel();
            }
            on_dismiss();
        })
    };
    let shown = Rc::new(shown);
    let content = move || {
        let (title_style, text_style) = text_styles();
        let (title, message) = (title.clone(), message.clone());
        let (shown, on_dismiss) = (Rc::clone(&shown), Rc::clone(&on_dismiss));
        Column(
            Modifier::empty().width(CARD_WIDTH).padding_each(
                BUTTON_INSET,
                TOP_PADDING,
                BUTTON_INSET,
                BUTTON_INSET,
            ),
            ColumnSpec::default(),
            move || {
                let text_padding = Modifier::empty().padding_each(TEXT_INSET, 0.0, TEXT_INSET, 0.0);
                Text(title.clone(), text_padding.clone(), title_style.clone());
                Text(
                    message.clone(),
                    text_padding.padding_each(0.0, MESSAGE_GAP, 0.0, BUTTONS_GAP),
                    text_style.clone(),
                );
                StackedActions(&shown, &on_dismiss);
            },
        );
    };
    PopoverCard(anchor, dismiss, Rc::new(content));
}
