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
| device info       | ■ `NSProcessInfo` + `os_proc_available_memory` | ■ /proc + `getrusage`/`mallopt` | ■ `getrusage`; resident on linux | ● `navigator.deviceMemory` (reflective) | process readings — resident set, memory still available to this process, processor time, and returning free pages — sit beside the device total; every one is optional and a platform that will not say reports nothing rather than zero |
| clipboard         | ■   | ■ ClipboardManager JNI | ■ arboard | ■ Async Clipboard API (`web_clipboard`) | web reads are a promise, so the bridge takes the *paste* (`request_paste`) instead of answering `read_text` |
| back requests     | ■   | ■ back key → `push_back_request` behind `set_back_interception` | □ (apps map keys themselves) | □ | predictive back must stay off (`enableOnBackInvokedCallback=false`) |
| safe-area insets  | ■   | ■ WindowInsets listener → `local_safe_area_insets` | ● zero | ● zero | replaced cranscan's marker-file bridge |
| system theme      | ■ `window.theme()` polled | ■ uiMode + ConfigChanged | ■ winit `ThemeChanged` (+ cached env probe) | ■ `prefers-color-scheme` listener | drives LiquidTheme Auto |
| image picker      | ■   | ● file-picker fallback | ● | ● | camera source stays iOS-only for now |
| camera            | ■ `AVCaptureSession` | ■ Camera2 (`CranposeCamera`) | ● nokhwa on macOS and Windows (`camera-native`); Linux open | □ | frames pushed as `CameraFrame` with the turn carried as `rotation_degrees` (`upright_rgba8` applies it in the conversion pass); observable state and lens list (`CameraLenses`, `LensFacing`); bounded latest-wins analysis stream; stills asked for rather than waited on |
| background activity | ■ | □ (FGS is app policy) | □ | □ | documented |
| file save dialog  | □ (export picker still open) | ■ ACTION_CREATE_DOCUMENT | ■ rfd save | ■ browser download | `FilePicker::save_file`; killed cranscan's direct rfd |
| launch arguments  | ● argv (`simctl launch`, `launchArguments`) | ■ intent extras + `onNewIntent` | ● argv | □ (query string still open) | `launch_args()`; `is_debuggable()` = `FLAG_DEBUGGABLE` on Android, `debug_assertions` elsewhere |
| media playback    | ■ `AVAudioPlayer` + `AVAudioSession` + MediaPlayer | ■ `MediaPlayer` + `AudioManager` + `MediaSession` + `Visualizer` | ■ `cranpose-media` (symphonia + cpal) | ■ `<audio>` + Media Session + Web Audio | observable `PlaybackState`/`PlaybackProgress`; the audio-focus policy lives in the framework; analysis samples are capability-gated (iOS has none, Android needs `RECORD_AUDIO`); desktop and iOS play local files only; the equalizer is ten octave bands where the framework builds the filters, the device's own bands on Android, and absent on iOS |
| power             | ■ `NSProcessInfo.thermalState` + `UIDevice` battery | ■ `PowerManager` thermal status + `BatteryManager` | ■ thermal on macOS (`NSProcessInfo`, Foundation not UIKit); battery on Linux (`/sys/class/power_supply`) | ■ battery via `navigator.getBattery` when the browser has it | `PowerCapabilities` names the two halves separately, and `PowerReading` tells `Unsupported` apart from `Unknown` — a browser whose `getBattery` promise has not resolved is not a browser without one |
| bundled assets    | ■ `NSBundle` resource directory, streamed | ■ `AssetManager`; an uncompressed asset streams through its own descriptor | ■ beside the executable, in a `.app`'s `Resources`, or the working directory, streamed | □ | a browser has no packaged file — its resources arrive over HTTP, so the web fetches them through `http` instead |
| incoming content  | □ | ■ `ACTION_SEND` / `ACTION_VIEW` shares and `onNewIntent` | ■ files dropped on the window, and documents named in `argv` | ■ files dropped on the canvas | one `IncomingContent` stream on every platform that has one; winit owns the iOS `UIApplicationDelegate`, so `openURL` is not reachable, and a macOS `.app` opened from the Finder is told by an Apple Event winit does not surface — drops arrive, opens do not |
| app updates       | ■ check only | ■ check and install (`PackageInstaller`, digest verified before commit) | ■ check only | □ | `AppUpdateCapabilities` splits `check` from `install`: App Store Review Guideline 3.3.2 forbids an iOS application from replacing its own binary and the framework owns no desktop installer, so both report `install: false` rather than registering an installer that can only fail. The check needs the application's own `http-native` opt-in and reports `check: false` without it |
| host surface      | ● window size | ● window size | ■ observable size and resize requests | ■ canvas size, observable, with resize requests | |
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
   once in their manifest). Apps launch the framework-owned `CranposeActivity`
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

- Desktop battery on macOS and Windows. Linux publishes it from
  `/sys/class/power_supply`, which is a file to read; macOS would need IOKit's
  `IOPSCopyPowerSourcesInfo` and Windows `GetSystemPowerStatus`, each a second
  FFI surface for one number. `PowerCapabilities::battery` reports `false`
  there rather than answering with a guess.
- Desktop thermal on Linux and Windows. Neither exposes thermal *pressure*; the
  Linux kernel publishes temperatures, and a threshold this framework invented
  to turn degrees into a `ThermalState` would read as a measurement while being
  a guess.
- iOS network monitor (NWPathMonitor) and export-style save dialog.
- Web camera (getUserMedia).
- Network items for the desktop and iOS media backends. Both play local files;
  `AVPlayer` and a streaming `symphonia` source are what would close that,
  and until then the URI is refused rather than downloaded into memory first.
- Analysis samples on iOS. `AVAudioPlayer` metering gives an average and a peak
  per channel, not the samples a visualiser draws, so
  `MediaCapabilities::analysis` reports `false` rather than publishing
  something else under that name.
- An equalizer on iOS. Shaping playback needs an `AVAudioEngine` graph with an
  `AVAudioUnitEQ` in it; `AVAudioPlayer` plays a file to the output and has
  nowhere to put one, so `MediaCapabilities::equalizer` reports `false`. Moving
  the iOS backend onto `AVAudioEngine` would close this and the analysis gap
  above together.
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
