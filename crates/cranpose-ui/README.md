# Cranpose UI

UI primitives and composables for Cranpose: widgets, layout containers,
modifiers, semantics, text and lazy lists. Most applications import these APIs
through the `cranpose` prelude. Depend directly on this crate for custom
widgets or framework integrations.

Custom layouts use the `Layout` composable with a measure policy. The policy
measures each child against `Constraints` and returns a `MeasureResult` with
the layout size and child positions. See the public `Layout` API docs
for its current callback types and placement helpers.
