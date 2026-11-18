use std::{mem, rc::Rc};

use cranpose_macros::composable;
use cranpose_services::HapticFeedback;
use cranpose_ui::{
    Modifier, SemanticsConfiguration, SemanticsWidgetRole, SharedText, VerticalAlignment,
    text::FontWeight,
    widgets::{Box, BoxSpec, Row, RowSpec, Spacer, Text},
};
use cranpose_ui_layout::{Alignment, LinearArrangement};

use super::{
    button::{ButtonMetrics, GlassButtonSpec, GlassButtonWithFeedback},
    glass_surface::floating_material,
};
use crate::{
    material::{LiquidModifierExt, LiquidShape},
    theme::{liquid_colors, liquid_typography},
};

// The iOS 27 bottom toolbar, measured on the simulator at 3x. Every capsule
// is 48 points tall and sits 28 points in from the window's sides and bottom,
// 12 points from the next. A text item takes a capsule of its own; adjacent
// icons share one, 60 points a member less 15, at least 48, split equally.
const HEIGHT: f32 = 48.0;
const MARGIN: f32 = 28.0;
const GAP: f32 = 12.0;
const ICON_SLOT: f32 = 60.0;
const ICON_GROUP_TRIM: f32 = 15.0;
const ICON_SIZE: f32 = 30.0;
const TEXT_PADDING: f32 = 16.0;

/// The scope a [`LiquidToolbar`]'s items are declared in, leading to
/// trailing.
pub struct LiquidToolbarScope {
    entries: super::content_scope::ScopeContent<ToolbarEntry>,
}

impl LiquidToolbarScope {
    /// A button reading `label`, on a capsule of its own.
    pub fn button(&self, label: impl Into<SharedText>, on_click: impl Fn() + 'static) {
        self.entries.push(ToolbarEntry::Text(ToolbarText {
            label: label.into(),
            prominent: false,
            on_click: Rc::new(on_click),
        }));
    }

    /// A button reading `label` on the accent color, for the primary action.
    pub fn prominent_button(&self, label: impl Into<SharedText>, on_click: impl Fn() + 'static) {
        self.entries.push(ToolbarEntry::Text(ToolbarText {
            label: label.into(),
            prominent: true,
            on_click: Rc::new(on_click),
        }));
    }

    /// A button showing `icon`, an SVG path in a 24 unit box, that readers
    /// hear as `description`. Adjacent icon buttons share one capsule.
    pub fn icon_button(
        &self,
        icon: &'static str,
        description: impl Into<String>,
        on_click: impl Fn() + 'static,
    ) {
        self.entries.push(ToolbarEntry::Icon(ToolbarIcon {
            path: icon,
            description: description.into(),
            on_click: Rc::new(on_click),
        }));
    }

    /// Room that pushes the items after it to the far end. Several share the
    /// room equally.
    pub fn flexible_space(&self) {
        self.entries.push(ToolbarEntry::FlexibleSpace);
    }
}

enum ToolbarEntry {
    Text(ToolbarText),
    Icon(ToolbarIcon),
    FlexibleSpace,
}

struct ToolbarText {
    label: SharedText,
    prominent: bool,
    on_click: Rc<dyn Fn()>,
}

struct ToolbarIcon {
    path: &'static str,
    description: String,
    on_click: Rc<dyn Fn()>,
}

// What the toolbar draws: a text capsule, a capsule of adjacent icons or a
// flexible space.
enum ToolbarPart {
    Text(ToolbarText),
    Icons(Rc<[ToolbarIcon]>),
    FlexibleSpace,
}

fn toolbar_parts(entries: Vec<ToolbarEntry>) -> Vec<ToolbarPart> {
    let mut parts = Vec::with_capacity(entries.len());
    let mut icons = Vec::new();
    for entry in entries {
        let part = match entry {
            ToolbarEntry::Icon(icon) => {
                icons.push(icon);
                continue;
            }
            ToolbarEntry::Text(text) => ToolbarPart::Text(text),
            ToolbarEntry::FlexibleSpace => ToolbarPart::FlexibleSpace,
        };
        if !icons.is_empty() {
            parts.push(ToolbarPart::Icons(mem::take(&mut icons).into()));
        }
        parts.push(part);
    }
    if !icons.is_empty() {
        parts.push(ToolbarPart::Icons(icons.into()));
    }
    parts
}

/// A row of glass capsules along the bottom of the window. Place it at the
/// window's bottom edge: it keeps its capsules 28 points in from the sides
/// and the bottom, over the home indicator's inset. Readers hear each item as
/// a button with its label or description.
///
/// UIKit's `UIToolbar` and SwiftUI's `toolbar` in the bottom bar placement.
#[composable]
pub fn LiquidToolbar(modifier: Modifier, content: impl FnOnce(&LiquidToolbarScope)) {
    // Only this shell is generic, so every call site shares the body below.
    ToolbarRow(
        modifier,
        toolbar_parts(super::content_scope::ScopeContent::collect(
            |entries| LiquidToolbarScope { entries },
            content,
        )),
    );
}

#[composable(no_skip)]
fn ToolbarRow(modifier: Modifier, parts: Vec<ToolbarPart>) {
    Row(
        modifier
            .fill_max_width()
            .padding_each(MARGIN, 0.0, MARGIN, MARGIN),
        RowSpec::default()
            .horizontal_arrangement(LinearArrangement::spaced_by(GAP))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            for part in &parts {
                match part {
                    ToolbarPart::Text(text) => TextItem(
                        text.label.clone(),
                        text.prominent,
                        Rc::clone(&text.on_click),
                    ),
                    ToolbarPart::Icons(icons) => IconGroup(Rc::clone(icons)),
                    ToolbarPart::FlexibleSpace => {
                        Spacer(Modifier::empty().weight(1.0));
                    }
                }
            }
        },
    );
}

#[composable(no_skip)]
fn TextItem(label: SharedText, prominent: bool, on_click: Rc<dyn Fn()>) {
    let colors = liquid_colors();
    let spec = if prominent {
        GlassButtonSpec::prominent()
    } else {
        GlassButtonSpec::glass().with_content_color(colors.label)
    };
    let mut style = liquid_typography().body;
    style.span_style.color = Some(spec.content_color(&colors));
    style.span_style.font_weight = Some(if prominent {
        FontWeight::SEMI_BOLD
    } else {
        FontWeight::MEDIUM
    });
    GlassButtonWithFeedback(
        Modifier::empty(),
        spec,
        ButtonMetrics {
            height: HEIGHT,
            padding: (TEXT_PADDING, 0.0),
        },
        HapticFeedback::ImpactLight,
        None,
        move || on_click(),
        move || {
            Text(label.clone(), Modifier::empty(), style.clone());
        },
    );
}

#[composable(no_skip)]
fn IconGroup(icons: Rc<[ToolbarIcon]>) {
    let colors = liquid_colors();
    let width = (ICON_SLOT * icons.len() as f32 - ICON_GROUP_TRIM).max(HEIGHT);
    let material = floating_material(colors.label, HEIGHT).shape(LiquidShape::Capsule);
    Row(
        Modifier::empty()
            .width(width)
            .height(HEIGHT)
            .glass_effect(material),
        RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            for icon in icons.iter() {
                let on_click = Rc::clone(&icon.on_click);
                let description = icon.description.clone();
                let path = icon.path;
                let color = colors.label;
                Box(
                    Modifier::empty()
                        .weight(1.0)
                        .fill_max_height()
                        .clickable(move |_| on_click())
                        .semantics(move |config: &mut SemanticsConfiguration| {
                            config.role = Some(SemanticsWidgetRole::Button);
                            config.content_description = Some(description.clone());
                        }),
                    BoxSpec::default().content_alignment(Alignment::CENTER),
                    move || crate::icons::Icon(path, None, ICON_SIZE, color),
                );
            }
        },
    );
}
