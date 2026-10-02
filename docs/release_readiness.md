# Release readiness: 0.9 toward 1.0

Audit date: 2026-10-02. Source baseline: `59ac28c6ba30a265f4cee66502ebc1f3ab12bb39`.
0.9 is the stabilization release line. It does not freeze the public API or
assert that every platform, service and performance target is complete.

## Evidence checked

This audit inspected code, public-behavior tests, CI definitions and GitHub
results. It distinguishes source inspection from an executed check.

| Finding | Evidence | What it establishes |
| --- | --- | --- |
| Rich text is implemented | [AnnotatedString](../crates/cranpose-ui/src/text/annotated_string.rs), [LinkedText](../crates/cranpose-ui/src/widgets/linked_text.rs), [annotated baseline regressions](../crates/cranpose-render/common/tests/annotated_text_baselines.rs), merged [#1045](https://github.com/samoylenkodmitry/Cranpose/pull/1045). | The May text tracker was stale. Styled text, links and corrected baseline geometry exist. |
| Clickable controls provide reader activation and keyboard focus | [Clickable modifier](../crates/cranpose-ui/src/modifier/clickable.rs), shared pressable behavior. | The old claim that clickable has no default semantics is wrong. Its position-taking callback still differs from Compose. |
| Semantics-based robot testing exists | [Robot helpers](../crates/cranpose-testing/src/robot_helpers.rs), [demo accessibility audit](../apps/desktop-demo/tests/accessibility_audit.rs). | Absence of a Compose-style testTag API does not imply absence of semantic testing. |
| Physical-iPhone reader flows have evidence | [VoiceOver validation](accessibility_voiceover_validation.md), [screen identity and native input](accessibility_screen_identity_validation.md), dated 2026-09-22. | Navigation, capture and a specific edit/save/reopen flow were exercised. The earlier simulator-only report is not the latest evidence. |
| Network status has an optimistic fallback | [NetworkMonitor](../crates/cranpose-services/src/network_status.rs) defaults to online/unmetered without a backend. | Applications must not treat this fallback as measured connectivity or an explicit Unsupported result. |
| Continuous checks cover multiple build targets | [Rust](../.github/workflows/rust.yml), [heavy](../.github/workflows/heavy-selfhosted.yml), [nightly](../.github/workflows/nightly.yml). | Builds, tests and robots have distinct coverage; a cross-build does not establish native Windows runtime behavior. |
| Publication validates an outside consumer | [Publish workflow](../.github/workflows/publish.yml) builds the isolated desktop, web and Android app after publishing. | Registry consumption is already checked; it is not a missing release mechanism. |

The latest full Rust and heavy runs inspected before these documentation changes
passed at `1ccd5314d`:
[Rust](https://github.com/samoylenkodmitry/Cranpose/actions/runs/36962892131),
[heavy](https://github.com/samoylenkodmitry/Cranpose/actions/runs/36962892046).
The subsequent v0.1.177 release metadata and isolated-consumer commits are
separate revisions. Their existence is not a claim that these new changes passed.

## Acceptance work before 1.0

| Gate | Acceptance evidence still needed |
| --- | --- |
| Supported API | Explicit stable crates/features, intended breaking changes completed, compatibility checks and a minimum Rust version policy. Internal changes may continue while supported 1.x contracts remain compatible. |
| Platform runtime | Named supported OS/browser versions and candidate runs for startup, input, focus, scrolling, lifecycle and recovery. Add continuous Windows runtime coverage or narrow the stated support tier. |
| Accessibility and text | Expand the existing physical-device results to the reader/browser and text-input matrix. Do not repeat the stale claim that there has been no physical-iPhone validation. |
| Performance | Current candidate measurements and agreed budgets for presentation tails, latency, memory, allocations, startup and idle work on named devices. |
| Consumers | Candidate-package validation in Showcase, Cranamp and CranScan, with installation, packaging and upgrade evidence. Keep the isolated-consumer builds green. |
| Release control | All required checks green for the intended candidate; deliberately configure required branch checks. The GitHub main ruleset was disabled when inspected on 2026-10-02. |

Full Compose API parity and every optional platform service are scope decisions,
not automatic 1.0 requirements. Public statements must match the supported scope.

## Performance evidence

Open follow-ups are tracked in
[#902](https://github.com/samoylenkodmitry/Cranpose/issues/902),
[#901](https://github.com/samoylenkodmitry/Cranpose/issues/901),
[#809](https://github.com/samoylenkodmitry/Cranpose/issues/809),
[#794](https://github.com/samoylenkodmitry/Cranpose/issues/794),
[#793](https://github.com/samoylenkodmitry/Cranpose/issues/793) and
[#792](https://github.com/samoylenkodmitry/Cranpose/issues/792).

Their issue titles and early measurements are historical baselines. Later
comments report merged improvements and explicitly limit their conclusions.
Re-measure the candidate rather than restating a historical allocation/FPS/PSS
number as today's result. The SurfaceFlinger timestamp correction in #792/#902
also prevents treating the old desired-time deltas as established end-to-end
latency.

## Fresh checks for the documentation change

On 2026-10-02, using the pinned toolchain on macm3:

- `cargo nextest run --cargo-profile ci -p desktop-app -p cranpose-ui -p cranpose-app-shell --no-fail-fast`:
  2,091 passed, four configured skips. This includes 18 documentation regressions,
  offline startup, animated chapter selection and interruption, wheel fling,
  compact navigation, all-tab rendering and accessibility. A GPU pixel regression
  verifies repeated window growth and shrinkage without stale clipping and keeps
  the repository link visible at the resized window's corner.
- GPU robot runs passed at 390×780, 800×600, 1100×820 and 1440×1000, covering
  continuous reading, tabs scrolling out of view, wheel/article synchronization,
  chapter actions, compact back navigation and fling interruption. Captures
  confirm the wheel's labels and markings rotate together beneath the glass.
- `cargo nextest run --cargo-profile ci -p cranpose-render-common -p cranpose-ui
  -E 'test(annotated_text_baselines) or test(widget_composition)'`:
  49 passed; unrelated tests were filtered out.
- `just clippy` passed for the workspace, all targets and the facade crate.
  Local `just precommit` passed; 82 relative links in the original documentation
  refresh resolved to existing targets.

These results do not refresh physical-device or cross-framework measurements.

## 0.9.0 release evidence

The [0.9.0 crates](https://crates.io/crates/cranpose/0.9.0) were published on
2026-10-02. The immutable `v0.9.0` tag points to
`922045d40ae6a5d9dba979cad78cbfc26c11512f`.

- The production source at `79bdf82a9` passed all nine checks in
  [Rust](https://github.com/samoylenkodmitry/Cranpose/actions/runs/36972310086)
  and [heavy CI](https://github.com/samoylenkodmitry/Cranpose/actions/runs/36972310046).
  The tag adds automated version metadata and
  [workspace inheritance for the macro dependency](https://github.com/samoylenkodmitry/Cranpose/pull/1085).
  Locked Cargo resolution, tag verification and precommit gates passed for
  that dependency repair.
- [Publication and isolated consumer checks](https://github.com/samoylenkodmitry/Cranpose/actions/runs/36973849809)
  passed. The desktop, web and Android consumer builds resolved the published
  0.9.0 crates outside the workspace.
- After the workflow aligned the workspace and isolated consumer at 0.9.0,
  main at `35cad0b97` passed all nine checks in fresh
  [Rust](https://github.com/samoylenkodmitry/Cranpose/actions/runs/36976459199)
  and [heavy CI](https://github.com/samoylenkodmitry/Cranpose/actions/runs/36976459286)
  runs, including the macOS workspace tests, Linux robots, Android and iOS builds.
- [Web deployment](https://github.com/samoylenkodmitry/Cranpose/actions/runs/36975505681)
  passed from the final tag; the
  [hosted demo](https://samoylenkodmitry.github.io/Cranpose/) returned HTTP 200.
- The [release page](https://github.com/samoylenkodmitry/Cranpose/releases/tag/v0.9.0)
  carries the platform downloads and release notes. Its
  [artifact build record](https://github.com/samoylenkodmitry/Cranpose/actions/runs/36977364842)
  is separate from crate publication.

For subsequent candidates, follow [the release runbook](release.md). Keep the
tag, check URLs, publication, artifacts and consumer evidence together. Dated
device reports retain their original dates and scope; a successful release
does not refresh those measurements.
