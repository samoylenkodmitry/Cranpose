---
tags: [cranpose-rules]
max_turns: 30
timeout_seconds: 900
allowed_tools: [Read, Glob, Grep, Skill, Write, Edit, Bash]
---

Write a small Rust library crate in the current directory (`Cargo.toml`, `src/lib.rs`) for a UI toolkit's drag tracking. The platform reports pointers by a `u64` id; the layout tree names nodes by a `usize` index. A `DragTracker` remembers, for each pointer that is down, the node it went down on and where it started. A drag can be locked to the horizontal axis, locked to the vertical axis, or free. When the tracker hands a move to the nodes under the pointer, a caller chooses either to stop at the first node that consumes it or to hand it to every node. Give it `press`, `move_to`, `release` and `deliver` functions, and tests. Run `cargo test` before you finish.
