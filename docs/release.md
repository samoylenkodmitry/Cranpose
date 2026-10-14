# Release Cranpose

Create a `vX.Y.Z` tag at the green `main` head in the GitHub web UI. The
[publish workflow](../.github/workflows/publish.yml) owns version changes,
crate publication and the isolated consumer update.

The [release acceptance ledger](release_readiness.md) lists the 0.9 scope and
1.0 criteria. Check the documentation home, demo navigation and external
consumers against the candidate.

## Versioning

Every crate in `publish-order.txt` takes the workspace version, so one release
moves all of them. The crates depend on each other with caret requirements, so
the rule below holds for every published crate.

- A patch release (0.9.43 to 0.9.44) keeps the API of the release before it. An
  app that built against 0.9.43 builds against 0.9.44 unchanged.
- A change that breaks callers goes into the next minor release (0.9 to 0.10).
  Cargo does not move a `0.9` requirement to 0.10 on its own.

`sync_versions` enforces the rule with
[`semver_gate.sh`](../scripts/ci/semver_gate.sh): it runs
`cargo semver-checks` for each published crate against the previous release
and stops the release before anything is published. To ship a breaking change,
delete the refused tag and tag the same `main` head with `v<major>.<minor + 1>.0`.
The gate compares with every feature on; a previous release that does not build
that way is compared with its default features.

The crates declare `rust-version` from the workspace: the minimum supported
Rust version. It is the oldest toolchain CI builds with, the ESP32 firmware's
Xtensa toolchain, which follows the `rust-toolchain.toml` pin with a delay.
[`msrv_toolchain_test.sh`](../scripts/ci/msrv_toolchain_test.sh) keeps the
values in step, and the nightly `just msrv-check` builds the whole workspace
with that version. When the minimum rises, an edition 2024 app on an older
toolchain resolves to the last release whose `rust-version` that toolchain
meets.

### Crates apps depend on

| Crates | Use |
| --- | --- |
| `cranpose` | The facade: app launcher, platform shells, renderers and the prelude. |
| `cranpose-core`, `cranpose-ui`, `cranpose-ui-graphics`, `cranpose-ui-layout`, `cranpose-foundation`, `cranpose-animation` | Composition, widgets, modifiers, graphics types, custom layouts, input and animation. |
| `cranpose-services`, `cranpose-capabilities`, `cranpose-navigation`, `cranpose-liquid`, `cranpose-fonts` | Platform services, permissions, navigation, the Liquid design system and fonts. |
| `cranpose-coroflow`, `coroflow` | Coroutines and flows. |
| `cranpose-audio`, `cranpose-media` | Audio and media playback. |
| `cranpose-native` | Cranpose inside Android and UIKit apps, and native views inside Cranpose. |
| `cranpose-testing` | Tests: the robot, assertions and the accessibility audit. |

The `cranpose` features an app chooses:

- Platforms: `desktop` (`desktop-x11`, `desktop-wayland`), `android`, `ios`,
  `web`, `watchos`, `framebuffer` and `embed`.
- Renderers: `renderer-wgpu`, `renderer-wgpu-gles` and `renderer-pixels`.
- Text: `embedded-default-font` (default), `installed-fonts`, `text-shaping`,
  `localization`, `localization-formatting`, `localized-arabic`,
  `localized-devanagari`, `localized-cjk` and `southeast-asian-line-breaks`.
- Services: `audio`, `audio-desktop`, `camera-desktop`, `media`, `media-wav`,
  `image-loading`, `webview`, `storekit`, `playbilling`, `heart-rate` and
  `wearable`.
- Tools: `robot`, `preview`, `hot-reload` and `release-log-info`.

The other crates are internal: `cranpose-macros` (reached through `cranpose`),
`cranpose-assets`, `cranpose-runtime-std`, `cranpose-app-shell`,
`cranpose-localization`, `cranpose-storekit`, `cranpose-gl-templates`, the
`cranpose-render-*` and `cranpose-platform-*` crates and the font data crates.
So are the features that other features turn on (`desktop-shell`,
`native-windows`, `media-backend`) and `cranpose-core`'s `internal`,
`inspection` and `test-helpers`. Their API follows the rule above as well,
because the public crates depend on them. An app that needs one of them is
missing a public API; report that as a bug.

## Workflow

1. `sync_versions` validates the tag shape and the `main` head. The job runs
   `cargo xtask bump-release-version` and the semver gate, commits
   `release: vX.Y.Z [skip ci]`, pushes the metadata commit and moves the tag to
   the new commit.
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
