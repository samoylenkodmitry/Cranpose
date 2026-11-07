# Agent Notes for Cranpose

- Handle all platforms, always. A change is not done while any platform Cranpose ships lacks it: Android, iOS, watchOS, macOS, Windows, Linux and the web (WebGPU and WebGL).
- Never write absurd, wasteful code, and remove any you spot in the codebase as soon as you spot it.
- Tests: write only e2e and integration tests through public entry points, plus tests for corner cases that can break behavior. No unit tests of implementation details (private state or storage, internal structure, call order, type sizes, allocation capacities, pointer reuse): they test nothing and duplicate the code. Put integration tests in `tests/`. Test bodies go under `/test*/`, never beside implementation: declare them with `#[cfg(test)] #[path = "tests/<name>.rs"] mod tests;` (`scripts/dev/move_inline_tests.py <file>...` moves an inline module out). Measure performance with benchmarks and profiles.
- Check branch/status at start and completion and after relevant git operations; isolate concurrent work. Never use `git reset`; preserve work with a stash when needed. Preserve unrelated and uncommitted work.
- Merge a batch of ready PRs straight into main, one after another, each on its own green CI run: no combined branch, no rebase and no new run after another PR of the batch lands. Rebase only for a conflict. Then check main's CI once for the whole batch and fix what it finds. A PR that changes only README files merges without waiting for CI.
- When done with a worktree (any repository's), push its work and run `just worktree-done <path>`, from another repository `just -f <Cranpose checkout>/justfile worktree-done <path>`; it removes the worktree and its branch or says what would be lost. It removes a linked worktree only when nothing uncommitted remains, every commit is on a remote and no process uses it. Reclaim disk with `just gc-apply`. Never `rm` a worktree, `target/` or `build/`.
- Keep tool results near 2,000 tokens by default; return relevant excerpts, failures and changed results. Save full logs to files and expand only when needed. Discover only needed tool schemas; reuse unchanged instructions and evidence.
- Batch independent reads/checks. Wait 30–60 seconds for long jobs when supported; report progress between waits. Avoid tight polling, repeated status checks and unchanged log dumps.
- Run targeted checks after meaningful edits and required broad checks before integration; rerun only for changed code, failures or new risks. Documentation-only edits need link/format validation, not product builds.
- Work without subagents unless requested. When requested, use bounded tasks and minimal context; use only small, fast, inexpensive models. After two passes with no measurable progress, revisit the hypothesis or reference and change approach. Do not declare unfinished work complete.

Code rules (lints and gates check the rest; follow their errors):

- KISS, DRY and SOLID; ten or more duplicated lines need a shared abstraction. Fix root causes and audit every consumer of a wrong value. Leave no partial fixes, deprecated paths or compatibility layers in this pre-alpha repo. Review architecture, correctness and maintainability before completion.
- One concept per module, named after its main item; a parent module holds only `mod` and `pub use`. Name the owning module before adding code. Split a file over its budget first, in its own commit.
- Types carry the rules: newtypes for ids and units, an enum for an argument that selects behavior, a field valid in one state inside that enum variant, `Option` for absence and never a sentinel. Give a function a new type before it reaches 8 arguments.
- Never clone, copy or allocate to satisfy the borrow checker; rearchitect, even with a big refactoring. Fix a borrow conflict in this order: a shorter borrow or a named `let`; `mem::take`; split the struct into parts and move the method onto the part; indices into an owning `Vec`.
- Several maps with one key become one map of a struct.
- Items are private, then `pub(crate)`; `pub` is the crate API only, and a public signature holds no `Rc`, `RefCell` or `Box`. A `RefCell` borrow or a lock lasts a few lines and never spans a callback.
- Names: use idiomatic Rust names (composable functions use CamelCase); no `get_` prefix; `as_` is free, `to_` costs, `into_` consumes; no `Manager`, `Helper` or `Util` types; no module name repeated in a type name. A trait exists for generic or `dyn` use, never to group methods; a function with a clear receiver is a method.
- Public API matches Jetpack Compose in names and behavior, so Google's Compose documentation serves developers who move to Cranpose. Modifiers use snake case (`Modifier.focusRestorer()` is `Modifier::focus_restorer()`); composables, types and enum variants keep Compose's names. Where Compose uses a bit mask or a sentinel, an enum of the same name takes its place.
- `let _guard = x` keeps a guard alive; `let _ = x` drops it at once.
- Doc comments go on public API only: document every public API reachable from a published crate root. `scripts/dev/strip_private_docs.py <file>...` removes doc comments from everything else; `--check` counts them. A `//` comment states a reason the code cannot show, never history.
- A refactor keeps behavior and output bytes identical; untested behavior gets a test first.
- `thiserror` in libraries, `anyhow` in applications; a specific `Result` error for failure, `Option` for absence.
- Use `cargo add` for dependencies and `cargo upgrade` for upgrades.
- Suppress a lint with `#[expect]`, which fails once nothing needs it; use `#[allow]` only where the lint fires in some build configurations and not in others.
- Prefer `async`/`await` and Tokio for asynchronous work.
- Do not hardcode configuration; consider parallelism and SIMD where measured benefits hold, including wasm.
- `#[cfg(feature = "robot-app")]` is forbidden.
- Use plain, direct explanations about current behavior; omit historical or transitional labels and conditional offers to fix known problems.

Performance rules:

- Design data-oriented; read the hot path and its allocations per iteration before changing it. Smells: a hash table where a flat array by slot would do, a pointer where an index would do (pack the hottest data in its upper bits), an array of structures a pass reads one field of, an array of bools instead of a bitset, a heap list instead of inline capacity or an arena, a cache with no measured win (delete it; redesign the slow path instead).
- Benchmark on the slowest shipped device against the last release before merging. Desktop numbers do not count.
- Amplify every loss you spot: build a bench layout that makes the loss large, then work on it with that layout. Add the layout to the benches and the nightly, so a regression of the same kind shows at once.
- Keep every test, benchmark and device run as short as physically possible. Alternate A and B, read the result after each pair, and stop as soon as it clearly shows the answer: a difference well past the noise, or clearly no difference. Never run long fixed batches.
- Benchmarks measure each launch from its start, warm-up included, over the same span for every subject, and keep the fps of every second of the run. Every graph shows those seconds; no data is dropped or hidden, and frameworks are compared on the same full runs. Every report shows each subject's first start after install (its data and caches cleared) and its second start apart, for every framework, case and metric.
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

## Performance coding guide

Identical pictures; less work per frame; Jetpack Compose API. Apply these rules to framework code, generated code and shaders. Each rule names the evidence that proves it. Use the [mobile acceptance protocol](docs/mobile_60fps_architecture.md).

- Work: remove repeated computation, traversal and representation conversion. Give each prepared result one owner. Evidence: calls/frame, affected frames and saved time. A branch that never runs cannot explain a gain.
- Builds: inspect the consuming workspace's effective profile, features, target and flags. Dependency profiles are ignored. Compare optimization level, LTO and codegen units separately. Evidence: source, lockfile, command and installed-library hashes; matching profiling symbols. Cranpose's profile cannot tune an unchanged external app. [Build configuration][build]
- Allocation: borrow within a phase; reuse owned collections across phases. Avoid temporary `collect`, boxed iterators and duplicate shared wrappers. Reserve from measured lengths; bound retention. Evidence: allocation count, bytes, lifetimes and time. `SmallVec`, arenas and `clone_from` need a measured fit. Preserve destruction and observation leases. [Heap allocation][heap]
- Allocators: attribute allocation cost before comparing supported alternatives. Keep process allocator selection explicit at the executable boundary. Evidence: separate Rust, driver and platform allocations; include RSS, faults and heat. A Rust global allocator does not replace the GPU driver's allocator. No allocator imposed by a reusable library. [Alternative allocators][build]
- Layout and cache: keep hot fields together; separate cold payloads. Prefer dense storage and sequential traversal when access patterns fit. Evidence: bytes touched and target layouts; cache/refill counters where available. No assumed cache-line size, universal copy threshold or automatic benefit from padding. [Cache effects][cache], [type sizes][sizes]
- Copying: pass small values directly; borrow large data when it removes copies. Share immutable data across actual ownership boundaries. Evidence: attributed copy sites and byte counts. A `memcpy` sample without a usable caller does not identify a structure. [Ownership][cheats]
- Bounds: use slices, iterators, exact chunks and necessary range checks before loops. Handle tails and unequal lengths. Evidence: target assembly and unchanged error behavior. No silent `zip` truncation, masked invalid indices, or `debug_assert!` as a release invariant. No `unsafe`. [Bounds checks][bounds]
- Inlining: expose small hot helpers to cross-crate optimization. Keep large generic wrappers thin; outline cold work. Evidence: caller assembly, runtime and code size. Inlining is a non-transitive hint and can worsen code generation. No blanket `inline(always)`. [Inlining][inline], [current reference][codegen]
- Collections: choose algorithms from actual key/cardinality distributions. Use lazy fallbacks. Discard ordering only when it has no meaning. Evidence: complexity at realistic sizes; collision and ordering correctness. Keep the project hashing abstraction and untrusted-input protection. [Collections/state machines][book]
- SIMD and math: batch independent work; inspect auto-vectorization first. Use safe abstractions that support shipped targets. Evidence: ARMv7, AArch64 and wasm results, including remainders. Preserve rounding, NaNs, signed zero, blending and clipping. Never cross-compile with host `target-cpu=native`. [Batching/SIMD][cheats]
- Concurrency: use async for waiting. Parallelize when transfer, synchronization and thermal costs pay. Keep mutation local; avoid per-item shared counters. Evidence: critical path, queues, waits, wakeups and hot throughput. Relaxed atomics still transfer cache lines. More threads and lock-free code are hypotheses. [Cache sharing][cache], [threads/async][cheats]
- I/O and diagnostics: buffer repeated I/O, reuse parsing buffers and keep disabled logging cheap. Preserve validation and errors. Evidence: syscalls and instrumentation overhead. Group counters used in ratios; check running coverage and output freshness. Disable instrumentation for acceptance. [I/O][io]
- Macros and tooling: inspect expansion, monomorphization and build timings when they dominate. Use maintained APIs and existing `just` gates. Evidence: separate build-time gains from runtime gains. PGO needs representative training and a reproducible consuming build. [Compile times][compile], [macros][book]
- GPU: count shaded pixels, samples, transferred bytes and attachment loads/stores. Preserve dependencies and blend order. Evidence: exact captures and device timing. Fewer passes can repeat shading; fewer vertices can increase overdraw. CPU savings cannot resolve a saturated GPU.

Experiment contract: name the cost, remove it diagnostically, confirm the path runs, then implement. Use integration tests to preserve observable behavior and benchmarks to measure performance; do not assert implementation details. Use a focused check before paired device acceptance. Retain failed legs and temperatures. Publish the decision and evidence, not a diary. Run required repository gates on the final change; avoid repeated whole suites per hypothesis.

Sources: all [Performance Book](https://nnethercote.github.io/perf-book/introduction.html) topics, including compilation; the cheat-sheet language, layout, ownership and API design pages; all seven cache examples; both the inlining and the bounds articles. The 265-page 2018 book was surveyed across eleven chapters: compiler, types/collections, memory, lints, profiling, benchmarks, macros, threads and async. Its compiler-plugin, futures and nightly instructions are dated, not current dependency advice. Source speedup percentages are not Cranpose forecasts.

[build]: https://nnethercote.github.io/perf-book/build-configuration.html#alternative-allocators
[heap]: https://nnethercote.github.io/perf-book/heap-allocations.html
[sizes]: https://nnethercote.github.io/perf-book/type-sizes.html
[io]: https://nnethercote.github.io/perf-book/io.html
[compile]: https://nnethercote.github.io/perf-book/compile-times.html
[cheats]: https://cheats.rs/
[cache]: https://igoro.com/archive/gallery-of-processor-cache-effects/
[inline]: https://matklad.github.io/2021/07/09/inline-in-rust.html
[bounds]: https://shnatsel.medium.com/how-to-avoid-bounds-checks-in-rust-without-unsafe-f65e618b4c1e
[codegen]: https://doc.rust-lang.org/reference/attributes/codegen.html#the-inline-attribute
[book]: https://github.com/goldcoders/Rust/blob/master/Eguia%20Moraza%2C%20Iban%20-%20Rust%20high%20performance_%20learn%20to%20skyrocket%20the%20performance%20of%20your%20Rust%20applications-Packt%20Publishing%20%282018%29.pdf

## Workflows

Each section holds the rules for one kind of operation. They are project requirements.

### Code tools

- Prefer RustRover MCP (`mcp__rustrover__*`) for semantic navigation, types/usages, call graphs, analysis and refactoring when ready: `search_symbol`, `get_symbol_info` and `analyze_calls` for code intelligence; `rename_refactoring` for semantic renames; `get_file_problems`, `lint_files` and `run_inspection_kts` for analysis. Pass `projectPath` on every IDE call.
- Use direct file tools for known-file reads, literal searches, documentation and simple edits. IDE `apply_patch`, `create_new_file` and `reformat_file` are optional when useful.
- Run build, test, Git, SSH and other shell commands directly through the shell/exec tool. RustRover's MCP terminal is not required; use it only for a specific IDE-terminal need or an explicit request. Respect project host and execution limits; follow [Builds and shell](#builds-and-shell).
- Open RustRover only when semantic work warrants the setup. If IDE access fails, explain the limitation once and use an appropriate fallback without compulsory reopening or repeated retries. The `scripts/dev/ide_search.py text|regex|symbol|file <query> [--in <glob>]... [--context N]` helper queries the same IDE server and is optional when that server is ready and useful.

### Git and CI

- Before diagnosing a red test, fetch `origin main` and rebase; confirm claimed fixes are ancestors of `HEAD`.
- After a push, arm a CI watcher before the turn ends (`gh pr checks <n> --watch` under a monitor, or the desktop app's Auto-fix) and act on each result; a wait with no watcher is a stale session.
- Keep related fixes in one PR; finish requested code changes before optional measurements or PR prose.
- Before opening a PR, run `/simplify` on the diff, or review it the same way where that command is missing: the gates check shape, and review catches renamed copies and code in the wrong module.
- Cut budgets in cleanup PRs of their own, with no feature work. A PR that lowers a budgeted count lowers its budget in the same commit.
- CI time: pull request checks stay the fastest lane; main's run after a merge stays under 20 minutes and the nightly under an hour. Time a new check locally before choosing its lane.
- Worktrees share stashes: inspect contents, resolve the immutable stash hash, and apply only the intended work.
- Install hooks once per clone with `just hooks`; stage new files before `just precommit` so diff checks include them.
- On GitHub 404 or unexpected permission errors, check `gh auth status`. This Mac's `gh` holds two accounts and other sessions switch the active one. Never run `gh auth switch`, which changes every session. Scope the account to one command: `GH_TOKEN=$(gh auth token --user samoylenkodmitry) gh ...`. For git pushes, scope a credential helper that prints that token.

### Builds and shell

- Run each `rm` as a standalone command, then verify separately; never chain it with another operation or loop body.
- Never run ad hoc complex shell commands or pipelines; write a reusable script for the job, keep it under `scripts/` when it serves the repository, and run that.
- Reclaim build artifacts only with `just gc` and `just gc-apply`: they sweep cargo target dirs and Gradle build dirs of this repository's worktrees and of the checkouts beside it, primary checkouts protected. Never remove `target/`, `build/`, source or uncommitted work by hand.
- A new worktree starts with a copy of the target of the closest worktree, the one whose files differ least ([cargo-seed](https://github.com/samoylenkodmitry/cargo-seed), run by the post-checkout hook). On APFS the copy shares disk blocks, and cargo rebuilds only what differs. `just seed` fills a worktree that has no target.
- Use the exact CI recipes and shipped features; change checks in `justfile`, never inline in workflows.
- `just web` always uses release mode; `just android` assembles the Android demo release; root `perf*.sh` scripts run performance checks.
- Prefer SSH builds on `samarch-1` or `macm3`; follow [Hosts](#hosts). More traps are in [development troubleshooting](docs/development_troubleshooting.md).
- Both SSH hosts use zsh: upload a script and run it with Bash, or quote `bash -lc` without premature variable expansion.
- On samarch-1, use `scripts/ci/with_host_lock.sh --shared` for builds and `--exclusive` for measurements; acquire the lock instead of polling load.
- Invoke `run_robot_test.sh` bare: it takes the host lock itself, and an outer lock self-deadlocks.
- Preserve command failures and stderr; a pipeline's final command or an absent error message does not prove success.

### Hosts

- samarch-1 provides Linux/X11 and Android SDK `/home/s/develop/sdk`; macm3 provides Apple toolchains and SDK `~/Library/Android/sdk`.
- Add `~/.cargo/bin` to noninteractive SSH PATH; inspect remote checkouts because a copied `.git` may point to another host.
- Write `scp` targets as `host:relative/path`: a `~` in a local variable expands to the local home, and the copy lands on a path the other host does not have; assert `git status` on the host shows the files you changed before a run.
- Linux runners are `samarch-1-cranpose` and `samarch-1-cranpose-2`; macm3 runners are `dmitriis-mac-Cranpose` and `macm3-cranpose-2`.
- `mac-idle-Cranpose` is the user's signing Mac: its gatekeeper (`~/actions-runner/gatekeeper.sh`) lets it listen only while the Mac sits idle on power, and it then takes `cranpose-mac` jobs; do not force it online with `run-now.sh` for extra CI capacity.
- `samarch-cranpose` runs on samarch, the user's desktop, as the overflow of `cranpose-linux` jobs (no GPU, no Emscripten): the user timer `cranpose-runner-gate.timer` starts `cranpose-runner.service` once such a job has waited 3 minutes and stops it after 10 idle minutes; jobs run at `Nice=10`, a fifth of the desktop's CPU weight and at most 16 of its 32 threads, as its CPU sits near 90 C at rest.
- Keep CI from push to green under 20 minutes: when a pool's queue grows, add capacity to it from these hosts before waiting on it.
- Diagnose queued CI from runner `_diag/Runner_*.log` JobDispatcher entries; the jobs API can lag actual execution.
- Host capacity locking uses native `flock(1)` when available and Python `fcntl.flock` otherwise, on the same host-wide lock files. Only updated scripts that take these locks participate; coordinate old checkouts and direct commands separately.

### Bugs and performance

- Follow the [performance coding guide](#performance-coding-guide); reduce measured work and preserve exact pictures on shipped targets.
- For nontrivial bugs: explore, record evidence, rank causes, compare architecture options, implement, verify and iterate.
- Start bugs with a failing integration regression that exercises observable behavior; for a device UI bug, write the robot e2e test first.
- Verify optimizations with integration tests of observable behavior and measured performance. When using a deliberate correctness mutation, it must change an observable result rather than an internal representation.
- `just mutants-diff` mutates the lines a branch changed; a mutant no test caught is changed code no test checks.
- Hold the shared per-device lock for the entire FPS sequence; run ABAB without cooling waits, add BABA only while the result is still unclear, and log temperatures and the battery level before and after every run.
- Measure production FPS on a physical display; Xvfb presentation measures software presentation.

### UI and platform references

- Check draggable and droppable windows with `scripts/dev/drag_window.sh` (launch, windows, oswindows, screenwindows, shotwindow, drag, drag-pane, snap, key, cpu, trace, shot); never drive the pointer or the keyboard with ad hoc `cliclick` calls. `scripts/dev/check_tool_tear.sh <out-dir>` is the whole tear of a tool pane as one check: it reads the traces back and pictures only the torn window's region, never the whole screen.
- Check system dialogs with device UI tests; iOS tests require USB and Settings > Developer > Enable UI Automation.
- Check Jetpack Compose sources at ssh samarch `/media/huge/projects/android/androidx`; verify freshness before treating this mid-2023 checkout as current.

### Release and lessons

- Release: a bare `v*` tag at green main, never a hand bump: [runbook](docs/release.md).
- Route new lessons through [TIME_WASTERS.md](TIME_WASTERS.md); use short one-liners and remove duplicates or resolved incident notes.

## Simple speech

Never leave prepositions trailing at the ends of clauses (e.g., use "the version by which..." instead of "the version... by"), and keep modifiers close to the words they modify. Use simple words in their original meaning. no poetic, no jargon. No contrastive sentences. No special symbols. No Trailing Participial Phrases. NO for any of these: Overused Buzzwords, Empty Transition Openers, unnecessary adjectives that try to sell an ordinary fact, list things in triples (e.g., "fast, efficient, and reliable" or "streamline, optimize, and scale")., Not only... but also..., wrap-up summaries that don't add actual data. | **No sales pitch or hype:** Adopt a neutral, engineering-first tone. Never use marketing fluff, exclamation points for enthusiasm, or words designed to "sell" a feature (e.g., *effortless, supercharge, magical, lightning-fast*). State facts and mechanics directly.

Never connect sentences with ", and". If you want to add something to the current sentence, then make it the next sentence, not a part connected with ", and bla bla bla". That is a bad antipattern.

Never use the verb-noun pair, like "a home Jack built" or "the dots Jack connects". That is a perversion. Use the correct order of words: "a home built by Jack", "the dots connected by Jack".

Write every sentence in canonical Subject-Verb-Object order. Put the subject first. Put the verb right after the subject. Put the object after the verb: "The compiler rejects the call." Never put the object, an adverb or a complement before the subject, like "Rejected by the compiler is the call" or "Into the cache goes the entry". Never invert the subject and the verb. Put no long insertion between the subject and the verb.
