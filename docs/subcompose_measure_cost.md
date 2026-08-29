# Subcompose measure cost is conserved across boundaries

**Claim.** Per-pass subcompose measure cost is proportional to slot
CONTENT, not to boundary COUNT. Restructuring boundaries redistributes
the cost to whichever layer owns the slot; only making clean slots free
removes it. Measured at ~0.3 ms per body-sized slot per pass on a
Kirin 980 (Huawei Mate 20 X).

Anyone reading a wrapper chain like
`Scaffold > BoxWithConstraints > ... > LazyColumn` and planning to
delete "redundant" subcompose wrappers as a perf win should read the
experiment below first. Deleting a boundary is sometimes right — but for
architecture reasons, not for speed.

## The experiment (2026-08-29, device A/B, pre-registered)

cranscan wrapped its whole body in a `BoxWithConstraints` that received
**fixed constraints** (min == max on both axes: 360×748) — a subcompose
boundary that, by arithmetic, cannot do constraint-dependent
composition. PR #545 added `report_size_state` so the size-reactive
topology could come from observable state instead. The A/B swapped that
BWC for a plain `Box + report_size_state` and measured scroll-driven
layout passes on the device, A/B/A/B, with per-node layout telemetry as
the only per-frame switch.

Prediction stated in advance: root layout p50 drops 1.71 → ~1.3 ms
(the BWC's ~0.4 ms/pass leaves). It missed.

| metric (p50/pass) | A: BWC | B: Box | delta |
|---|---|---|---|
| root total, round 1 | 1.76 ms | 1.53 ms | −0.23 |
| root total, round 2 | 1.69 ms | 1.68 ms | −0.01 |
| wrapper node at the swap site | 0.96 ms | 0.56 ms | −0.39 |
| wrapper cost over its child | 0.37 ms | 0.03 ms | −0.34 |
| scaffold minus its content child | 0.70 ms | 0.97 ms | **+0.27** |

The swap-site win was real and matched the prediction's mechanism. The
root-level win was not, because the scaffold got more expensive by
almost the same amount — in every pass, in both rounds.

## Why

Every subcompose measure pass runs `perform_subcompose` →
`Composer::subcompose_slot`, a compose walk over the retained slot
table, with a fresh `SnapshotStateObserver` and a fresh `Composer`
constructed per pass (`crates/cranpose-ui/src/layout/mod.rs`, observer
construction in `measure_subcompose_node`). During scroll the
boundary's descendants are dirty every frame, so the
cached/retained-activation path never engages and the walk runs every
pass. Its cost scales with what is IN the slot:

- With the BWC present, the body lived in the BWC's slot: the BWC paid
  ~0.35 ms/pass to walk it, and the scaffold's content slot held only
  the BWC node (cheap).
- With the BWC gone, the body lives directly in the scaffold's content
  slot: the scaffold pays ~0.27 ms/pass for the same walk.

The boundary was deleted; the tax moved one level up. Net saving was
one layer of fixed subcompose machinery (~0.1 ms), inside measurement
noise in one of two rounds.

`MutableState::set` was ruled out as the cause: its equality gate
returns before `record_write`, so `report_size_state`'s per-pass set of
an unchanged size dirties nothing.

## Implication for the scheduler work

The fix that actually removes the tax is dirty-id retention: a measure
pass over a subcompose node whose slot composition has no dirty state
must reuse the retained slots without a compose walk. Until that lands,
every subcompose layer on a scroll-dirty path — Scaffold, every
SwipeToDismiss row, every lazy container — pays the walk for its slot
content on every frame, and moving content between layers only changes
who pays.

The fixed-constraints BWC swap itself was still correct to land: a
subcompose boundary with min == max on both axes is architecturally
dead weight, and after retention lands the plain Box costs nothing
while a BWC always re-enters the subcompose machinery. It just is not a
perf win on its own today, and its A/B is the measurement that proved
the conservation law above.
