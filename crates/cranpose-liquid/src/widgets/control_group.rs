use std::rc::Rc;

use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, SemanticsConfiguration, SemanticsWidgetRole, VerticalAlignment,
    widgets::{Box, BoxSpec, Row, RowSpec, Text},
};
use cranpose_ui_graphics::{Brush, CornerRadii};
use cranpose_ui_layout::Alignment;

use crate::{
    theme::{liquid_colors, liquid_typography},
    widgets::content_scope::ScopeContent,
};

// The iOS 27 control group, measured on the simulator at 3x: a capsule 32
// points tall in the tertiary fill, split into equal parts with no divider,
// each part's label centered in the footnote font and the label color.
const HEIGHT: f32 = 32.0;

/// The scope a [`LiquidControlGroup`]'s buttons are declared in, left to
/// right.
pub struct LiquidControlGroupScope {
    buttons: ScopeContent<ControlGroupButton>,
}

impl LiquidControlGroupScope {
    /// A button reading `label` that runs `on_click` when tapped.
    pub fn button(&self, label: impl Into<String>, on_click: impl Fn() + 'static) {
        self.buttons.push(ControlGroupButton {
            label: label.into(),
            on_click: Rc::new(on_click),
        });
    }
}

struct ControlGroupButton {
    label: String,
    on_click: Rc<dyn Fn()>,
}

/// Buttons joined in one capsule, each taking an equal share of its width.
/// Readers hear each as a button with its label.
///
/// SwiftUI's `ControlGroup` outside a toolbar or menu.
#[composable]
pub fn LiquidControlGroup(modifier: Modifier, content: impl FnOnce(&LiquidControlGroupScope)) {
    // Only this shell is generic, so every call site shares the body below.
    ControlGroupCapsule(
        modifier,
        ScopeContent::collect(|buttons| LiquidControlGroupScope { buttons }, content),
    );
}

// The capsule and its buttons.
#[composable(no_skip)]
fn ControlGroupCapsule(modifier: Modifier, buttons: Vec<ControlGroupButton>) {
    let colors = liquid_colors();
    let mut style = liquid_typography().footnote;
    style.span_style.color = Some(colors.label);
    let fill = colors.tertiary_fill;
    Row(
        modifier.height(HEIGHT).draw_behind(move |scope| {
            scope.draw_round_rect(Brush::solid(fill), CornerRadii::uniform(HEIGHT * 0.5));
        }),
        RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            for button in &buttons {
                let on_click = Rc::clone(&button.on_click);
                let label = button.label.clone();
                let reader_label = label.clone();
                let style = style.clone();
                Box(
                    Modifier::empty()
                        .weight(1.0)
                        .fill_max_height()
                        .clickable(move |_| on_click())
                        .semantics(move |config: &mut SemanticsConfiguration| {
                            config.role = Some(SemanticsWidgetRole::Button);
                            config.content_description = Some(reader_label.clone());
                        }),
                    BoxSpec::default().content_alignment(Alignment::CENTER),
                    move || {
                        Text(
                            label.clone(),
                            Modifier::empty().hide_from_accessibility(),
                            style.clone(),
                        );
                    },
                );
            }
        },
    );
}
