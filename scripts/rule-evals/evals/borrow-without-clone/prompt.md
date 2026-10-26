---
tags: [cranpose-rules]
max_turns: 30
timeout_seconds: 900
allowed_tools: [Read, Glob, Grep, Skill, Write, Edit, Bash]
---

`src/lib.rs` in the current directory is a frame renderer from our UI toolkit. `render_frame` clones the device `Arc` and the pending list on every frame, only to get past the borrow checker. Remove those clones without changing the public API or the behavior; the test in `tests/` must still pass. Run `cargo test` before you finish.

Input files: Cargo.toml, src/lib.rs, tests/frames.rs
