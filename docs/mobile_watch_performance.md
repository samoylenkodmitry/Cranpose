# Mobile renderer: measurement index

Decision and next experiment: [60 FPS sheet](mobile_60fps_architecture.md).
Raw evidence root: `/tmp/cranpose-mobile-watch-60fps`. Each app leg records APK,
source provenance, SurfaceFlinger frames, elapsed time, temperatures and validity.
Retain all legs. Historical analysis is available in Git at `01eb24cf`.

| Evidence | Files under the evidence root | Meaning |
| --- | --- | --- |
| Main/shared app comparisons | `{watch,huawei}-main0d-shared-a571{-showcase,}-matrix.json` | Eight alternating legs per workload; presentation thread explicitly enabled |
| Current default watch control | `watch-main0d-respecialize37-auto-matrix.json`, matching `-incomplete.json` | Seven completed legs; eighth lost to TLS disconnect; cannot establish acceptance |
| Shared platform gates | `cranpose-shared-rim-56328905-{gates,ios-gates}.json` | Required recipes pass on audited source |
| Re-specialization correction | `37bd0ce8` tests and native reproduced provenance | Stale material overrides are cleared when inputs change |
| Native corrected glass case | `watch-glass-explicit-669c85a0-proof.json` | Fixed 7 pass; omission guard fails declaration checks; wrong curve fails pixels; restored 7 pass |
| Live producer CPU probe | `{watch,huawei}-streaming-producer-{2-512,1-1024,2-2048}/matrix.json` | Complete production/preparation/publication, without renderer cost |
| Huawei core-policy control | `huawei-streaming-producer-2-2048-app-affinity/{matrix,affinity}.json` | Same framework core selection; streaming still slower |
| Probe provenance | `cranpose-streaming-producer-provenance.json` | Framework/app inventories and native binary hashes |
| Packet timelines | `*-frame-trace-a571-*/` | Diagnostics; presentation calls and GPU execution are distinct intervals |
| Device ownership | `sequence-ownership.jsonl` | Whole-device sequence leases shared with Fable |

## Interpretations that must survive compression

- **Main matters.** Its Megaboss advantage is a regression signal, not permission
  to copy a cache without proving invalidation and image correctness.
- **No cooling exclusions.** Heat is part of the result. Report every ABAB/BABA
  temperature transition; battery temperature alone cannot attribute energy.
- **Streaming remains held.** Watch 2/2,048 improves preparation median and p99;
  every tested Huawei configuration loses median time. Actual renderer ownership,
  upload, draw and completion costs remain unmeasured.
- **Glass fixture correction.** The material builder writes frost × activity.
  The first resting fixture therefore already declared no substrate. Its five
  Adreno pixels did not isolate the added activity predicate. `669c85a0` tests
  positive frost independently of zero activity; exact comparisons stay exact.
- **Transport failures remain failures.** Watch TLS drops occurred at 06:41,
  07:15 and 08:12 UTC. The last cleanup hid the primary error and lost the report.
  The corrected external finalizer saves evidence before cleanup; three tests
  pass after two demonstrated failures. Do not reconstruct an accepted leg from
  partial telemetry.
- **Rejected shortcuts:** retained-owner reuse slowed full scroll despite high
  reuse; queued recording slowed both devices; forced append inlining did not
  help; altered glass coordinates and approximate shader arithmetic changed
  pixels. Reopen only with new causal evidence.
