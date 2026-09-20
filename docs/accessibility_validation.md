# Accessibility robot validation

The shared `accessibility_robot` screen runs in the production demo on every
platform. The robots inspect the operating system or browser accessibility tree,
perform native actions, and verify the resulting application state. They exit
nonzero on failures and retain a `report.json` plus diagnostic artifacts in a new
output directory. Do not reuse an output directory.

## Coverage

Debug builds show a floating **Inspector** control. Click it to open the panel;
drag the control or the panel's title bar to move it out of the way. Positions
stay within the window when it resizes. No opening shortcut is registered.
**Normal**, **Overlay**, and **A11y only** switch
between app rendering, numbered accessible bounds, and the accessible representation.
**Pick element** temporarily hides the panel so any element can be selected without
activation. The reading-order list and property panel show the shared platform
projection, including names, roles, values, focus, state, bounds, and actions.
Password values remain protected and hidden elements remain excluded.

The panel and overlays use a separate render graph and input route; they never
become application layout or semantic nodes. Platform readers still activate the
application while inspection is open. Blue outlines mark accessible bounds, green
marks app focus, purple marks selection, and amber marks unnamed actionable elements.
Arrows select elements, 1/2/3 switch views, P picks, Page Up/Down scroll properties,
and Escape closes the panel. Click the app to return keyboard input to it.
Closed inspectors collect no tree snapshots. Release builds default to disabled;
`AppLauncher::with_developer_inspector(bool)` overrides either default.

The **build one** workflow accepts runner **Windows** and target **accessibility**.
It runs `just test-windows-accessibility` on a hosted Windows desktop, checks native
UI Automation actions, and exercises the floating inspector through the robot.
The `windows-accessibility` artifact retains native reports and inspector pictures.

Run the inspector end-to-end robot with:

```sh
CRANPOSE_INSPECTOR_ARTIFACTS=/tmp/inspector-pictures ./run_robot_test.sh --example robot_developer_inspector --sequential
```

On Linux without an X11 or Wayland session, prefix the command with `xvfb-run -a`
(put the environment assignment before that prefix). The runner acquires its
own host lock; do not wrap it in another host lock.

The robot checks distinct rendered modes, exact picture restoration, tree isolation,
privacy, picking without activation, keyboard navigation, and live app-state updates.
The optional artifact directory receives screenshots of each mode and selection.
This is the application's common accessibility projection; native platform tools
and the robots below validate the final OS/browser representation.

| Behavior | macOS AX | Linux AT-SPI | Windows UIA | Android | iOS XCTest | Browser AX/DOM |
| --- | --- | --- | --- | --- | --- | --- |
| Repeated words and independent nested controls | Yes | Yes | Yes | Yes | Yes | Yes |
| Hidden content and password privacy | Yes | Yes | Yes | Yes | Yes | Yes |
| Disabled state and rejected activation | Yes | Yes | Yes | Yes | State | Yes |
| Native activation changes application state | Yes | Yes | Yes | Yes | Yes | Yes |
| Passive progress and adjustable ranges | Yes | Yes | Yes | Yes | Values | Yes |
| Editable text reaches application state | Yes | Yes | Yes | Yes | Native keyboard | Native input/keyboard |
| Selection survives editing | — | — | — | Yes | — | Yes |
| Accessibility reconnect and navigation | — | — | — | Yes | — | Navigation |

Coverage describes implemented assertions, not a claim that every platform has
been run on every change. Record the actual platform, artifact hash, test counts,
and reports for a validation run. Windows cross-compilation does not prove UIA
behavior; an interactive Windows runner must execute its robot. iOS simulator
tests do not substitute for VoiceOver and hardware checks on an iPhone.

## Desktop

Build the same desktop app with `just build-accessibility-desktop`, then run:

```sh
just robot-accessibility-macos target/ci/desktop-app /tmp/a11y-macos
just robot-accessibility-linux target/ci/desktop-app /tmp/a11y-linux
just robot-accessibility-windows target/ci/desktop-app.exe a11y-windows
```

Each command runs five assertion groups and saves the native tree, application
log, and executable SHA-256. macOS requires Accessibility permission for the
terminal or runner; the recipe installs its pinned PyObjC dependency in a local
virtual environment. Linux requires Python GObject introspection, the AT-SPI 2
typelib, `busctl`, `dbus-run-session`, and `xvfb-run`. It enables accessibility on
a private session bus and takes the host lock. Windows requires Python,
PowerShell, and an interactive desktop. The runner scopes UIA to the process it
launched and uses Invoke, Value, and RangeValue patterns.

`just clippy-windows` checks the Windows build from a non-Windows host using
`cargo-xwin`; install that tool before running the recipe. Native builds use the
normal Windows Rust toolchain.

The AccessKit family is upgraded together. The workspace pins the core,
consumer, and AT-SPI common crates to upstream commit
`6ee0558b6315b3ef1594db24ce45a030ecac7cb5`, which fixes disabled nodes being exposed
as enabled on Linux. The released AT-SPI adapter does not yet contain that fix.
The Linux robot checks the resulting native state and editable-text behavior.

## Web

```sh
bash scripts/a11y/build_platform.sh web /tmp/a11y-web-build
just robot-accessibility-web /tmp/a11y-web-build/site /tmp/a11y-web
```

The build runs the shipped wasm feature lint, release build, and packaging.
The robot serves only the packaged site on a private localhost port and runs
headless Chrome. Set `CHROME` to select an executable. Its fifteen checks cover
native names and roles, disabled activation, adjustable values, retained DOM
identity and focus, forward/backward Tab, single keyboard activation, list
ownership, dictated Unicode input, selection, and keyboard editing. Artifacts
include the browser log, native tree, screenshot, and report.

## Android

```sh
just android-robot-build
just robot-android-accessibility emulator-5554 /tmp/a11y-android
```

The ten instrumentation tests include Google's Accessibility Test Framework,
tree parsing, live navigation, reconnect, and actions on the shared screen.
They target the release robot APK and Android's real `UiAutomation` node tree.
The runner locks and wakes the selected device, records both APK hashes, and
retains instrumentation output. Use an explicitly selected, dedicated emulator
for reconnect: another accessibility service keeps the application connected.

On a shared physical device whose accessibility service must remain active,
run `just robot-android-accessibility-connected SERIAL OUTPUT`. This runs nine
tests and explicitly records reconnect as not requested. Both modes reject
skipped tests, runner failures, missing results, and unexpected test counts.

## iOS

```sh
just ios-sim
just robot-accessibility-ios target/aarch64-apple-ios-sim/debug/CranposeDemo.app SIMULATOR_UUID /tmp/a11y-ios
```

Select an available simulator UUID from `xcrun simctl list devices available`.
The runner boots that simulator if necessary and installs the app. Three XCTest
tests inspect names, traits and numeric values, activate native elements, and
tap and type through the native keyboard. All three must execute and pass with
zero skips or expected failures. The result bundle includes native hierarchy
and screenshot attachments; `xcodebuild.log` and the executable hash accompany
the report. `just clippy-ios` checks the shipped simulator feature set;
`just ios-device` checks the physical-device binary build.

## Regression and harness checks

Run `just test-android-accessibility-contract` to verify that the Android,
desktop, and iOS result guards accept valid runs and reject their intended
failures. Shared Rust regressions run under `just test`, including pointer focus
publication and clearing, semantic names and boundaries, disabled actions,
password privacy, and progress roles.

The robots validate native contracts, not spoken output, speech timing, every
screen reader's navigation model, or all application screens. Complete a release
review with VoiceOver, TalkBack, Orca, and NVDA or Narrator on the relevant
hardware. Keep runtime results separate from build checks and manual review.
