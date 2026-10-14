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
| Linux desktop, X11 | Vulkan or OpenGL driver | wgpu | 1 | The robot suite on every push and every night ([heavy](../.github/workflows/heavy-selfhosted.yml), [nightly](../.github/workflows/nightly.yml)); AT-SPI activation, ranges, text and modals every night. |
| Windows desktop | Windows 10, DirectX 12 or Vulkan | wgpu | 1 | UI Automation and the inspector robot every night ([runtime matrix](../.github/workflows/runtime-matrix.yml)). |
| macOS desktop | macOS 12, Metal | wgpu | 1 | Workspace tests on every push ([Rust](../.github/workflows/rust.yml)). |
| Chrome | WebGPU, or WebGL2 without it | wgpu | 1 | Every page on WebGPU and WebGL2 on every push; focus, accessibility and composition every night. |
| Firefox | WebGPU, or WebGL2 without it | wgpu | 1 | WebDriver keyboard checks every night. |
| Safari on macOS | WebGPU, or WebGL2 without it | wgpu | 1 | WebDriver keyboard checks every night. |
| Android | API 24 (7.0), Vulkan or GLES 3 | wgpu | 1 | Accessibility navigation, audit and interaction tests in an API 35 emulator every night. |
| iOS | iOS 15 | wgpu (Metal) | 1 | XCTest accessibility tests in an iPhone simulator every night. |
| Linux desktop, Wayland | Vulkan or OpenGL driver | wgpu | 2 | Built with the X11 shell; checked by hand. |
| Wear OS | As Android | wgpu | 2 | Built on every push; checked on a Pixel Watch 3. |
| watchOS | watchOS 10 (simulator), 27 (device) | SpriteKit host | 2 | Built on every push; checked in the simulator and on an Apple Watch. |
| ESP32-S3 | ESP-IDF | pixels | 2 | Boots in QEMU to its first frame every night. |
| Embedded in a native app | Android and UIKit hosts | wgpu | 2 | Built on every push; text input, readers and multi-touch inside an embedded screen are incomplete (#1161). |

Physical-device runs are recorded in
[device measurement](device_measurement.md),
[VoiceOver validation](accessibility_voiceover_validation.md) and the
[release readiness ledger](release_readiness.md).

## Minimum Rust

The crates declare `rust-version`; see [versioning](release.md#versioning).
