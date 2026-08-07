# Capability parity — every service API real on every platform

Status: implementation contract (2026-07-10). Companion audit: the
0.1.44→0.1.57 extraction arc added the service registry + iOS backends but
left Android/desktop/web unfilled, so cranscan ships ~1,500 lines of app-side
Java/JNI duplicating framework concerns.

Registry pattern (kept): trait in `cranpose-services` + `set_platform_X` +
accessor with a documented graceful default. The fixes below fill the empty
slots, kill app-side duplication, and centralize feature plumbing. No
`cfg(target_*)` in app-facing API. Where a platform genuinely lacks the
concept the accessor must say so (`is_supported`/`None`/`Unsupported`) —
never silently pretend.

## Target matrix (■ real backend, □ honest unsupported, ● built-in default)

| Capability        | ios | android | desktop | web | notes |
|-------------------|-----|---------|---------|-----|-------|
| file picker       | ■   | ■       | ■       | ■   | already parified |
| writable folder   | ■   | ■       | ■       | □   | already parified (web = honest unsupported) |
| soft keyboard     | ■   | ■       | ■       | ■   | already parified |
| uri handler       | ■   | ■ | ■ | ■ | backends existed; facade features now enable them |
| haptics           | ■   | ■ `performHapticFeedback` JNI | □ | ■ `navigator.vibrate` | desktop honest no-op |
| share sheet       | ■   | ■ ACTION_SEND + `CranposeShareProvider` | □ (save-dialog instead) | ■ Web Share API | |
| notifier          | ■   | ■ channels + deep-link Java | ■ zero-dep CLI (notify-send/osascript/PowerShell) | ■ Notification API | |
| network status    | □   | ■ ConnectivityManager Java | ● explicit assumption | ■ `navigator.onLine` | iOS NWPathMonitor still open |
| device info       | ■   | ● /proc/meminfo | ● linux; □ mac/win | ■ `navigator.deviceMemory` (reflective) | |
| clipboard         | ■   | ■ ClipboardManager JNI | ■ arboard | ● in-process fallback | web system-write bridge still open |
| back requests     | ■   | ■ back key → `push_back_request` behind `set_back_interception` | □ (apps map keys themselves) | □ | predictive back must stay off (`enableOnBackInvokedCallback=false`) |
| safe-area insets  | ■   | ■ WindowInsets listener → `local_safe_area_insets` | ● zero | ● zero | replaced cranscan's marker-file bridge |
| system theme      | ■ `window.theme()` polled | ■ uiMode + ConfigChanged | ■ winit `ThemeChanged` (+ cached env probe) | ■ `prefers-color-scheme` listener | drives LiquidTheme Auto |
| image picker      | ■   | ● file-picker fallback | ● | ● | camera source stays iOS-only for now |
| camera            | ■   | □ (cranscan keeps its system-camera round-trip; camera2 port is the next arc) | □ | □ | frames as RGBA `CameraFrame` |
| background activity | ■ | □ (FGS is app policy) | □ | □ | documented |
| file save dialog  | □ (export picker still open) | ■ ACTION_CREATE_DOCUMENT | ■ rfd save | ■ browser download | `FilePicker::save_file`; killed cranscan's direct rfd |
| launch arguments  | ● argv (`simctl launch`, `launchArguments`) | ■ intent extras + `onNewIntent` | ● argv | □ (query string still open) | `launch_args()`; `is_debuggable()` = `FLAG_DEBUGGABLE` on Android, `debug_assertions` elsewhere |
| in-app purchases  | ■ StoreKit 2 (`cranpose-storekit`) | ■ Play Billing (`playbilling` feature + `CranposeBilling`) | □ | □ | `purchases::store_state()` snapshot read from the frame loop; a platform without a store reports `StorePhase::Unavailable` and owns nothing, so a build with no backend never grants a paid entitlement by accident |

## Structural fixes

1. **Feature plumbing centralized in the facade.** `cranpose`'s `desktop-shell`
   / `android` / `ios` / `web` features enable the matching
   `cranpose-services/*` features (`uri-native`, `uri-android`, `uri-web`,
   `file-picker-native`, `file-picker-web`, `system-theme`, …). Apps stop
   re-listing them (desktop-demo and cranscan Cargo.tomls shrink).
2. **`CranposeActivity`.** `CranposeFilePickerActivity` is renamed to what it
   is — the cranpose base activity — and gains the new capability hooks
   (share, notifier + deep-link relay, network callback, WindowInsets
   listener, clipboard, haptics, ACTION_CREATE_DOCUMENT save). Java lives at
   `crates/cranpose/android/java/dev/cranpose/android/` (plus
   `CranposeShareProvider`, a zero-androidx content provider apps declare
   once in their manifest). Apps extend it; cranscan's `CranScanActivity`
   shrank to billing + recognition service + camera + shared-in images.
3. **`open_content_uri` stays** as a documented Android-only streaming
   utility: cranamp's audio engine streams tracks through it from the
   published crate, and the cross-platform replacement (a streaming
   `PickedEntry` read) is a separate design.
4. **Launcher provides nothing special-case.** Composition locals already
   default to `default_*()`; the one-off `ProvideUriHandler` wrap in every
   launcher path is removed. `Provide*` remains a test seam.
5. **Docs match reality** (writable-folder iOS note, navigation module doc).

## Still open

- iOS network monitor (NWPathMonitor) and export-style save dialog.
- Android camera2 backend for `cranpose_services::camera` (cranscan keeps its
  app-side system-camera round-trip + preview until then).
- Web system-clipboard write bridge; web camera (getUserMedia).
- Web launch arguments from the URL query string (`launch_args()` is empty on
  wasm today; the shell would install a snapshot from `location.search`).
- `local_*()` accessor seams for `camera`/`device_info`/`network`/`background`.

## cranscan adoption (done in the same arc)

Deleted from cranscan and replaced by the framework: RAM JNI
(`device_info()`), SAF backup JNI (`pick_writable_folder` /
`open_writable_folder`), share JNI (`default_share_sheet`), notifier JNI +
deep-link markers (`default_notifier` + `take_notification_deeplink`),
clipboard JNI (clipboard session), open-url JNI (URI handler), insets marker
bridge (`local_safe_area_insets`), back markers + predictive-back Java
(framework back interception; `enableOnBackInvokedCallback` now `false`),
desktop rfd/arboard deps (`save_file` + clipboard session). Kept app-side:
billing, recognition foreground service, model downloads, the system-camera
round-trip + preview, shared-in images (ACTION_SEND receiver).
