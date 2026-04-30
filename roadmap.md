[x] Harden anchor and payload-anchor lifecycle writes so stale generations cannot silently no-op in release builds.
[x] Make slot writer validation pure by separating pending payload-location flushing from invariant checks.
[x] Remove the disconnected `RecomposeScope` group-anchor mirror and route scope identity through the slot table only.
[x] Propagate immediate detached-node disposal errors instead of discarding cleanup failures.
[ ] Promote detached subtree root-node metadata from debug-only checks to validated runtime invariants.
[ ] Clean up slot-table introspection: rename the fake reader module, fix sparse anchor stats, and report retained payloads in snapshots.
[ ] Remove wrapper/generic slot-table ceremony and debug-only mutation-guard indirection that add no semantic value.
