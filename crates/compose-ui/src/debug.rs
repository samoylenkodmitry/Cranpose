//! Debug utilities for inspecting the current screen state
//!
//! This module provides utilities to log and format the UI hierarchy and render operations,
//! making it easier to debug layout and rendering issues.
//!
//! # Usage
//!
//! ```rust,ignore
//! use compose_ui::{log_layout_tree, log_render_scene, log_screen_summary};
//!
//! // After computing layout
//! let layout_tree = applier.compute_layout(root, viewport_size)?;
//! log_layout_tree(&layout_tree);
//!
//! // After rendering
//! let renderer = HeadlessRenderer::new();
//! let render_scene = renderer.render(&layout_tree);
//! log_render_scene(&render_scene);
//!
//! // Or get a quick summary
//! log_screen_summary(&layout_tree, &render_scene);
//! ```

use crate::layout::{LayoutBox, LayoutTree};
use crate::modifier::{ModifierChainInspectorNode, ModifierInspectorRecord};
use crate::renderer::{RecordedRenderScene, RenderOp};
use compose_foundation::{ModifierNodeChain, NodeCapabilities};
use std::fmt::Write;
use std::sync::{Arc, Mutex, OnceLock};

/// Logs the current layout tree to stdout with indentation showing hierarchy
pub fn log_layout_tree(layout: &LayoutTree) {
    println!("\n=== LAYOUT TREE (Current Screen) ===");
    log_layout_box(layout.root(), 0);
    println!("=== END LAYOUT TREE ===\n");
}

fn log_layout_box(layout_box: &LayoutBox, depth: usize) {
    let indent = "  ".repeat(depth);
    let rect = &layout_box.rect;

    println!(
        "{}[Node #{}] pos: ({:.1}, {:.1}), size: ({:.1}x{:.1})",
        indent, layout_box.node_id, rect.x, rect.y, rect.width, rect.height
    );

    for child in &layout_box.children {
        log_layout_box(child, depth + 1);
    }
}

/// Logs the current render scene to stdout showing all draw operations
pub fn log_render_scene(scene: &RecordedRenderScene) {
    println!("\n=== RENDER SCENE (Current Screen) ===");
    println!("Total operations: {}", scene.operations().len());

    for (idx, op) in scene.operations().iter().enumerate() {
        match op {
            RenderOp::Primitive {
                node_id,
                layer,
                primitive,
            } => {
                println!(
                    "[{}] Node #{} - Layer: {:?}, Primitive: {:?}",
                    idx, node_id, layer, primitive
                );
            }
            RenderOp::Text {
                node_id,
                rect,
                value,
            } => {
                println!(
                    "[{}] Node #{} - Text at ({:.1}, {:.1}): \"{}\"",
                    idx, node_id, rect.x, rect.y, value
                );
            }
        }
    }
    println!("=== END RENDER SCENE ===\n");
}

/// Returns a formatted string representation of the layout tree
pub fn format_layout_tree(layout: &LayoutTree) -> String {
    let mut output = String::new();
    writeln!(output, "=== LAYOUT TREE (Current Screen) ===").ok();
    format_layout_box(&mut output, layout.root(), 0);
    writeln!(output, "=== END LAYOUT TREE ===").ok();
    output
}

fn format_layout_box(output: &mut String, layout_box: &LayoutBox, depth: usize) {
    let indent = "  ".repeat(depth);
    let rect = &layout_box.rect;

    writeln!(
        output,
        "{}[Node #{}] pos: ({:.1}, {:.1}), size: ({:.1}x{:.1})",
        indent, layout_box.node_id, rect.x, rect.y, rect.width, rect.height
    )
    .ok();

    for child in &layout_box.children {
        format_layout_box(output, child, depth + 1);
    }
}

/// Returns a formatted string representation of the render scene
pub fn format_render_scene(scene: &RecordedRenderScene) -> String {
    let mut output = String::new();
    writeln!(output, "=== RENDER SCENE (Current Screen) ===").ok();
    writeln!(output, "Total operations: {}", scene.operations().len()).ok();

    for (idx, op) in scene.operations().iter().enumerate() {
        match op {
            RenderOp::Primitive {
                node_id,
                layer,
                primitive,
            } => {
                writeln!(
                    output,
                    "[{}] Node #{} - Layer: {:?}, Primitive: {:?}",
                    idx, node_id, layer, primitive
                )
                .ok();
            }
            RenderOp::Text {
                node_id,
                rect,
                value,
            } => {
                writeln!(
                    output,
                    "[{}] Node #{} - Text at ({:.1}, {:.1}): \"{}\"",
                    idx, node_id, rect.x, rect.y, value
                )
                .ok();
            }
        }
    }
    writeln!(output, "=== END RENDER SCENE ===").ok();
    output
}

/// Logs a compact summary of what's on screen (counts by type)
pub fn log_screen_summary(layout: &LayoutTree, scene: &RecordedRenderScene) {
    println!("\n=== SCREEN SUMMARY ===");
    println!("Total nodes in layout: {}", count_nodes(layout.root()));

    let mut text_count = 0;
    let mut primitive_count = 0;

    for op in scene.operations() {
        match op {
            RenderOp::Text { .. } => text_count += 1,
            RenderOp::Primitive { .. } => primitive_count += 1,
        }
    }

    println!("Render operations:");
    println!("  - Text elements: {}", text_count);
    println!("  - Primitive shapes: {}", primitive_count);
    println!("=== END SUMMARY ===\n");
}

fn count_nodes(layout_box: &LayoutBox) -> usize {
    1 + layout_box
        .children
        .iter()
        .map(count_nodes)
        .sum::<usize>()
}

/// Logs the contents of a modifier node chain including capabilities.
pub fn log_modifier_chain(chain: &ModifierNodeChain, nodes: &[ModifierChainInspectorNode]) {
    let dump = format_modifier_chain(chain, nodes);
    print!("{}", dump);
}

/// Formats the modifier chain using inspector data.
pub fn format_modifier_chain(
    chain: &ModifierNodeChain,
    nodes: &[ModifierChainInspectorNode],
) -> String {
    let mut output = String::new();
    writeln!(output, "\n=== MODIFIER CHAIN ===").ok();
    writeln!(
        output,
        "Total nodes: {} (entries: {})",
        nodes.len(),
        chain.len()
    )
    .ok();
    writeln!(
        output,
        "Aggregated capabilities: {}",
        describe_capabilities(chain.capabilities())
    )
    .ok();
    for node in nodes {
        let indent = "  ".repeat(node.depth);
        let inspector = node
            .inspector
            .as_ref()
            .map(describe_inspector)
            .unwrap_or_default();
        let inspector_suffix = if inspector.is_empty() {
            String::new()
        } else {
            format!(" {inspector}")
        };
        writeln!(
            output,
            "{}- {} caps={} agg={}{}",
            indent,
            node.type_name,
            describe_capabilities(node.capabilities),
            describe_capabilities(node.aggregate_child_capabilities),
            inspector_suffix,
        )
        .ok();
    }
    writeln!(output, "=== END MODIFIER CHAIN ===\n").ok();
    output
}

fn describe_capabilities(mask: NodeCapabilities) -> String {
    let mut parts = Vec::new();
    if mask.contains(NodeCapabilities::LAYOUT) {
        parts.push("LAYOUT");
    }
    if mask.contains(NodeCapabilities::DRAW) {
        parts.push("DRAW");
    }
    if mask.contains(NodeCapabilities::POINTER_INPUT) {
        parts.push("POINTER_INPUT");
    }
    if mask.contains(NodeCapabilities::SEMANTICS) {
        parts.push("SEMANTICS");
    }
    if mask.contains(NodeCapabilities::MODIFIER_LOCALS) {
        parts.push("MODIFIER_LOCALS");
    }
    if mask.contains(NodeCapabilities::FOCUS) {
        parts.push("FOCUS");
    }
    if parts.is_empty() {
        "[NONE]".to_string()
    } else {
        format!("[{}]", parts.join("|"))
    }
}

fn describe_inspector(record: &ModifierInspectorRecord) -> String {
    if record.properties.is_empty() {
        record.name.to_string()
    } else {
        let props = record
            .properties
            .iter()
            .map(|prop| format!("{}={}", prop.name, prop.value))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{}({})", record.name, props)
    }
}

type TraceCallback = dyn Fn(&[ModifierChainInspectorNode]) + Send + Sync + 'static;

fn trace_slot() -> &'static Mutex<Option<Arc<TraceCallback>>> {
    static TRACE: OnceLock<Mutex<Option<Arc<TraceCallback>>>> = OnceLock::new();
    TRACE.get_or_init(|| Mutex::new(None))
}

/// RAII guard returned when installing a modifier chain trace subscriber.
pub struct ModifierChainTraceGuard {
    active: bool,
}

impl Drop for ModifierChainTraceGuard {
    fn drop(&mut self) {
        if self.active {
            *trace_slot().lock().unwrap() = None;
        }
    }
}

/// Installs a callback that receives modifier chain snapshots when debugging is enabled.
pub fn install_modifier_chain_trace<F>(callback: F) -> ModifierChainTraceGuard
where
    F: Fn(&[ModifierChainInspectorNode]) + Send + Sync + 'static,
{
    *trace_slot().lock().unwrap() = Some(Arc::new(callback));
    ModifierChainTraceGuard { active: true }
}

pub(crate) fn emit_modifier_chain_trace(nodes: &[ModifierChainInspectorNode]) {
    let maybe = trace_slot().lock().unwrap().clone();
    if let Some(callback) = maybe {
        callback(nodes);
    }
}

#[cfg(test)]
#[path = "tests/debug_tests.rs"]
mod tests;
