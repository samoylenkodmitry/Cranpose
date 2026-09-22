# Task-specific agent workflows

Read only the sections required by the current operation. These are project requirements; moving them out of AGENTS.md does not make them optional.

## Code tools

- Use RustRover MCP (`mcp__rustrover__*`) for code search, understanding, analysis, refactoring and edits: `search_symbol`, `get_symbol_info` and `analyze_calls` for declarations, usages and call graphs; `rename_refactoring` for renames; `apply_patch` and `create_new_file` for edits; `get_file_problems`, `lint_files` and `run_inspection_kts` for analysis; `reformat_file` for formatting. Pass `projectPath` on every call. Use IDE `search_text` and `search_regex` only for strings and comments. Do not replace code intelligence with Bash/grep/rg scans, or code edits with sed, ad hoc scripts or hand edits.
- Run build, test, Git, SSH and other shell commands directly through the shell/exec tool. RustRover's MCP terminal is not required; use it only for a specific IDE-terminal need or an explicit request. This does not permit shell-based code discovery when IDE tools can answer the question. Follow [builds and shell](#builds-and-shell) for host and command constraints.
- If direct IDE tools are unavailable, open the tree in RustRover first (`open -a RustRover <path>`) and retry; explain any remaining limitation before a fallback. The existing `scripts/dev/ide_search.py text|regex|symbol|file <query> [--in <glob>]... [--context N]` helper queries the same IDE server and may serve as a fallback, not the default when direct MCP search works.

## Rust and API conventions

- No unsafe code.
- `unwrap()` is forbidden
- Use KISS, DRY and SOLID; duplicated code of ten or more lines needs a shared abstraction.
- Fix root causes completely; do not leave partial changes, deprecated paths or compatibility layers in this pre-alpha repository.
- A wrong value fixed at one consumer is still wrong at the others; audit every consumer of that value before calling the bug fixed.
- Review architecture, correctness and maintainability before completion; fix supported problems without inventing new ones.
- Use `cargo add` for dependencies and `cargo upgrade` for upgrades.
- Use `anyhow` in applications and `thiserror` in libraries.
- Use specific `Result<T, E>` errors for failure and `Option<T>` for absence.
- Use idiomatic Rust names; composable functions use CamelCase.
- Prefer `async`/`await` and Tokio for asynchronous work.
- Document every public API reachable from a published crate root; all other code comments are forbidden (`scripts/dev/strip_private_docs.py <file>...` removes the rest).
- Write unit tests for all public functions and methods; put integration tests in `tests/`.
- do not write tests in the same file with the implementation; all tests should be under `/test*/` folder, declared with `#[cfg(test)] #[path = "tests/<name>.rs"] mod tests;` (`scripts/dev/move_inline_tests.py <file>...` moves an inline module out)
- Do not hardcode configuration; consider parallelism and SIMD where measured benefits hold, including wasm.
- `#[cfg(feature = "robot-app")]` is forbidden.
- Use plain, direct explanations; omit historical labels, "migration", and conditional offers to fix known problems.

## Git and CI

- Check `git status` and the current branch before work and before completion; isolate concurrent edits in a worktree.
- Before diagnosing a red test, fetch `origin main` and rebase; confirm claimed fixes are ancestors of `HEAD`.
- After a push, arm a CI watcher before the turn ends (`gh pr checks <n> --watch` under a monitor, or the desktop app's Auto-fix) and act on each result; a wait with no watcher is a stale session.
- Keep related fixes in one PR; finish requested code changes before optional measurements or PR prose.
- Never use `git reset`; preserve work with a stash when needed.
- Worktrees share stashes: inspect contents, resolve the immutable stash hash, and apply only the intended work.
- Install hooks once per clone with `just hooks`; stage new files before `just precommit` so diff checks include them.
- On GitHub 404 or unexpected permission errors, run `gh auth switch --user samoylenkodmitry`.

## Builds and shell

- Run each `rm` as a standalone command, then verify separately; never chain it with another operation or loop body.
- Never run ad hoc complex shell commands or pipelines; write a reusable script for the job, keep it under `scripts/` when it serves the repository, and run that.
- Reclaim build artifacts only with `just gc` and `just gc-apply`; never remove `target/`, `build/`, source or uncommitted work by hand.
- Use the exact CI recipes and shipped features; change checks in `justfile`, never inline in workflows.
- `just web` always uses release mode; `just android` assembles the Android demo release; root `perf*.sh` scripts run performance checks.
- Prefer SSH builds on `samarch-1` or `macm3`; see [host details](development_troubleshooting.md).
- Both SSH hosts use zsh: upload a script and run it with Bash, or quote `bash -lc` without premature variable expansion.
- On samarch-1, use `scripts/ci/with_host_lock.sh --shared` for builds and `--exclusive` for measurements; acquire the lock instead of polling load.
- Invoke `run_robot_test.sh` bare: it takes the host lock itself, and an outer lock self-deadlocks.
- Preserve command failures and stderr; a pipeline's final command or an absent error message does not prove success.

## Bugs and performance

- Follow the [performance coding guide](performance_coding_guide.md); reduce measured work and preserve exact pictures on shipped targets.
- For nontrivial bugs: explore, record evidence, rank causes, compare architecture options, implement, verify and iterate.
- Start bugs with a failing regression test; for a device UI bug, write the robot e2e test first.
- Prove every optimization's correctness test fails when the optimization is deliberately broken; correctness takes priority over speed.
- Hold the shared per-device lock for the entire FPS sequence; run ABAB then BABA without cooling waits and log temperatures before and after every run.
- Measure production FPS on a physical display; Xvfb presentation measures software presentation.

## UI and platform references

- Check draggable and droppable windows with `scripts/dev/drag_window.sh` (launch, windows, oswindows, screenwindows, shotwindow, drag, drag-pane, snap, key, cpu, trace, shot); never drive the pointer or the keyboard with ad hoc `cliclick` calls. `scripts/dev/check_tool_tear.sh <out-dir>` is the whole tear of a tool pane as one check: it reads the traces back and pictures only the torn window's region, never the whole screen.
- Check system dialogs with device UI tests; iOS tests require USB and Settings > Developer > Enable UI Automation.
- Check Jetpack Compose sources at ssh samarch `/media/huge/projects/android/androidx`; verify freshness before treating this mid-2023 checkout as current.

## Release and lessons

- Release: a bare `v*` tag at green main, never a hand bump: [runbook](release.md).
- Route new lessons through [TIME_WASTERS.md](../TIME_WASTERS.md); use short one-liners and remove duplicates or resolved incident notes.
