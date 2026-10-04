# Subcompose boundary cost

This report records a device experiment from 2026-08-29 on a Huawei Mate 20 X
(Kirin 980). The result applies to the source revision in the experiment.
Current slot reuse behavior is described in [slot-table invariants](slot_table_invariants.md)
and [lazy lists](lazy_list_doc.md).

## Experiment

The CranScan body has fixed constraints of 360 × 748. Arm A uses
`BoxWithConstraints`; arm B uses `Box` with `report_size_state`. The route uses
A/B/A/B order and per-node layout telemetry. The hypothesis predicts a root
layout p50 decrease from 1.71 ms to about 1.3 ms.

| Metric, p50 per pass | A: BoxWithConstraints | B: Box | Delta |
| --- | ---: | ---: | ---: |
| Root total, round 1 | 1.76 ms | 1.53 ms | −0.23 ms |
| Root total, round 2 | 1.69 ms | 1.68 ms | −0.01 ms |
| Wrapper node | 0.96 ms | 0.56 ms | −0.39 ms |
| Wrapper cost above child | 0.37 ms | 0.03 ms | −0.34 ms |
| Scaffold cost above content child | 0.70 ms | 0.97 ms | +0.27 ms |

Arm B reduces wrapper cost and increases scaffold cost. The body moves from
the wrapper's subcompose slot into the scaffold's content slot. The slot walk
therefore moves to the parent. The second root measurement falls within noise.

## Use the result

Measure the whole layout path after a boundary change. A local cost decrease
can move work into a parent. Use retained-slot reuse to reduce repeated
composition work, and verify behavior against the current implementation.
`MutableState::set` checks equality before a write, so an unchanged size value
preserves the state's write status.
