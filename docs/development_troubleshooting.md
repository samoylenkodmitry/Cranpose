# Development troubleshooting

## Hosts and processes

- samarch-1 provides Linux/X11 and Android SDK `/home/s/develop/sdk`; macm3 provides Apple toolchains and SDK `~/Library/Android/sdk`.
- Add `~/.cargo/bin` to noninteractive SSH PATH; inspect remote checkouts because a copied `.git` may point to another host.
- Linux runners are `samarch-1-cranpose` and `samarch-1-cranpose-2`; macm3 runners are `dmitriis-mac-Cranpose` and `macm3-cranpose-2`.
- `mac-idle-Cranpose` is the user's signing Mac and normally stays offline while in use; do not start it for extra CI capacity.
- Diagnose queued CI from runner `_diag/Runner_*.log` JobDispatcher entries; the jobs API can lag actual execution.
- Host capacity locking requires `flock(1)` and does not operate on macOS; use explicit process ownership there.
- APFS snapshots or shared blocks can retain space after GC; trust `df` and use another build host when space remains low.
- Copied Git indexes can retain invalid fsmonitor state; use `git -c core.fsmonitor=false status` to check a copied checkout.
- Pattern-based process searches can include their caller or zombies; use the repository waiter and verify exact PIDs independently.
- Stopping a wrapper can leave its descendants alive; a cancelled CI status alone does not prove the host lock was released.
- SIGTERM is 15 and SIGKILL is 9; a missing artifact requires separate filesystem evidence.
- Bash pipeline status requires `pipefail` or immediate `PIPESTATUS`; zsh uses `pipestatus` with one-based indexing.
- Bash 3.2 needs `${args[@]+"${args[@]}"}` for optional arrays under `set -u`.
- Bash `read` collapses empty tab-separated fields; use structured data or an explicit sentinel for optional fields.
- Use `/bin/ls` or `rg --files` when shell aliases produce a suspiciously empty directory listing.
- Use `rg` for working-tree searches because untracked files are absent from `git grep` by default.
- First-time cruft-pack collection does not establish that unreachable objects are disposable; inspect stashes and reflogs before repository maintenance.

## Builds and CI

- Pin the complete toolchain from `rust-toolchain.toml`; rustup aliases such as `1.98` and `1.98.0` have separate target installations.
- `cargo ndk` may omit JSON artifact records; verify the executable's freshness, source inventory and native hash explicitly.
- Run browser builds through `just web`; host builds and invented wasm feature combinations do not validate the shipped browser.
- Review lint autofixes across shipped ABIs and rustflags; a cast redundant on 64-bit can be required on 32-bit.
- `just dep-budget` covers all shipped triples; inspect `WORKSPACE_DUPLICATE_DEBT` before proposing an unavailable dependency upgrade.
- `just fmt` includes the isolated consumer workspace; `just doc` excludes colliding demo library names.
- Include source-hygiene tests with focused source changes; per-crate tests alone do not cover repository-wide checks.
- Declare Python gate dependencies in pinned requirements and provision them in the owning `just` recipe; runner-global packages are not reproducible.
- For a completed job in a running workflow, retrieve `gh api repos/OWNER/REPO/actions/jobs/JOB_ID/logs`; `gh run view --log-failed` waits for the workflow.
- Cargo output parsers need `--color never` and ANSI-resistant parsing; test with `CARGO_TERM_COLOR=always`.
- After restoring a mutant or syncing older source timestamps into a diagnostic checkout, touch the changed files before rebuilding; `rsync -a` can otherwise leave Cargo reusing the mutant binary. Verify that the affected crate actually recompiles.
- Attribute large Mach-O unwind sections with a linker map and demangled symbols before changing profiles or features.
- Macro-emitted generic initializers multiply code per expansion; pass values to a shared nongeneric helper where suitable.
- Check sccache non-cacheable reasons; `enable_local_sccache` disables incremental compilation and starts the shared daemon outside runner cleanup tracking.
- Avoid cache actions that prune persistent self-hosted Cargo directories; their registry and toolchain already survive between jobs.
- Missing Apple signing intermediates can coexist with a valid identity listing; import the pinned intermediate into the job's signing keychain.
- Keep runner `.env` files free of comments and send a User-Agent in release-monitor HTTP requests.
- Required checks must report even for documentation-only changes; the tested diff predicate handles root Markdown and both rename endpoints.
- Verify CI against the intended repository, SHA and complete expected check set; empty, partial, skipped or cancelled results are not success.
- A nonresponsive test needs stack, CPU and artifact evidence; pool deadlocks and macOS signature validation require different fixes.

## Releases

- The annotated tag-push workflow writes release versions; read `publish.yml` before releasing instead of bumping them manually.
- Verify the tag still identifies the required main revision when publishing starts; interrupted publication requires checking tag, main and registry state together.
- The isolated demo validates published-consumer behavior; unreleased framework changes require its existing workspace-patch build path.
- The Android Gradle plugin ships inside the Cranpose crate and has no separate plugin publication.
- Squash-merged branch tips need not be ancestors of main; verify the merge commit returned by GitHub and its content.
