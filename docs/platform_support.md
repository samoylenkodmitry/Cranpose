# Platform support

This page names the platforms Cranpose supports and the checks that run the
app on each of them. A build alone does not count as support: a platform is in
tier 1 only when CI starts the app there and drives it.
[Platform capabilities](capability_parity.md) lists the services each platform
has, and [accessibility validation](accessibility_validation.md) lists the
reader checks.

## Tiers

**Tier 1.** CI runs the app on the platform on every push or every night, and a
failure is a red build.

**Tier 2.** CI builds the platform on every push. Runs happen on devices by
hand, with dated evidence in the linked documents.

| Platform | Minimum | Renderer | Tier | Runtime check |
| --- | --- | --- | --- | --- |
| Linux desktop, X11 | Vulkan or OpenGL driver | wgpu | 1 | The robot suite on every push and every night ([heavy](../.github/workflows/heavy-selfhosted.yml), [nightly](../.github/workflows/nightly.yml)); AT-SPI activation, ranges, text and modals, the menu bar drawn in the window, secure and Unicode input and navigation every night. |
| Windows desktop | Windows 10, DirectX 12 or Vulkan | wgpu | 1 | UI Automation, the inspector robot, the native menu, secure and Unicode input and navigation every night ([runtime matrix](../.github/workflows/runtime-matrix.yml)). |
| macOS desktop | macOS 12, Metal | wgpu | 1 | Workspace tests on every push ([Rust](../.github/workflows/rust.yml)); accessibility, the native menu bar, secure and Unicode input, navigation and GPU device loss every night. |
| Chrome | WebGPU, or WebGL2 without it | wgpu | 1 | Every page and a GPU device loss on WebGPU and WebGL2 on every push; focus, accessibility and composition every night. |
| Firefox | WebGPU, or WebGL2 without it | wgpu | 1 | WebDriver keyboard checks every night. |
| Safari on macOS | WebGPU, or WebGL2 without it | wgpu | 1 | WebDriver keyboard checks every night. |
| Android | API 24 (7.0), Vulkan or GLES 3 | wgpu | 1 | Accessibility navigation, audit and interaction tests and a stop and resume in an API 35 emulator every night. |
| iOS | iOS 15 | wgpu (Metal) | 1 | XCTest accessibility tests and a background and return in an iPhone simulator every night. |
| Linux desktop, Wayland | Vulkan or OpenGL driver | wgpu | 2 | Built with the X11 shell; checked by hand. |
| Wear OS | As Android | wgpu | 2 | Built on every push; checked on a Pixel Watch 3. |
| watchOS | watchOS 10 (simulator), 27 (device) | SpriteKit host | 2 | Built on every push; checked in the simulator and on an Apple Watch. |
| ESP32-S3 | ESP-IDF | pixels | 2 | Boots in QEMU to its first frame every night. |
| Embedded in a native app | Android and UIKit hosts | wgpu | 2 | Built on every push; text input, readers and multi-touch inside an embedded screen are incomplete (#1161). |

## Scenarios

What the runtime checks drive, and where. Each job installs the build and starts
it cold, so a job fails when the app does not start and show its first screen.
The record job of the [runtime matrix](../.github/workflows/runtime-matrix.yml)
keeps each job's result and the SHA-256 of every file the jobs kept.

| Scenario | Where it runs |
| --- | --- |
| Startup | Every job below, from a fresh install. |
| Focus and keyboard | Chromium focus checks; WebDriver keyboard checks in Firefox and Safari; the desktop accessibility robots. |
| IME, secure and Unicode input | `robot_secure_unicode_input` on Linux, Windows and macOS; the web IME checks in Firefox. |
| Navigation and restoration | `robot_navigation_restoration` on Linux, Windows and macOS; the Android navigation tests. |
| Accessibility actions | AT-SPI, UI Automation, macOS accessibility, Android and iOS accessibility tests, the web mirror in Chromium. |
| Lifecycle | Android: the activity stops, losing its surface, and resumes. iOS: home, then the app again. Both check the state and a new frame. |
| GPU recovery | `robot_gpu_device_loss` on macOS (two losses, then four in a row ending the app); both web backends on every push. Android and iOS: the `cranpose_lose_gpu_device` drill by hand ([capabilities](capability_parity.md)). |
| Menus | `robot_desktop_menus` on Linux, Windows and macOS; `robot_window_menu_bar` on Linux and macOS. |

Physical-device runs are recorded in
[device measurement](device_measurement.md),
[VoiceOver validation](accessibility_voiceover_validation.md) and the
[release readiness ledger](release_readiness.md).

## Minimum Rust

The crates declare `rust-version`; see [versioning](release.md#versioning).
