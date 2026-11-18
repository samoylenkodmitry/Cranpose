use std::{ops::Range, rc::Rc};

use cranpose_macros::composable;
use cranpose_services::HapticFeedback;
use cranpose_ui::{
    Modifier, SemanticsWidgetRole, SharedText, VerticalAlignment,
    text::{FontWeight, TextStyle},
    widgets::{Box, BoxSpec, Row, RowSpec, Spacer, Text},
};
use cranpose_ui_layout::{Alignment, LinearArrangement};

use super::{
    button::{ButtonMetrics, GlassButtonSpec, GlassButtonWithFeedback},
    content_scope::ScopeContent,
    glass_surface::floating_material,
};
use crate::{
    material::LiquidModifierExt,
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
const TEXT_METRICS: ButtonMetrics = ButtonMetrics {
    height: HEIGHT,
    padding: (16.0, 0.0),
};

/// The scope a [`LiquidToolbar`]'s items are declared in, leading to
/// trailing.
pub struct LiquidToolbarScope {
    entries: ScopeContent<ToolbarEntry>,
}

impl LiquidToolbarScope {
    /// A button reading `label`, on a capsule of its own.
    pub fn button(&self, label: impl Into<SharedText>, on_click: impl Fn() + 'static) {
        self.text(label.into(), TextKind::Plain, Rc::new(on_click));
    }

    /// A button reading `label` on the accent color, for the primary action.
    pub fn prominent_button(&self, label: impl Into<SharedText>, on_click: impl Fn() + 'static) {
        self.text(label.into(), TextKind::Prominent, Rc::new(on_click));
    }

    /// A button showing `icon`, an SVG path in a 24 unit box, that readers
    /// hear as `description`. Adjacent icon buttons share one capsule.
    pub fn icon_button(
        &self,
        icon: &'static str,
        description: impl Into<SharedText>,
        on_click: impl Fn() + 'static,
    ) {
        self.entries.push(ToolbarEntry::Icon {
            path: icon,
            description: description.into(),
            on_click: Rc::new(on_click),
        });
    }

    /// Room that pushes the items after it to the far end. Several share the
    /// room equally.
    pub fn flexible_space(&self) {
        self.entries.push(ToolbarEntry::FlexibleSpace);
    }

    fn text(&self, label: SharedText, kind: TextKind, on_click: Rc<dyn Fn()>) {
        self.entries.push(ToolbarEntry::Text {
            label,
            kind,
            on_click,
        });
    }
}

#[derive(Clone, Copy)]
enum TextKind {
    Plain,
    Prominent,
}

enum ToolbarEntry {
    Text {
        label: SharedText,
        kind: TextKind,
        on_click: Rc<dyn Fn()>,
    },
    Icon {
        path: &'static str,
        description: SharedText,
        on_click: Rc<dyn Fn()>,
    },
    FlexibleSpace,
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
        ScopeContent::collect(|entries| LiquidToolbarScope { entries }, content).into(),
    );
}

#[composable(no_skip)]
fn ToolbarRow(modifier: Modifier, entries: Rc<[ToolbarEntry]>) {
    let body = liquid_typography().body;
    Row(
        modifier
            .fill_max_width()
            .padding_each(MARGIN, 0.0, MARGIN, MARGIN),
        RowSpec::default()
            .horizontal_arrangement(LinearArrangement::spaced_by(GAP))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let mut index = 0;
            while let Some(entry) = entries.get(index) {
                match entry {
                    ToolbarEntry::Text {
                        label,
                        kind,
                        on_click,
                    } => TextItem(label.clone(), *kind, body.clone(), Rc::clone(on_click)),
                    ToolbarEntry::Icon { .. } => {
                        let run = entries[index..]
                            .iter()
                            .take_while(|entry| matches!(entry, ToolbarEntry::Icon { .. }))
                            .count();
                        IconGroup(Rc::clone(&entries), index..index + run);
                        index += run;
                        continue;
                    }
                    ToolbarEntry::FlexibleSpace => {
                        Spacer(Modifier::empty().weight(1.0));
                    }
                }
                index += 1;
            }
        },
    );
}

#[composable(no_skip)]
fn TextItem(label: SharedText, kind: TextKind, mut style: TextStyle, on_click: Rc<dyn Fn()>) {
    let colors = liquid_colors();
    let (spec, weight) = match kind {
        TextKind::Plain => (
            GlassButtonSpec::glass().with_content_color(colors.label),
            FontWeight::MEDIUM,
        ),
        TextKind::Prominent => (GlassButtonSpec::prominent(), FontWeight::SEMI_BOLD),
    };
    style.span_style.color = Some(spec.content_color(&colors));
    style.span_style.font_weight = Some(weight);
    GlassButtonWithFeedback(
        Modifier::empty(),
        spec,
        TEXT_METRICS,
        HapticFeedback::ImpactLight,
        None,
        move || on_click(),
        move || {
            Text(label.clone(), Modifier::empty(), style.clone());
        },
    );
}

// The icons of `entries` in `run`, on one capsule.
#[composable(no_skip)]
fn IconGroup(entries: Rc<[ToolbarEntry]>, run: Range<usize>) {
    let color = liquid_colors().label;
    let width = (ICON_SLOT * run.len() as f32 - ICON_GROUP_TRIM).max(HEIGHT);
    Row(
        Modifier::empty()
            .width(width)
            .height(HEIGHT)
            .glass_effect(floating_material(color, HEIGHT)),
        RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            for entry in &entries[run.clone()] {
                let ToolbarEntry::Icon {
                    path,
                    description,
                    on_click,
                } = entry
                else {
                    continue;
                };
                let (path, on_click) = (*path, Rc::clone(on_click));
                Box(
                    Modifier::empty()
                        .weight(1.0)
                        .fill_max_height()
                        .clickable(move |_| on_click())
                        .role(SemanticsWidgetRole::Button)
                        .content_description(description.as_str()),
                    BoxSpec::default().content_alignment(Alignment::CENTER),
                    move || crate::icons::Icon(path, None, ICON_SIZE, color),
                );
            }
        },
    );
}
