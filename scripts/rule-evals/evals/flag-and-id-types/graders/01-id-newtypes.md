---
type: llm
focus: trace
weight: 1
---

Pointer ids and node ids are two distinct newtypes (for example `struct PointerId(u64)` and `struct NodeId(usize)`), and the tracker's functions take those types, not bare u64 or usize
