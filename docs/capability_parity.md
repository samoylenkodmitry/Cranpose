# Platform capabilities

This page describes current service backends and their public limits. Backend
presence records source support; [release readiness](release_readiness.md)
records device checks. A default value describes framework behavior, while a
platform backend reports an observed value.

## Current support

| Capability | Current support |
|---|---|
| File selection | iOS, Android, desktop, and web provide file selection. Save requests use `UIDocumentPickerViewController` on iOS, `ACTION_CREATE_DOCUMENT` on Android, `rfd` on desktop, and browser downloads on web. Writable folders work on iOS, Android, and desktop. |
| Keyboard | iOS and Android provide soft-keyboard services. Desktop exposes IME requests and platform control through `DesktopTextInput`; web text input uses browser controls. |
| URI launch | iOS, Android, desktop, and web provide URI handlers through facade platform features. |
| Haptics | iOS, Android, and web provide haptics. Desktop uses the service default. |
| Share | iOS and web provide native share sheets. Android uses `ACTION_SEND` with `CranposeShareProvider`; desktop applications can use file-save services. |
| Notifications | iOS, Android, desktop, and web provide notification services. Desktop dispatches through `notify-send`, `osascript`, or PowerShell. |
| Network status | Android uses `ConnectivityManager`; web tracks `navigator.onLine` and online/offline events. iOS and desktop use the framework default, which reports online and unmetered. |
| Device information | iOS, Android, and desktop report process memory and CPU readings where available. Web reads `navigator.deviceMemory` when the browser exposes the value. Optional readings use `Option` to represent platform availability. |
| Clipboard | iOS, Android, desktop, and web provide clipboard services. Web paste uses `request_paste` because browser reads complete asynchronously. |
| Back requests | iOS and Android publish back requests. Desktop apps map keys through their own input policy; web apps use browser navigation. Android predictive back remains disabled through `enableOnBackInvokedCallback=false`. |
| Safe area | iOS and Android report window insets. Desktop and web report zero insets. |
| System theme | iOS, Android, desktop, and web observe system theme changes. The service drives `LiquidTheme` Auto. |
| Image selection | iOS provides the native image and camera picker. Other platforms use the file-picker service. |
| Camera | iOS and macOS use AVFoundation; Android uses Camera2. The Apple backend publishes frames, lens lists, camera state, and still captures. macOS supports signed, bundled applications with `NSCameraUsageDescription`. |
| Background activity | iOS uses `beginBackgroundTask`; Android uses a foreground service. Other platforms use foreground execution. |
| Launch arguments | iOS and desktop read process arguments; Android reads launch intent extras. Web starts with an empty `launch_args()` snapshot. |
| Media playback | iOS uses AVFoundation and MediaPlayer; Android uses `cranpose-media` with Android audio focus and media sessions; desktop uses `cranpose-media`; web uses `<audio>`, Media Session, and Web Audio. Native and desktop `cranpose-media` decoders read HTTP(S) sources through byte-range requests. The iOS player accepts HTTP(S) URLs through AVPlayer. The web player passes URIs to the browser audio element. Android document URIs use the media source opener. |
| Media capabilities | The active player reports seek, speed, loop, analysis, session, equalizer, and probe support through `MediaCapabilities`. Native iOS reports playback analysis and equalizer as unavailable; Android and software playback provide both. |
| Power | iOS reports thermal state and battery status; Android reports thermal state, battery, and background restrictions. macOS reports thermal state; Linux reports battery status. Web reads battery status when `navigator.getBattery` exists. |
| Bundled assets | iOS streams assets from `NSBundle`; Android streams assets through `AssetManager`; desktop reads beside the executable, from app resources, or from the working directory. Web assets use HTTP. |
| Incoming content | Android accepts declared media shares and open-with intents at launch and through `onNewIntent`. Desktop and web accept file drops; desktop also reads file paths from `argv`. |
| App updates | iOS and desktop check for updates. Android checks and installs verified packages through `PackageInstaller`. Update checks require the application's `http-native` feature. |
| Host surface | iOS, Android, and web report window or canvas size. Desktop reports observable size and accepts resize requests. |
| In-app purchases | iOS uses StoreKit 2; Android uses Play Billing. Builds with either store backend expose purchase state; other builds report `StorePhase::Unavailable`. |

## Current API gaps

- iOS network status uses the framework's online, unmetered default. An iOS
  `NWPathMonitor` backend remains future work.
- The camera service covers native Apple and Android backends. Web applications
  can call browser camera APIs through application code.
- iOS media playback reports `analysis: false` and `equalizer: false` through
  `MediaCapabilities`; AVAudioEngine integration can provide both capabilities.
- Web `launch_args()` starts with an empty snapshot. Query-string interpretation remains
  application policy.
- Camera, device information, network status, and background activity expose
  service functions. The service modules reserve corresponding `local_*`
  CompositionLocal accessors for future API work.
- Web applications save files through browser downloads. Writable-folder
  selection covers iOS, Android, and desktop.
- Desktop power support covers macOS thermal state and Linux battery status.
  macOS battery status, Linux and Windows thermal state, and Windows battery
  status remain future platform work.
- Incoming-content adapters cover Android launch intents and desktop/web file
  drops. iOS open-URL events and macOS Finder document-open events remain future
  integration work.

## Service contract

Each service lives in `cranpose-services` and exposes a platform registration
function plus a public accessor. Services with a meaningful cross-platform
fallback return a documented default; optional services use `Option` or a
capability field. `cranpose` facade features select platform backends. The
Android platform guide lists required manifest declarations.
