# Releasing Cranpose

A release is one action: create a `vX.Y.Z` tag on green `main` in the GitHub web
UI. Nothing before it, nothing after it.

## What the tag sets off

`.github/workflows/publish.yml` takes it from there:

1. `sync_versions` runs `cargo xtask bump-release-version "$tag"`, commits
   `release: ${tag} [skip ci]`, pushes it to `main`, and moves the tag onto that
   commit, so the tag's tree matches what gets published.
2. The crates publish to crates.io in dependency order.
3. `bump_isolated_demo` points `apps/isolated-demo` at the version that now
   exists on crates.io.

The workspace version on `main` therefore moves *during* the release, not
before it.

## Do not hand-push the bump

The tempting reconstruction of the flow is "bump the version, commit
`release: v0.1.N`, push, then tag". It is wrong, and it reddens `main`.

`[skip ci]` on the workflow's own commit is load-bearing. Between step 1 and
step 3 the workspace is at `0.1.N` while `apps/isolated-demo` still pins
`0.1.N-1`, and that tree is not meant to be built. A hand-pushed commit carries
no `[skip ci]`, so CI runs on exactly that tree. Cutting v0.1.117 this way
reddened three steps of `fmt + tests + clippy (mac)` at once:

- `Check Cranpose package versions`
- `Test the quality gates' diff-scoping logic`
- `Run workspace tests` (187 passed, 1 failed: xtask's
  `check_versions_passes_against_the_real_workspace`)

`.githooks/commit-msg` refuses a `release:` commit message without `[skip ci]`
for this reason. Install the hooks once per clone with `just hooks`.

## Do not hand-fix the isolated demo pin

`apps/isolated-demo` resolves the *published* crates, so pinning it to `0.1.N`
before `0.1.N` exists on crates.io leaves a lockfile cargo cannot resolve. It is
the canary that proves a release is consumable from outside the workspace, and
only `bump_isolated_demo`, after the publish, can move it.

## If a release goes wrong

Read `publish.yml` before fixing anything by hand: most release-time reds are a
step the workflow already owns. `versions` failing on `main` right after a tag
usually means the publish did not reach `bump_isolated_demo`, and the fix is to
let that job finish or re-run it, not to edit the pin.
