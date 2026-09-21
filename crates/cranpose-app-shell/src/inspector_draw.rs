use std::rc::Rc;

use cranpose_core::NodeId;
use cranpose_render_common::graph::{
    DrawPrimitiveNode, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, RenderGraph,
    RenderNode, TextPrimitiveNode,
};
use cranpose_ui::{TextLayoutOptions, TextStyle, text::AnnotatedString};
use cranpose_ui_graphics::{Brush, Color, CornerRadii, DrawPrimitive, Point, Rect, Size, Stroke};

use super::{InspectorAction, InspectorControl, InspectorMode, InspectorState};

const INK: Color = Color(0.92, 0.95, 1.0, 1.0);
const MUTED: Color = Color(0.67, 0.73, 0.83, 1.0);
const PANEL: Color = Color(0.055, 0.075, 0.115, 0.98);
const ACCENT: Color = Color(0.2, 0.75, 1.0, 1.0);
const WARNING: Color = Color(1.0, 0.65, 0.24, 1.0);
const ROW: f32 = 30.0;

pub(super) fn panel_bounds(state: &InspectorState, viewport: Size) -> Rect {
    let width = (viewport.width - 16.0).clamp(0.0, 390.0);
    let height = (viewport.height - 16.0).clamp(0.0, 620.0);
    floating_bounds(
        Rect {
            x: (viewport.width - width - 8.0).max(0.0),
            y: 8.0,
            width,
            height,
        },
        state.panel_position,
        viewport,
    )
}

pub(super) fn launcher_bounds(state: &InspectorState, viewport: Size) -> Rect {
    floating_bounds(
        Rect {
            x: (viewport.width - 158.0).max(0.0),
            y: (viewport.height - 56.0).max(0.0),
            width: 146.0_f32.min(viewport.width),
            height: 44.0_f32.min(viewport.height),
        },
        state.launcher_position,
        viewport,
    )
}

fn floating_bounds(mut bounds: Rect, position: Option<Point>, viewport: Size) -> Rect {
    if let Some(position) = position {
        bounds.x = position
            .x
            .clamp(0.0, (viewport.width - bounds.width).max(0.0));
        bounds.y = position
            .y
            .clamp(0.0, (viewport.height - bounds.height).max(0.0));
    }
    bounds
}

struct Canvas {
    children: Vec<RenderNode>,
    viewport: Rect,
}

impl Canvas {
    fn rectangle(&mut self, rect: Rect, color: Color, stroke: Option<Stroke>) {
        self.children.push(RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::AfterChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::RoundRect {
                    rect,
                    brush: Brush::Solid(color),
                    radii: CornerRadii::uniform(3.0),
                    stroke,
                },
                clip: None,
            }),
        }));
    }

    fn text(&mut self, text: &str, rect: Rect, color: Color, font_size: f32) {
        let mut style = TextStyle::default();
        style.span_style.color = Some(color);
        let id = NodeId::MAX.saturating_sub(1024 + self.children.len());
        self.children.push(RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::AfterChildren,
            node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
                node_id: id,
                rect,
                text: Rc::new(AnnotatedString::from(text)),
                text_style: style,
                font_size,
                layout_options: TextLayoutOptions::default(),
                clip: Some(rect),
            })),
        }));
    }

    fn button(
        &mut self,
        state: &mut InspectorState,
        action: InspectorAction,
        text: &str,
        bounds: Rect,
        selected: bool,
    ) {
        self.rectangle(
            bounds,
            if selected {
                Color(0.10, 0.28, 0.40, 1.0)
            } else {
                Color(0.13, 0.16, 0.22, 1.0)
            },
            None,
        );
        self.text(
            &truncate(text, bounds.width - 16.0),
            Rect {
                x: bounds.x + 8.0,
                y: bounds.y + 7.0,
                width: (bounds.width - 16.0).max(0.0),
                height: bounds.height - 7.0,
            },
            if selected { ACCENT } else { INK },
            12.0,
        );
        state.controls.push(InspectorControl { action, bounds });
    }

    fn finish(self) -> RenderGraph {
        let mut root = LayerNode {
            local_bounds: self.viewport,
            children: self.children,
            ..Default::default()
        };
        root.recompute_raster_cache_hashes();
        RenderGraph::new(root)
    }
}

pub(super) fn build(state: &mut InspectorState, viewport: Size) -> RenderGraph {
    state.controls.clear();
    let mut canvas = Canvas {
        children: Vec::new(),
        viewport: Rect::from_size(viewport),
    };
    if state.open {
        draw_nodes(&mut canvas, state);
    }
    if !state.open || state.picking {
        let bounds = launcher_bounds(state, viewport);
        if state.launcher_position.is_some() {
            state.launcher_position = Some(Point {
                x: bounds.x,
                y: bounds.y,
            });
        }
        let label = if state.picking {
            "Cancel picking"
        } else {
            ":: Inspector"
        };
        let action = if state.picking {
            InspectorAction::Pick
        } else {
            InspectorAction::Toggle
        };
        canvas.button(state, action, label, bounds, state.picking);
    } else {
        let panel = panel_bounds(state, viewport);
        if state.panel_position.is_some() {
            state.panel_position = Some(Point {
                x: panel.x,
                y: panel.y,
            });
        }
        draw_panel(&mut canvas, state, panel);
    }
    canvas.finish()
}

fn draw_nodes(canvas: &mut Canvas, state: &InspectorState) {
    if state.mode == InspectorMode::Accessibility {
        canvas.rectangle(canvas.viewport, Color(0.96, 0.97, 0.99, 1.0), None);
    }
    for (index, node) in state.nodes.iter().enumerate() {
        let selected = state.selected == Some(index);
        if state.mode == InspectorMode::Normal && !selected && !state.picking {
            continue;
        }
        let color = if node.issue {
            WARNING
        } else if selected {
            Color(0.80, 0.35, 1.0, 1.0)
        } else if node.focused {
            Color(0.1, 0.8, 0.4, 1.0)
        } else {
            ACCENT
        };
        canvas.rectangle(
            node.bounds,
            color,
            Some(Stroke::new(if selected { 3.0 } else { 1.5 })),
        );
        let label = if state.mode == InspectorMode::Accessibility {
            format!("{}  {}", index + 1, node.label)
        } else {
            (index + 1).to_string()
        };
        let width = if state.mode == InspectorMode::Accessibility {
            node.bounds.width.max(32.0)
        } else {
            32.0
        };
        let bounds = Rect {
            x: node.bounds.x.max(0.0),
            y: node.bounds.y.max(0.0),
            width,
            height: 22.0,
        };
        canvas.rectangle(bounds, PANEL, None);
        canvas.text(
            &truncate(&label, width),
            Rect {
                x: bounds.x + 3.0,
                y: bounds.y + 3.0,
                width: bounds.width - 6.0,
                height: 18.0,
            },
            INK,
            11.0,
        );
    }
}

fn draw_panel(canvas: &mut Canvas, state: &mut InspectorState, panel: Rect) {
    canvas.rectangle(panel, PANEL, None);
    canvas.button(
        state,
        InspectorAction::Move,
        ":: ACCESSIBILITY INSPECTOR",
        Rect {
            x: panel.x + 5.0,
            y: panel.y + 5.0,
            width: (panel.width - 54.0).max(0.0),
            height: 32.0,
        },
        false,
    );
    canvas.button(
        state,
        InspectorAction::Toggle,
        "X",
        Rect {
            x: panel.x + panel.width - 44.0,
            y: panel.y + 5.0,
            width: 36.0,
            height: 32.0,
        },
        false,
    );
    let modes = [
        (InspectorAction::Normal, "Normal", InspectorMode::Normal),
        (InspectorAction::Overlay, "Overlay", InspectorMode::Overlay),
        (
            InspectorAction::Accessibility,
            "A11y only",
            InspectorMode::Accessibility,
        ),
    ];
    let width = (panel.width - 32.0) / 3.0;
    for (index, (action, label, mode)) in modes.into_iter().enumerate() {
        canvas.button(
            state,
            action,
            label,
            Rect {
                x: panel.x + 8.0 + index as f32 * (width + 8.0),
                y: panel.y + 44.0,
                width,
                height: ROW,
            },
            state.mode == mode,
        );
    }
    for (index, (action, label)) in [
        (InspectorAction::Pick, "Pick element"),
        (InspectorAction::Previous, "Previous"),
        (InspectorAction::Next, "Next"),
    ]
    .into_iter()
    .enumerate()
    {
        canvas.button(
            state,
            action,
            label,
            Rect {
                x: panel.x + 8.0 + index as f32 * (width + 8.0),
                y: panel.y + 82.0,
                width,
                height: ROW,
            },
            false,
        );
    }
    let rows = ((panel.height * 0.40 - 124.0) / ROW).floor().max(1.0) as usize;
    let start = state.selected.unwrap_or(0) / rows * rows;
    let end = (start + rows).min(state.nodes.len());
    canvas.text(
        &format!("READING ORDER  {} elements", state.nodes.len()),
        Rect {
            x: panel.x + 12.0,
            y: panel.y + 124.0,
            width: panel.width - 24.0,
            height: 20.0,
        },
        MUTED,
        11.0,
    );
    for index in start..end {
        let label = truncate(
            &format!("{}  {}", index + 1, state.nodes[index].label),
            panel.width - 24.0,
        );
        canvas.button(
            state,
            InspectorAction::Select(index),
            &label,
            Rect {
                x: panel.x + 8.0,
                y: panel.y + 148.0 + (index - start) as f32 * ROW,
                width: panel.width - 16.0,
                height: ROW - 2.0,
            },
            state.selected == Some(index),
        );
    }
    draw_details(canvas, state, panel, panel.y + 156.0 + rows as f32 * ROW);
}

fn detail_text(state: &InspectorState) -> &str {
    state.selected.and_then(|index| state.nodes.get(index))
        .map(|node| node.details.as_str())
        .unwrap_or("Select from the list or use Pick element.\n\nBlue: accessible bounds\nGreen: app focus\nPurple: selected\nAmber: missing accessible name")
}

pub(super) fn detail_line_count(state: &InspectorState, viewport: Size) -> usize {
    wrapped_lines(
        detail_text(state),
        panel_bounds(state, viewport).width - 24.0,
    )
    .len()
}

fn draw_details(canvas: &mut Canvas, state: &mut InspectorState, panel: Rect, y: f32) {
    let lines = wrapped_lines(detail_text(state), panel.width - 24.0);
    state.detail_offset = state.detail_offset.min(lines.len().saturating_sub(1));
    let max_lines = ((panel.y + panel.height - 86.0 - y) / 17.0).max(0.0) as usize;
    for (index, line) in lines
        .into_iter()
        .skip(state.detail_offset)
        .take(max_lines)
        .enumerate()
    {
        canvas.text(
            &line,
            Rect {
                x: panel.x + 12.0,
                y: y + index as f32 * 17.0,
                width: panel.width - 24.0,
                height: 17.0,
            },
            INK,
            12.0,
        );
    }
    for (index, (action, label)) in [
        (InspectorAction::DetailsUp, "Details up"),
        (InspectorAction::DetailsDown, "Details down"),
    ]
    .into_iter()
    .enumerate()
    {
        let width = (panel.width - 24.0) / 2.0;
        canvas.button(
            state,
            action,
            label,
            Rect {
                x: panel.x + 8.0 + index as f32 * (width + 8.0),
                y: panel.y + panel.height - 78.0,
                width,
                height: ROW,
            },
            false,
        );
    }
    canvas.text(
        "Drag the title bar to move this panel",
        Rect {
            x: panel.x + 12.0,
            y: panel.y + panel.height - 37.0,
            width: panel.width - 24.0,
            height: 17.0,
        },
        MUTED,
        10.0,
    );
    canvas.text(
        "App accessibility projection",
        Rect {
            x: panel.x + 12.0,
            y: panel.y + panel.height - 21.0,
            width: panel.width - 24.0,
            height: 17.0,
        },
        MUTED,
        10.0,
    );
}

fn truncate(text: &str, width: f32) -> String {
    let capacity = (width / 7.0).max(1.0) as usize;
    if text.chars().count() <= capacity {
        return text.to_string();
    }
    text.chars()
        .take(capacity.saturating_sub(1))
        .chain(['…'])
        .collect()
}

fn wrapped_lines(text: &str, width: f32) -> Vec<String> {
    let capacity = (width / 7.0).max(1.0) as usize;
    text.lines()
        .flat_map(|line| {
            let chars: Vec<_> = line.chars().collect();
            if chars.is_empty() {
                vec![String::new()]
            } else {
                chars
                    .chunks(capacity)
                    .map(|chunk| chunk.iter().collect())
                    .collect()
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/inspector_draw_tests.rs"]
mod tests;
