//! Structured, on-demand layout data for embedded IDE inspection.

use cranpose_ui::{LayoutBox, LayoutTree};
use serde::Serialize;

/// JSON schema version returned on the structured inspection channel.
pub const SCHEMA_VERSION: u32 = 2;
/// Host request channel. Payload is an optional unsigned request identifier.
pub const REQUEST_CHANNEL: &str = "cranpose.inspector.v2.request";
/// Application response channel containing a JSON [Snapshot].
pub const SNAPSHOT_CHANNEL: &str = "cranpose.inspector.v2.snapshot";

/// A property reported by a framework modifier or layout node.
#[derive(Debug, Serialize, PartialEq)]
pub struct Property {
    /// Human-readable property name.
    pub name: String,
    /// Current value, formatted by its owner.
    pub value: String,
}

/// An element in a modifier chain, in declaration order.
#[derive(Debug, Serialize, PartialEq)]
pub struct ModifierInfo {
    /// Modifier name.
    pub name: String,
    /// Properties published by the modifier.
    pub properties: Vec<Property>,
}

/// A composable definition that contributed to a layout node.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    /// Function name.
    pub name: String,
    /// Compiler-reported source file.
    pub file: String,
    /// One-based declaration line.
    pub line: u32,
    /// Compiler-reported package directory.
    pub manifest_dir: String,
}

/// A layout node in preorder, with surface-local logical bounds.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    /// Identity includes the allocation generation to distinguish recycled nodes.
    pub id: String,
    /// Parent identity, absent for the root.
    pub parent: Option<String>,
    /// Layout node classification.
    pub kind: String,
    /// Text drawn by the node, if any.
    pub text: String,
    /// Left edge in logical pixels.
    pub x: f32,
    /// Top edge in logical pixels.
    pub y: f32,
    /// Width in logical pixels.
    pub width: f32,
    /// Height in logical pixels.
    pub height: f32,
    /// Modifier chain in declaration order.
    pub modifiers: Vec<ModifierInfo>,
    /// Composable origins, outermost first; present in preview builds.
    pub sources: Vec<Source>,
}

/// A complete response to one host request, never emitted in the idle frame loop.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Protocol schema version.
    pub schema: u32,
    /// Host request identity, allowing stale responses to be discarded.
    pub request_id: u64,
    /// Capture duration in microseconds.
    pub capture_micros: u64,
    /// Whether the node budget cut off part of a large tree.
    pub truncated: bool,
    /// Primary-surface nodes in preorder.
    pub nodes: Vec<Node>,
}

/// Captures the already-laid-out primary surface without rendering another scene.
///
/// `node_budget` bounds work for unusually large trees. Children whose parents
/// were truncated are omitted as well.
pub fn snapshot(tree: Option<&LayoutTree>, request_id: u64, node_budget: usize) -> Snapshot {
    let started = std::time::Instant::now();
    let mut result = Snapshot {
        schema: SCHEMA_VERSION,
        request_id,
        capture_micros: 0,
        truncated: false,
        nodes: Vec::new(),
    };
    let mut stack = tree
        .map(|tree| vec![(tree.root(), None)])
        .unwrap_or_default();
    while let Some((layout, parent)) = stack.pop() {
        if result.nodes.len() == node_budget {
            result.truncated = true;
            break;
        }
        let node = capture(layout, parent);
        for child in layout.children.iter().rev() {
            stack.push((child, Some(node.id.clone())));
        }
        result.nodes.push(node);
    }
    result.capture_micros = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
    result
}

fn capture(layout: &LayoutBox, parent: Option<String>) -> Node {
    let text = layout
        .node_data
        .modifier_slices
        .text_content()
        .unwrap_or_default()
        .to_owned();
    let kind = if text.is_empty() {
        format!("{:?}", layout.node_data.kind)
    } else {
        "Text".to_owned()
    };
    #[cfg(not(feature = "preview"))]
    let sources = Vec::new();
    #[cfg(feature = "preview")]
    let (kind, sources) = {
        let mut kind = kind;
        let sources: Vec<Source> = layout
            .node_data
            .source_trace
            .iter()
            .map(|source| Source {
                name: source.name.to_owned(),
                file: source.file.to_owned(),
                line: source.line,
                manifest_dir: source.manifest_dir.to_owned(),
            })
            .collect();
        if text.is_empty()
            && let Some(source) = sources.iter().rev().find(|source| source.name != "Layout")
        {
            kind = source.name.clone();
        }
        (kind, sources)
    };
    let modifiers = layout
        .node_data
        .modifier
        .collect_inspector_records()
        .into_iter()
        .map(|entry| ModifierInfo {
            name: entry.name.to_owned(),
            properties: entry
                .properties
                .into_iter()
                .map(|property| Property {
                    name: property.name.to_owned(),
                    value: property.value,
                })
                .collect(),
        })
        .collect();
    Node {
        id: format!("{}:{}", layout.node_id, layout.node_generation),
        parent,
        kind,
        text,
        x: layout.rect.x,
        y: layout.rect.y,
        width: layout.rect.width,
        height: layout.rect.height,
        modifiers,
        sources,
    }
}

#[cfg(test)]
#[path = "tests/inspection_tests.rs"]
mod tests;
