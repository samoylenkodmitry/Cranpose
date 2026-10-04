# Cranpose core

Core composition and state runtime for Cranpose. The crate supplies the
composer, slot storage, recomposition scopes, node applier and snapshot-based
mutable state for higher-level crates.

Application code normally uses these APIs through `cranpose`. Depend directly
on `cranpose-core` for runtime integration and lower-level framework components.

The slot table stores active groups, payloads and nodes separately. Retained
inactive branches are represented as detached subtrees. The invariants for
these structures are documented in
[`docs/slot_table_invariants.md`](../../docs/slot_table_invariants.md). The
[slot table design note](../../docs/cranpose_slot_table_v2_design.md) describes
the current structure.
