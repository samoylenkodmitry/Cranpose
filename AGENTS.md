# Agent Notes for Cranpose

- No unsafe code or `unwrap()`. Keep KISS, DRY and SOLID; duplicated code of ten or more lines needs a shared abstraction.
- Fix root causes and audit every consumer of a wrong value. Leave no partial fixes, deprecated paths or compatibility layers in this pre-alpha repo. Review architecture, correctness and maintainability before completion.
- For implementation, read [Rust/API conventions](docs/agent-workflows.md#rust-and-api-conventions) and the [performance coding guide](docs/performance_coding_guide.md). Test every public function/method; all test bodies belong under `/test*/`, never beside implementation. Document public APIs only.
- Use RustRover MCP for code search, understanding, analysis, refactoring and edits; pass `projectPath`. Prefer IDE tools over Bash/grep/rg for code discovery. Run build/test/git and other shell commands directly through the shell tool; RustRover's MCP terminal is not required. Read [code tools](docs/agent-workflows.md#code-tools).
- Check branch/status at start and completion and after relevant git operations; isolate concurrent work. Never use `git reset`. Preserve unrelated and uncommitted work.

- Keep tool results near 2,000 tokens by default; return relevant excerpts, failures and changed results. Save full logs to files and expand only when needed. Discover only needed tool schemas; reuse unchanged instructions and evidence.
- Batch independent reads/checks. Wait 30–60 seconds for long jobs when supported; report progress between waits. Avoid tight polling, repeated status checks and unchanged log dumps.
- Run targeted checks after meaningful edits and required broad checks before integration; rerun only for changed code, failures or new risks. Documentation-only edits need link/format validation, not product builds.
- Work without subagents unless requested. When requested, use bounded tasks and minimal context; use only small, fast, inexpensive models. After two passes with no measurable progress, revisit the hypothesis or reference and change approach. Do not declare unfinished work complete.

Read the matching workflow before the operation; do not load all references at startup:

- Git changes, red tests, pushes and PRs: [Git and CI](docs/agent-workflows.md#git-and-ci); arm a CI watcher after pushes.
- Builds, shell, remote hosts or artifact cleanup: [builds and shell](docs/agent-workflows.md#builds-and-shell).
- Bug fixes and optimization: [bugs and performance](docs/agent-workflows.md#bugs-and-performance); failing regression first, measured work, exact pictures.
- Device UI, dragging or Compose comparisons: [UI/platform references](docs/agent-workflows.md#ui-and-platform-references).
- Releases or new lessons: [release and lessons](docs/agent-workflows.md#release-and-lessons). Keep notes short and remove resolved/duplicate incidents.
