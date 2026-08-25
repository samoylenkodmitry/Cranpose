# Cranpose Services

Multiplatform service abstractions for Cranpose applications.

## When to Use

This crate provides cross-platform interfaces and default implementations for:

- HTTP text fetching
- Opening external URIs
- Haptic feedback, including amplitude control and waveform patterns
- Sound effects and music

Applications can consume these services through CompositionLocals and override them in tests.

## Architecture

- **Interfaces**: `HttpClient`, `UriHandler`, `Haptics`, `AudioPlayer`
- **CompositionLocals**: `local_http_client()`, `local_uri_handler()`, `local_haptics()`, `local_audio()`
- **Default implementations**:
  - Desktop: `reqwest` for HTTP and `open` for URIs
  - Web: browser `fetch` and `window.open`
  - Android: `reqwest` for HTTP and `webbrowser` (ACTION_VIEW) for URIs

## Haptics

`HapticFeedback` names the seven semantic events every platform can express and
is what UI code should reach for. Below it sit three vibrator-level entry
points, for an app that designs its own set of distinct feels:

| Method | Android mapping |
| --- | --- |
| `Haptics::vibrate(duration_ms, amplitude)` | `VibrationEffect.createOneShot(long, int)` |
| `Haptics::play_pattern(&HapticPattern)` | `VibrationEffect.createWaveform(long[], int[], int)` |
| `Haptics::perform_effect(HapticEffect)` | `VibrationEffect.createPredefined(int)` |
| `Haptics::cancel()` | `Vibrator.cancel()` |
| `Haptics::has_amplitude_control()` | `Vibrator.hasAmplitudeControl()` |

All four carry a defaulted body that falls back to the closest
`HapticFeedback` constant, so a backend that implements only
`Haptics::perform` answers the whole trait and existing implementations keep
compiling.

`HapticPattern::new(timings_ms, amplitudes)` validates before anything reaches
the platform: the two slices must be the same length, there must be at least
one step, at least one timing must be non-zero, and a repeat index must point
at a real step. It returns `HapticError`, never a panic — which matters,
because `VibrationEffect.createWaveform` throws `IllegalArgumentException` on
the same inputs.

Amplitudes run 0 (off) to 255 (strongest). Devices without amplitude control
treat any non-zero amplitude as full strength; check
`Haptics::has_amplitude_control()` before designing around subtle levels.

Per-platform behaviour:

- **Android / Wear OS**: full support through the activity methods below.
- **iOS**: `UIFeedbackGenerator`. `vibrate` honours the amplitude as an impact
  intensity and ignores the duration; `play_pattern` plays a single impact
  weighted by the pattern's amplitude, because UIKit exposes no arbitrary
  waveform.
- **Web**: `navigator.vibrate`. `play_pattern` passes the timings through
  unchanged — the Vibration API takes exactly that array — and drops the
  amplitudes, which browsers cannot express. `cancel()` sends a zero-length
  vibration.
- **Desktop and the no-op default**: silent, and never panicking.

## Launch arguments

`launch_args()` is the equivalent of reading `intent.extras` in a Jetpack
Compose activity. A Cranpose app on Android is a `NativeActivity`: it sees
neither the environment of the shell that ran `am start` nor the launching
`Intent`, so flags read through `std::env::var` silently return nothing on
device.

```rust
use cranpose_services::launch_args;

let args = launch_args();
if args.is_debuggable() && args.boolean("ob_debug").unwrap_or(false) {
    let level = args.int("ob_level").unwrap_or(0);
    let seed = args.long("ob_seed").unwrap_or(0);
    let time_scale = args.float("ob_time_scale").unwrap_or(1.0);
    let screen = args.string("ob_screen").unwrap_or("");
}
```

`local_launch_args()` is the composition-local seam and `ProvideLaunchArgs`
the test seam; `isDebuggable()` reads the flag from composition.

| Platform | Source | How to pass one |
| --- | --- | --- |
| Android | extras of the launching `Intent` | `adb shell am start ... --ez ob_debug true --ei ob_level 7` |
| Desktop | process command line | `./app --ob_debug --ob_level=7` |
| iOS | process command line | `xcrun simctl launch <device> <bundle> --ob_level=7`, or `XCUIApplication().launchArguments` |
| Web | nothing by default | — |

The command line rather than the environment, because argv *is* the launch
payload: per-launch, not inherited by child processes, and already what the
platform tooling passes. `--name=value` is a text argument and a bare
`--name` is `true`; text parses on demand, so `--ob_level=7` reads back
through `int`.

`is_debuggable()` reports `ApplicationInfo.FLAG_DEBUGGABLE` on Android and
`cfg!(debug_assertions)` elsewhere. Gate debug options on it: extras still
arrive in a release build — Cranpose does not silently drop them, since a
deep link is an extra too — but the flag is `false`, so the options cannot be
switched on from outside a shipped app.

Android replaces the whole snapshot on `onNewIntent`, matching `setIntent`
replacing what `getIntent().getExtras()` returns; arguments from the previous
launch do not linger.

## Android activity contract

Apps that want these services on Android declare
`dev.cranpose.android.CranposeActivity` (or a subclass) as their launcher
activity; the methods below already live there. An app with its own activity
must provide the same names and signatures, because the Rust backend looks them
up by name over JNI. All of them are called from Rust on the native thread and
must not throw.

| Java method | JNI signature | Called by |
| --- | --- | --- |
| `void cranposeHaptic(int kind)` | `(I)V` | `Haptics::perform` |
| `void cranposeHapticOneShot(long durationMs, int amplitude)` | `(JI)V` | `Haptics::vibrate` |
| `void cranposeHapticWaveform(long[] timingsMs, int[] amplitudes, int repeat)` | `([J[II)V` | `Haptics::play_pattern` |
| `void cranposeHapticPredefined(int effect)` | `(I)V` | `Haptics::perform_effect` |
| `void cranposeHapticCancel()` | `()V` | `Haptics::cancel` |
| `boolean cranposeHapticHasAmplitudeControl()` | `()Z` | `Haptics::has_amplitude_control` |
| `String cranposeEncodeLaunchArguments()` | `()Ljava/lang/String;` | `launch_args` |

Argument encodings:

- `cranposeHaptic` `kind`: 0 light/selection, 1 medium, 2 heavy, 3 success,
  4 warning/error.
- `cranposeHapticOneShot` `amplitude`: `-1` for
  `VibrationEffect.DEFAULT_AMPLITUDE`, otherwise 1 to 255.
- `cranposeHapticWaveform` `repeat`: `-1` for a single pass, otherwise the
  index to loop back to. `timingsMs` and `amplitudes` always arrive with equal
  lengths — the Rust side rejects anything else before the JNI call.
- `cranposeHapticPredefined` `effect`: 0 `EFFECT_CLICK`, 1
  `EFFECT_DOUBLE_CLICK`, 2 `EFFECT_TICK`, 3 `EFFECT_HEAVY_CLICK`.
- `cranposeEncodeLaunchArguments` returns the launching intent's extras as one
  string: the first line is `1`/`0` for `ApplicationInfo.FLAG_DEBUGGABLE`, and
  each following line is `<type>\t<name>\t<value>` with `type` one of
  `b i l f s`. The activity also pushes the re-encoded payload to
  `nativeOnLaunchArguments(String)` from `onNewIntent`.

API-level guards, all present in `CranposeActivity`:

- `createOneShot` and `createWaveform` need API 26; below that the activity
  calls the deprecated `Vibrator.vibrate(long)` / `vibrate(long[], int)`
  overloads, which ignore amplitudes.
- `createPredefined` needs API 29; below that the activity substitutes a short
  one-shot of comparable weight.
- `VibratorManager` is used on API 31 and above, `Context.VIBRATOR_SERVICE`
  below it.

Wear OS 3 is API 30, so a watch build takes the amplitude and waveform paths;
the older branches exist for phones with a lower `minSdk`.

The vibrator needs `android.permission.VIBRATE`, which the
`cranpose-android-haptics` module contributes: an application adds
`services.add("haptics")` to the Gradle plugin's configuration rather than
writing the permission into its manifest. Without it `Vibrator` silently does
nothing.

## Audio

`AudioPlayer` is the sound interface: `load` / `load_clip` to hand the engine
decoded PCM, `play` and `play_loop` for voices, `stop` / `stop_voice` /
`stop_all`, `set_voice_params` to retune a running voice, and master and
per-bus volume and mute. `PlaybackParams { volume, rate, pan, bus }` describes
one voice; `rate` shifts pitch with speed, and
`PlaybackParams::pitch_semitones` expresses that in musical terms.

`SoundBank` and the `rememberSoundBank(&[SoundSpec])` composable load a set of
cues once and keep them alive across recompositions, releasing them when the
composable leaves. Each `SoundSpec` carries its own base volume and bus, so a
loud explosion and a quiet tick share one call site.

The compiled-in default is a no-op that still hands out real `SoundId`s and
remembers volume and mute settings, so an app behaves identically with and
without a device. The real engine is the `cranpose-audio` crate, installed with
`cranpose_audio::install()` (Cranpose's `audio` feature does this automatically
on Android).

**Audio needs no activity-side glue.** The Android backend is AAudio through
the NDK, so unlike haptics there is nothing to add to `CranposeActivity`.

## Media

`AudioPlayer` mixes short decoded cues. `MediaPlayer` plays one long encoded
item — a track, a podcast, a recording — through whatever the platform already
uses for media, and everything about it is published rather than polled:

* `rememberPlaybackState()` is what it is doing: `Loading`, `Playing`,
  `Paused`, `Ended`, `Failed`. `rememberPlaybackProgress()` is where it is —
  position, duration, buffered — and `playback_progress()` reads the same thing
  outside composition, for a seek bar being dragged or a waveform being drawn.
* `open_media(MediaItem)`, `play_media()`, `pause_media()`, `toggle_media()`,
  `stop_media()`, `seek_media(Duration)` and `seek_media_fraction(f32)` are the
  transport. A seek is clamped to the item here rather than in each backend.
* `set_media_volume` is the volume the *application* asks for. What reaches the
  device is that combined with the audio-focus gain, so an app may change its
  volume while ducked without undoing the duck.
* `publish_audio_focus(AudioFocus)` is what a backend calls when the device
  changes its mind, and the framework applies the policy every app otherwise
  gets wrong: duck and un-duck, pause on a transient loss and resume **only if
  it was the one that paused**, stop for good on a permanent one.
* `rememberMediaCommands()` carries the buttons pressed outside the app's own
  UI — a lock screen, a notification, a headset, a car. The transport commands
  have already been carried out by the time they arrive; what an app acts on is
  `Next` and `Previous`, which need the playlist it owns. `set_media_metadata`
  is what those surfaces show.
* `set_media_analysis_enabled(true)` turns on `rememberMediaSamples()` /
  `latest_media_samples()` for a visualiser. Off by default, latest-wins and
  bounded like camera frames, and only where `media_capabilities().analysis`
  says the platform will give the samples up.
* `media_equalizer_bands()` reports the bands the platform actually has, centre
  frequency and range, and `set_media_equalizer(EqualizerSettings)` applies a
  curve clamped to them. Backends that build their own filters report the
  contract's octave set (`OCTAVE_BAND_CENTERS_HZ`), so a curve saved on a
  desktop means the same thing in a browser; a platform effect reports what its
  implementation has, which is usually fewer bands on different centres. The
  curve is remembered whether or not a device can apply it, so a stored user
  setting survives one that cannot and reaches one that can.

Playback holds a background-work lease while it runs, so it carries on with the
app off screen; a host being destroyed stops it.

| Platform | Backend | Streams | Session | Analysis | Equalizer |
| --- | --- | --- | --- | --- | --- |
| Desktop | `cranpose-media` (`symphonia`, cpal) | local files | — | yes | 10 octave bands |
| Android | `cranpose-media` (`symphonia`, AAudio) plus `AudioManager` and `MediaSession` | local files and provider documents, including ones a provider streams | yes | yes | 10 octave bands |
| iOS | `AVAudioPlayer`, `AVAudioSession`, MediaPlayer | local files | yes | — | — |
| Web | `<audio>`, Media Session API, Web Audio | local and network | where the browser has it | yes | 10 octave bands |

Desktop and Android decode with the same crate, `cranpose-media`, enabled
through Cranpose's `media` feature. Android's own `MediaPlayer` is not in that
path and cannot be: it plays a file, and a document provider whose bytes come
off a network hands back a pipe, which it refuses. An in-process decoder needs
only bytes, so what plays on Android is what `symphonia` reads — and a stream
that cannot seek is spooled to the application's cache as it arrives. Java keeps
the half of the stack only it has, the audio-focus broker and the lock screen.
The desktop shell installs the backend itself; Android installs it wrapped in
that session, along with the rest of its services. Android applications add
`services.add("media")` to the Gradle plugin's configuration so the manifest
carries the playback service. iOS and the web register their platform backend.

## Device and process information

`device_info()` answers two different kinds of question. *What does this device
have* — `total_memory_bytes()` — sizes a decision made once, like whether a
model fits at all. *What is this process using, and what may it still have* is
asked while work is running, because the answer moves and because the platform
kills a process that gets it wrong:

* `resident_memory_bytes()` is what this process is holding. It is the number
  Android kills by, so it is the one a decode loop watches.
* `available_memory_bytes()` is what this process may still allocate — not free
  system memory. A device with gigabytes free will still stop this process at
  its own ceiling, and it is the second number that decides whether the next
  allocation is the one that ends the application.
* `process_cpu_time()` is how much of a core the work took, rather than how long
  it took. A background lane that must not heat the device rations this.
* `release_free_memory()` asks the allocator to give back pages a finished
  buffer no longer needs. Freeing a buffer does not shrink the process by
  itself, and on a platform that kills by resident size that is the difference
  between finishing and being killed. It reports whether the platform has such
  a call at all.

Every reading is optional. A platform that will not say reports `None` rather
than a zero an application would treat as "no memory left". The parts that can
be read from safe Rust — `/proc/meminfo`, `/proc/self/statm` — are the services
default; the rest need `libc` and live in the `cranpose` crate, installed over
the platform's own device info at startup so no application writes `getrusage`,
`mallopt` or `os_proc_available_memory` for itself.
