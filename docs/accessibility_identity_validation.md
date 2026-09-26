# Accessibility identity validation

Validation on 2026-09-21, starting from main `39b18fe1`.

## Regressions

The former canvas ID calculation discarded high key bits and resolved collisions
in reading order. Two controls with keys `0` and `1 << 31` on the same canvas
exchanged native IDs when reordered. The regression failed before replacing this
calculation with a snapshot that retains live IDs and retires removed IDs.
Android, iOS, desktop AccessKit, and the browser now publish and resolve actions
through that same snapshot abstraction.

Additional failing regressions established that a pane title changed a dialog's
native role to Region, a selection request could name a different field from its
target, and a 300-line field published only 255 text runs. Tests now require the
correct native role, matching selection ownership and run bounds, every text run,
and a caret that resolves to a published run. Duplicate identities and exhausted
IDs reject the update without changing the published snapshot.

## Native and browser runs

| Target | Recipe or runner | Result |
| --- | --- | --- |
| macOS 27.0 AX | `just robot-accessibility-macos` | 7 groups passed |
| Linux AT-SPI on samarch-1 | `just robot-accessibility-linux` | 7 groups passed with strict disabled-state checking (AccessKit AT-SPI 0.21) |
| Physical Android 10, arm64 | `android_accessibility_robot.py --connected-only` | 9 tests passed, including Accessibility Test Framework audit |
| iOS 26.5 simulator, iPhone 17 Pro | `just robot-accessibility-ios` | 3 XCTest cases passed, none skipped |
| Packaged release browser | `web_robot.py` | 27 checks passed |

The Android and iOS shipped feature checks passed. The web build passed the
shipped wasm lint and `just web`. The full workspace test run on macm3 passed
5,633 tests with 12 configured skips. Linux initially passed 5,631 of 5,637 tests:
three architecture assertions needed updating for the shared snapshot and dialog
helper; those now pass in all 130 platform scheduling checks. The other three
failures were rendering checks, including a Vulkan driver teardown fault; all
three passed on the Apple host.

The Linux native traversal encountered an object removed during modal dismissal.
Its existing bounded retry now recognizes the native AT-SPI missing-object error
as well as the D-Bus form. Regression tests require a complete retry, a failure
after three attempts if the object remains unavailable, and immediate propagation
of other errors. `just test-android-accessibility-contract` passed all 18 Python
tests, including these checks.

## Artifact fingerprints

| Artifact | SHA-256 |
| --- | --- |
| macOS desktop executable | `a135de2b2e5c9bc734bfae955f0f8af2cf4e5a9e437cb73b14a1226e7a482171` |
| Linux desktop executable | `120dc0fdec24fd9f2d622c37b8ef634f1d8b1bab2a45cea3bf8aabb00a016843` |
| Android release APK | `6878eb448d0bcf778eb674b4f31bf708e7b092766a047614bafffa65c86a2c57` |
| Android instrumentation APK | `cd313a38f0a2d3c5e9bb8406ed6c840ba96d7003bf64138e619554305be64f6f` |
| iOS simulator executable | `0b2f2394f4440f6eda9601adc41ce13716d070b0e450deb006bf8465c0271d70` |

Each native runner retained its `report.json`, logs, and fingerprint. Browser
reports retain the 27 individual checks and native tree. Reproduction commands
and prerequisites are in [accessibility validation](accessibility_validation.md).

## Remaining verification

The Windows UIA robot needs an interactive Windows desktop, so the AccessKit
upgrade to 0.35.1 there is checked by `just clippy-windows` alone.

The physical Android run kept existing accessibility services connected and did
not run the service reconnect test. The iOS run used a simulator. Native tree
and action tests do not establish spoken output, reader gesture behavior, or
usability for blind people. The reader/browser matrix, physical iPhone checks,
and usability sessions with blind users remain necessary before a general
quality or conformance claim.
