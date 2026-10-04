# Release Cranpose

Create a `vX.Y.Z` tag at the green `main` head in the GitHub web UI. The
[publish workflow](../.github/workflows/publish.yml) owns version changes,
crate publication and the isolated consumer update.

The [release acceptance ledger](release_readiness.md) lists the 0.9 scope and
1.0 criteria. Check the documentation home, demo navigation and external
consumers against the candidate.

## Workflow

1. `sync_versions` validates the tag shape and the `main` head. The job runs
   `cargo xtask bump-release-version`, commits `release: vX.Y.Z [skip ci]`,
   pushes the metadata commit and moves the tag to the new commit.
2. `publish` checks out the tag, publishes the Cranpose WGPU forks, then
   publishes framework crates in dependency order.
3. `bump_isolated_demo` updates the isolated consumer after registry publication.
4. The consumer checks build the published desktop, web and Android packages.

The workspace version and isolated consumer pin differ between the metadata
commit and the consumer update. The workflow's `[skip ci]` marker covers this
intermediate state. The commit hook enforces the release-message marker;
`just hooks` installs the repository hooks.

## Recovery

Inspect `publish.yml`, the failed job, the tag, `main` and registry state before
an intervention. A version-check failure after a tag can mean the workflow
still awaits `bump_isolated_demo`. Complete or retry the responsible job.

A manual workflow dispatch requires the release metadata on `main`. The normal
tag-push path writes the metadata itself. Keep each release on one workflow
path and verify the tag tree against the published version.
