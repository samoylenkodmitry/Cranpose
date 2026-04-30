# Slot Table Architecture Roadmap

- [x] Fix invalid-scope suppression so root render replay has one truthful invalidation-drain path.
- [x] Keep value-slot storage identity in release and make cross-table aliasing impossible in all build modes.
- [x] Type or remove the public untyped value-slot write surface while keeping macro parameter and return slots safe.
- [x] Remove or wire production scope-index rebuild metrics.
- [x] Add production-safe local invariants for critical slot and retention identity paths, with exhaustive validation remaining debug/test-only.
- [x] Make retained subtree restore transactional.
- [x] Collapse runtime-state ownership to a single source of truth across `SlotTable`, `SlotsHost`, and `ComposerRuntimeState`.
- [ ] Normalize anchor lifecycle debug stats semantics for group and payload registries.
- [ ] Sync active slot-table design docs with implementation.
