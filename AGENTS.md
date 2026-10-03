# Agent Notes for Cranpose

- No unsafe code or `unwrap()`. Keep KISS, DRY and SOLID; duplicated code of ten or more lines needs a shared abstraction.
- Fix root causes and audit every consumer of a wrong value. Leave no partial fixes, deprecated paths or compatibility layers in this pre-alpha repo. Review architecture, correctness and maintainability before completion.
- Always make performance optimizations. Never write absurd, wasteful code, and remove any you spot in the codebase as soon as you spot it.
- If you spot a clone, copy or allocation made just to satisfy the borrow checker, immediately rearchitect so no allocation is needed. A big refactoring is justified.
- For implementation, read [Rust/API conventions](docs/agent-workflows.md#rust-and-api-conventions) and the [performance coding guide](docs/performance_coding_guide.md). Document public APIs only.
- Tests: write only e2e and integration tests through public entry points, plus tests for corner cases that can break behavior. No unit tests of implementation details (private state, internal structure, call order): they test nothing and duplicate the code. Test bodies go under `/test*/`, never beside implementation.
- Prefer RustRover MCP for semantic navigation, types/usages, call graphs and refactoring when ready; pass `projectPath`. Use direct tools for known-file reads, literal searches, documentation and simple edits, and shell tools for builds, tests, Git and SSH. Open the IDE only when semantic work warrants the setup; if access fails, explain once and use an appropriate fallback. Respect project host and execution limits. Read [code tools](docs/agent-workflows.md#code-tools).
- Check branch/status at start and completion and after relevant git operations; isolate concurrent work. Never use `git reset`. Preserve unrelated and uncommitted work.
- When done with a worktree (any repository's), push its work and run `just worktree-done <path>`, from another repository `just -f <Cranpose checkout>/justfile worktree-done <path>`; it removes the worktree and its branch or says what would be lost. Reclaim disk with `just gc-apply`. Never `rm` a worktree, `target/` or `build/`.

- Keep tool results near 2,000 tokens by default; return relevant excerpts, failures and changed results. Save full logs to files and expand only when needed. Discover only needed tool schemas; reuse unchanged instructions and evidence.
- Batch independent reads/checks. Wait 30–60 seconds for long jobs when supported; report progress between waits. Avoid tight polling, repeated status checks and unchanged log dumps.
- Run targeted checks after meaningful edits and required broad checks before integration; rerun only for changed code, failures or new risks. Documentation-only edits need link/format validation, not product builds.
- Work without subagents unless requested. When requested, use bounded tasks and minimal context; use only small, fast, inexpensive models. After two passes with no measurable progress, revisit the hypothesis or reference and change approach. Do not declare unfinished work complete.

Performance rules:

- Benchmark on the slowest shipped device against the last release before merging. Desktop numbers do not count.
- When moving work between CPU, GPU or threads, measure the unit that bounds that device.
- Gate every new shader or per-frame feature so code that does not use it compiles or skips it out. Compare malioc registers and cycles per variant.
- In shaders, test pipeline constants with `&`, `|` or `if (CONST)`, never `&&` or `||`. Call large functions from one site.
- No extra pass, offscreen copy, isolation or synchronous compile without a per-case proof that it is needed. A missing opt-in must not add cost.
- No sleep or blocking wait in the frame loop.
- Every pacing or heuristic branch must be reachable on a real device and covered by a test of that state.
- Schedulers choose a job's work when the job starts, from current demand. Bound the queues and drop stale jobs.
- Key each cache artifact separately (plans vs compiled binaries). Persist entries created after startup.
- Recycle per-frame storage on the thread that produced it. Growth across frames is a leak.
- Debug order: diff against the release, offline proxies (malioc, spirv-opt, frame replay), then one ABAB device run on a frozen commit.

Read the matching workflow before the operation; do not load all references at startup:

- Git changes, red tests, pushes and PRs: [Git and CI](docs/agent-workflows.md#git-and-ci); arm a CI watcher after pushes.
- Builds, shell, remote hosts or artifact cleanup: [builds and shell](docs/agent-workflows.md#builds-and-shell).
- Bug fixes and optimization: [bugs and performance](docs/agent-workflows.md#bugs-and-performance); failing regression first, measured work, exact pictures.
- Device UI, dragging or Compose comparisons: [UI/platform references](docs/agent-workflows.md#ui-and-platform-references).
- Releases or new lessons: [release and lessons](docs/agent-workflows.md#release-and-lessons). Keep notes short and remove resolved/duplicate incidents.
