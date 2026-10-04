# Modifier system

Each Cranpose modifier defines layout, draw work, input, focus, semantics and other
behavior attached to a layout node. The builder stores modifier elements. A
runtime `ModifierNodeChain` reconciles those elements with stateful nodes.
The implementation follows the broad element/node model from Jetpack Compose;
the details below describe Cranpose.

## Modifier values

`Modifier` lives in
[`cranpose-ui/src/modifier/mod.rs`](../crates/cranpose-ui/src/modifier/mod.rs).
The empty value represents an empty chain. A non-empty value stores either one
element or a small vector with inline capacity four. Longer chains share their vector with
`Rc`; `then(self, next)` can extend uniquely owned storage in place. A shared
chain gets a new vector when an append needs to preserve another owner. The
value also caches strict and structural fingerprints, element count, and
whether an element provides composition locals.

Inspector metadata allocates when tests, inspection or modifier diagnostics
request metadata. Ordinary modifier values omit inspector storage. The element
iterator exposes the flat chain. `fold_in` and `fold_out` walk from opposite
ends.

## Elements and nodes

The node traits and chain runtime live in
[`cranpose-foundation/src/modifier.rs`](../crates/cranpose-foundation/src/modifier.rs).
`ModifierNodeElement` is a typed descriptor. Each element creates a node,
updates a compatible node, supplies a hash and equality contract, and declares
node capabilities. Optional element hooks supply an identity key,
inspector properties, and update invalidation behavior.

`ModifierNode` receives attach, detach and reset callbacks. Capability traits
let a node join the layout, draw, pointer-input, semantics or focus pipeline.
`NodeState` stores attachment, parent/child links and capability summaries.
Nodes can delegate to child nodes; traversal helpers visit the delegated tree.

`ModifierNodeChain` owns the node entries for a layout node. On update, the
chain indexes existing entries by element type, key and hash, then matches the
incoming elements against those entries. Compatible nodes receive updates;
new elements create and attach nodes; unused nodes detach. The chain rebuilds
its ordered links and aggregated capabilities after reconciliation. A
thread-local scratch buffer reuses temporary reconciliation storage across
chains.

## Dispatch and invalidation

Each element declares node capabilities. The runtime combines capabilities
through the node chain and delegated children. Capability-aware traversal skips
subtrees outside the requested stage. UI-level modifier slices
collect render and input data for downstream stages; see
[`cranpose-ui/src/modifier/slices.rs`](../crates/cranpose-ui/src/modifier/slices.rs).

An element update can request automatic invalidation for its capabilities or
name a targeted invalidation kind. The context associates each invalidation
with active capabilities, so a value change requests only the work its node
needs. Layout nodes and coordinators consume requests through the
modifier-chain integration in
[`cranpose-ui/src/modifier/chain.rs`](../crates/cranpose-ui/src/modifier/chain.rs).

## Built-in modifiers

The UI crate defines built-in element types and the public `Modifier` methods.
Common groups include:

- Layout: padding, size, fill, weight, offset and intrinsic size.
- Draw: background, border, clip masks, shadow and graphics layers.
- Input: click, drag and drop, scroll, pointer icons and key input.
- Focus and accessibility: focus targets, focus rings and semantics.
- Window roots: modifiers for native window surfaces.

The factories are grouped under
[`cranpose-ui/src/modifier`](../crates/cranpose-ui/src/modifier). The layout
system connects modifier chains to measurement and placement in
[`cranpose-ui/src/layout`](../crates/cranpose-ui/src/layout).

## Current implementation files

| Responsibility | Source |
| --- | --- |
| Modifier value and builder | [`modifier/mod.rs`](../crates/cranpose-ui/src/modifier/mod.rs) |
| Element, node, capability and lifecycle traits | [`modifier.rs`](../crates/cranpose-foundation/src/modifier.rs) |
| Node chain and invalidation bridge | [`modifier/chain.rs`](../crates/cranpose-ui/src/modifier/chain.rs) |
| Render and input slices | [`modifier/slices.rs`](../crates/cranpose-ui/src/modifier/slices.rs) |
| Built-in node implementations | [`modifier_nodes.rs`](../crates/cranpose-ui/src/modifier_nodes.rs) |
| Layout integration | [`layout/mod.rs`](../crates/cranpose-ui/src/layout/mod.rs) |
