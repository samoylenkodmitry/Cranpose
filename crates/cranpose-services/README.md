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

The activity also needs `<uses-permission android:name="android.permission.VIBRATE" />`
in its manifest — without it `Vibrator` silently does nothing.

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
