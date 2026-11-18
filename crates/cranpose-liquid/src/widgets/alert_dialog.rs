use std::rc::Rc;

use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, VerticalAlignment,
    widgets::{Column, ColumnSpec, DialogSpec, DialogWithScrim, DismissReason, Row, RowSpec, Text},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii};
use cranpose_ui_layout::LinearArrangement;

use super::label_button::LabelButton;
use crate::{
    material::{Glass, GlassShadow, LiquidModifierExt, LiquidShape},
    theme::{LiquidColors, liquid_colors, liquid_typography},
    widgets::content_scope::ScopeContent,
};

// The iOS 27 alert, measured on the simulator at 3x: the window dims with
// black at 20% in light mode and 48% in dark mode; a frosted card 320
// points wide with corners of 34 holds the
// semibold title and the message, its lines 20 points apart, 31 points in
// from its sides, then capsules 48 points tall, 22 apart and 22.67 in from
// the card's sides, the last 15.67 above its bottom.
const SCRIM_LIGHT: f32 = 0.2;
const SCRIM_DARK: f32 = 0.48;
const CARD_WIDTH: f32 = 320.0;
const CARD_RADIUS: f32 = 34.0;
const BUTTON_INSET: f32 = 68.0 / 3.0;
const TEXT_INSET: f32 = 31.0 - BUTTON_INSET;
const TOP_PADDING: f32 = 65.0 / 3.0;
const BOTTOM_PADDING: f32 = 47.0 / 3.0;
const MESSAGE_GAP: f32 = 22.0 / 3.0;
const MESSAGE_LINE: f32 = 20.0;
const BUTTONS_GAP: f32 = 21.0;
const BUTTON_HEIGHT: f32 = 48.0;
const BUTTON_SPACING: f32 = 22.0;

/// What an action of a [`LiquidAlertDialog`] does, which sets its color and
/// its place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiquidAlertRole {
    /// An ordinary action, in the accent color.
    Default,
    /// The action that backs out, in the accent color. Of two actions it sits
    /// on the left; the back gesture runs it.
    Cancel,
    /// An action that destroys data, in the destructive color.
    Destructive,
}

/// The scope a [`LiquidAlertDialog`]'s actions are declared in.
pub struct LiquidAlertScope {
    actions: ScopeContent<AlertAction>,
}

impl LiquidAlertScope {
    /// An action reading `label` in the look of `role` that runs `on_click`
    /// and closes the alert.
    pub fn action(
        &self,
        label: impl Into<String>,
        role: LiquidAlertRole,
        on_click: impl Fn() + 'static,
    ) {
        self.actions.push(AlertAction {
            label: label.into(),
            role,
            on_click: Rc::new(on_click),
        });
    }
}

struct AlertAction {
    label: String,
    role: LiquidAlertRole,
    on_click: Rc<dyn Fn()>,
}

/// A modal alert: `title`, `text` under it, then the actions declared in
/// `actions`. Two actions sit side by side with the cancel action on the
/// left; any other number stacks. A tap outside does nothing; the back
/// gesture runs the cancel action. Every action calls `on_dismiss_request`
/// after its own, as the alert closes.
///
/// UIKit's `UIAlertController` with the alert style and SwiftUI's `alert`;
/// Compose's `AlertDialog`.
#[composable]
pub fn LiquidAlertDialog(
    on_dismiss_request: impl Fn() + 'static,
    title: &str,
    text: &str,
    actions: impl FnOnce(&LiquidAlertScope),
) {
    // Only this shell is generic, so every call site shares the body below.
    AlertCard(
        Rc::new(on_dismiss_request),
        title.to_owned(),
        text.to_owned(),
        ScopeContent::collect(|actions| LiquidAlertScope { actions }, actions),
    );
}

// The dialog, its card and the actions.
#[composable(no_skip)]
fn AlertCard(on_dismiss: Rc<dyn Fn()>, title: String, text: String, actions: Vec<AlertAction>) {
    let mut actions = actions;
    if actions.len() == 2 && actions[1].role == LiquidAlertRole::Cancel {
        actions.swap(0, 1);
    }
    let actions = Rc::new(actions);
    let back = {
        let actions = Rc::clone(&actions);
        let on_dismiss = Rc::clone(&on_dismiss);
        move |reason: DismissReason| {
            if reason == DismissReason::BackRequested {
                if let Some(cancel) = actions
                    .iter()
                    .find(|action| action.role == LiquidAlertRole::Cancel)
                {
                    (cancel.on_click)();
                }
                on_dismiss();
            }
        }
    };
    let spec = DialogSpec {
        dismiss_on_outside_tap: false,
        ..DialogSpec::default()
    };
    let scrim = if liquid_colors().is_dark {
        SCRIM_DARK
    } else {
        SCRIM_LIGHT
    };
    DialogWithScrim(spec, Color::BLACK.with_alpha(scrim), back, move || {
        let colors = liquid_colors();
        let typography = liquid_typography();
        let mut title_style = typography.headline.clone();
        title_style.span_style.color = Some(colors.label);
        let mut text_style = typography.subheadline;
        text_style.span_style.color = Some(colors.secondary_label);
        // The message's lines stand 20 points apart.
        text_style.paragraph_style.line_height = cranpose_ui::text::TextUnit::Sp(MESSAGE_LINE);
        let face = Modifier::empty()
            .width(CARD_WIDTH)
            .glass_effect(face_material(&colors));
        let (title, text) = (title.clone(), text.clone());
        let (actions, on_dismiss) = (Rc::clone(&actions), Rc::clone(&on_dismiss));
        Column(
            face.padding_each(BUTTON_INSET, TOP_PADDING, BUTTON_INSET, BOTTOM_PADDING),
            ColumnSpec::default(),
            move || {
                let text_padding = Modifier::empty().padding_each(TEXT_INSET, 0.0, TEXT_INSET, 0.0);
                Text(title.clone(), text_padding.clone(), title_style.clone());
                Text(
                    text.clone(),
                    text_padding.padding_each(0.0, MESSAGE_GAP, 0.0, BUTTONS_GAP),
                    text_style.clone(),
                );
                AlertActions(&actions, &on_dismiss);
            },
        );
    });
}

// The card's frosted face, fitted to the native card over a solid and a
// checkered backdrop: the backdrop blurred by 12 points with its color
// raised, under white at 64% in light mode or gray 100 at 32% in dark mode,
// and a soft shadow.
fn face_material(colors: &LiquidColors) -> Glass {
    let tint = if colors.is_dark {
        Color::from_rgba_u8(100, 100, 100, 82)
    } else {
        Color::from_rgba_u8(255, 255, 255, 163)
    };
    Glass::regular()
        .face_lighting(false)
        .lift(0.0)
        .contrast(1.0)
        .blur_radius(12.0)
        .tint(tint)
        .saturation(1.4)
        .shadow_style(GlassShadow::new(
            Color::BLACK.with_alpha(0.162),
            32.0,
            8.0,
            1.5,
        ))
        .shape(LiquidShape::RoundedRect(CARD_RADIUS))
}

// The actions: side by side when there are two, else stacked.
#[composable(no_skip)]
fn AlertActions(actions: &Rc<Vec<AlertAction>>, on_dismiss: &Rc<dyn Fn()>) {
    let (actions, on_dismiss) = (Rc::clone(actions), Rc::clone(on_dismiss));
    if actions.len() == 2 {
        Row(
            Modifier::empty().fill_max_width(),
            RowSpec::default()
                .horizontal_arrangement(LinearArrangement::spaced_by(BUTTON_SPACING))
                .vertical_alignment(VerticalAlignment::CenterVertically),
            move || {
                for index in 0..actions.len() {
                    AlertButton(Modifier::empty().weight(1.0), &actions, index, &on_dismiss);
                }
            },
        );
        return;
    }
    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::default()
            .vertical_arrangement(LinearArrangement::spaced_by(BUTTON_SPACING * 0.5)),
        move || {
            for index in 0..actions.len() {
                AlertButton(
                    Modifier::empty().fill_max_width(),
                    &actions,
                    index,
                    &on_dismiss,
                );
            }
        },
    );
}

// One action's capsule: its label in the color of its role.
#[composable(no_skip)]
fn AlertButton(
    modifier: Modifier,
    actions: &Rc<Vec<AlertAction>>,
    index: usize,
    on_dismiss: &Rc<dyn Fn()>,
) {
    let Some(action) = actions.get(index) else {
        return;
    };
    let colors = liquid_colors();
    let mut style = liquid_typography().body;
    style.span_style.color = Some(match action.role {
        LiquidAlertRole::Destructive => colors.destructive,
        LiquidAlertRole::Default | LiquidAlertRole::Cancel => colors.accent,
    });
    let fill = colors.system_fill;
    let (on_click, on_dismiss) = (Rc::clone(&action.on_click), Rc::clone(on_dismiss));
    LabelButton(
        modifier.height(BUTTON_HEIGHT).draw_behind(move |scope| {
            scope.draw_round_rect(
                Brush::solid(fill),
                CornerRadii::uniform(BUTTON_HEIGHT * 0.5),
            );
        }),
        &action.label,
        style,
        move || {
            on_click();
            on_dismiss();
        },
    );
}
