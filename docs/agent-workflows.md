# Task-specific agent workflows

Read only the sections required by the current operation. These are project requirements; moving them out of AGENTS.md does not make them optional.

## Code tools

- Prefer RustRover MCP (`mcp__rustrover__*`) for semantic navigation, types/usages, call graphs, analysis and refactoring when ready: `search_symbol`, `get_symbol_info` and `analyze_calls` for code intelligence; `rename_refactoring` for semantic renames; `get_file_problems`, `lint_files` and `run_inspection_kts` for analysis. Pass `projectPath` on every IDE call.
- Use direct file tools for known-file reads, literal searches, documentation and simple edits. IDE `apply_patch`, `create_new_file` and `reformat_file` are optional when useful.
- Run build, test, Git, SSH and other shell commands directly through the shell/exec tool. RustRover's MCP terminal is not required; use it only for a specific IDE-terminal need or an explicit request. Follow [builds and shell](#builds-and-shell) for host and command constraints.
- Open RustRover only when semantic work warrants the setup. If IDE access fails, explain the limitation once and use an appropriate fallback without compulsory reopening or repeated retries. The existing `scripts/dev/ide_search.py text|regex|symbol|file <query> [--in <glob>]... [--context N]` helper queries the same IDE server and is optional when that server is ready and useful.

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
- Suppress a lint with `#[expect]`, which fails once nothing needs it; use `#[allow]` only where the lint fires in some build configurations and not in others.
- Prefer `async`/`await` and Tokio for asynchronous work.
- Document every public API reachable from a published crate root; all other code comments are forbidden (`scripts/dev/strip_private_docs.py <file>...` removes the rest).
- Write integration tests for observable behavior through public entry points; put them in `tests/`. Do not test implementation details such as private storage, type sizes, allocation capacities or pointer reuse. Measure performance with benchmarks and profiles.
- do not write tests in the same file with the implementation; all tests should be under `/test*/` folder, declared with `#[cfg(test)] #[path = "tests/<name>.rs"] mod tests;` (`scripts/dev/move_inline_tests.py <file>...` moves an inline module out)
- Do not hardcode configuration; consider parallelism and SIMD where measured benefits hold, including wasm.
- `#[cfg(feature = "robot-app")]` is forbidden.
- Use plain, direct explanations about current behavior; omit historical or transitional labels and conditional offers to fix known problems.

## Git and CI

- Check `git status` and the current branch before work and before completion; isolate concurrent edits in a worktree.
- Before diagnosing a red test, fetch `origin main` and rebase; confirm claimed fixes are ancestors of `HEAD`.
- After a push, arm a CI watcher before the turn ends (`gh pr checks <n> --watch` under a monitor, or the desktop app's Auto-fix) and act on each result; a wait with no watcher is a stale session.
- Keep related fixes in one PR; finish requested code changes before optional measurements or PR prose.
- A batch merge means merging each ready PR straight into main on its own green CI run, never through a combined branch; a PR is not rebased or run again after another PR of the batch lands, unless the two conflict. Watch main's CI once after the batch and fix any failure there.
- Never use `git reset`; preserve work with a stash when needed.
- Worktrees share stashes: inspect contents, resolve the immutable stash hash, and apply only the intended work.
- Install hooks once per clone with `just hooks`; stage new files before `just precommit` so diff checks include them.
- On GitHub 404 or unexpected permission errors, run `gh auth switch --user samoylenkodmitry`.

## Builds and shell

- Run each `rm` as a standalone command, then verify separately; never chain it with another operation or loop body.
- Never run ad hoc complex shell commands or pipelines; write a reusable script for the job, keep it under `scripts/` when it serves the repository, and run that.
- Reclaim build artifacts only with `just gc` and `just gc-apply`: they sweep cargo target dirs and Gradle build dirs of this repository's worktrees and of the checkouts beside it, primary checkouts protected. Never remove `target/`, `build/`, source or uncommitted work by hand.
- Finish with a worktree through `just worktree-done <path>`: it removes a linked worktree and its local branch only when nothing uncommitted remains, every commit is on a remote and no process uses it, and says why otherwise.
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
- Start bugs with a failing integration regression that exercises observable behavior; for a device UI bug, write the robot e2e test first.
- Verify optimizations with integration tests of observable behavior and measured performance. When using a deliberate correctness mutation, it must change an observable result rather than an internal representation.
- Hold the shared per-device lock for the entire FPS sequence; run ABAB without cooling waits, add BABA only while the result is still unclear, and log temperatures and the battery level before and after every run.
- Measure production FPS on a physical display; Xvfb presentation measures software presentation.

## UI and platform references

- Check draggable and droppable windows with `scripts/dev/drag_window.sh` (launch, windows, oswindows, screenwindows, shotwindow, drag, drag-pane, snap, key, cpu, trace, shot); never drive the pointer or the keyboard with ad hoc `cliclick` calls. `scripts/dev/check_tool_tear.sh <out-dir>` is the whole tear of a tool pane as one check: it reads the traces back and pictures only the torn window's region, never the whole screen.
- Check system dialogs with device UI tests; iOS tests require USB and Settings > Developer > Enable UI Automation.
- Check Jetpack Compose sources at ssh samarch `/media/huge/projects/android/androidx`; verify freshness before treating this mid-2023 checkout as current.

## Release and lessons

- Release: a bare `v*` tag at green main, never a hand bump: [runbook](release.md).
- Route new lessons through [TIME_WASTERS.md](../TIME_WASTERS.md); use short one-liners and remove duplicates or resolved incident notes.
