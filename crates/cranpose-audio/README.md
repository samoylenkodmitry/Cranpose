# Cranpose Audio

The real-time audio engine behind `cranpose_services::audio`.

`cranpose-services` owns the Compose-shaped API — the `AudioPlayer` trait, the
`SoundId` handle, `ProvideAudio`, `rememberSoundBank` — and ships a no-op
default so an app compiles and runs anywhere. This crate is the implementation
that makes sound come out: a software mixer running on the platform's real-time
audio thread, fed from the UI thread through a lock-free queue.

```rust,ignore
// Once at startup, on the thread that runs the composition.
cranpose_audio::install();
```

Android registers it automatically when the `cranpose/audio` feature is on.

## Why AAudio on Android and Wear OS

The three realistic Android paths, and why this crate takes the first:

| Path | Verdict |
| --- | --- |
| **AAudio through the `ndk` crate** | **Chosen.** Pure Rust over `libaaudio.so`, which every Android 8+ and Wear OS 3+ device ships. It hands us a real-time callback, so the mixer — pitch shifting, panning, polyphony, buses — is the same code on every platform. `ndk` is already a Cranpose dependency on Android, so the engine adds no new library, no Java, and no C++ toolchain. |
| `oboe` (the Rust binding to Google's Oboe) | Rejected. Oboe earns its keep through the OpenSL ES fallback it provides for API 16–25 and the per-device quirk table it carries for those releases. Wear OS 3 is API 30, so that fallback is dead weight here, while `oboe-sys` compiles Oboe's C++ through CMake and needs `libc++_shared.so` packaged into the APK. That is a large build-system cost for a path this crate never takes. |
| JNI `SoundPool` / `AudioTrack` | Rejected. `SoundPool` would hand polyphony, rate and pan to the platform, but it caps the rate at 0.5–2.0 (too narrow for a combo counter that climbs), limits clips to about 1 MiB, loads asynchronously with no ordering guarantee, and gives no way to mix music and effects on separate buses. `AudioTrack` in streaming mode would work, but every buffer would cross JNI from a Rust thread — more overhead than the NDK callback, and it would need Java on the activity side. |

Consequences of the choice, stated plainly:

* **The audio engine needs no Java glue at all.** Unlike haptics, there is
  nothing to add to `CranposeActivity`.
* The floor is API 26. `AAudioStreamBuilder_setUsage` and `setContentType` are
  API 28, so this backend does not call them; the stream keeps AAudio's default
  usage rather than raising the minimum SDK.
* The stream asks for `LowLatency`, 32-bit float, two channels. If AAudio opens
  the stream in another format the engine reports
  `AudioError::Backend(..)` rather than writing samples of the wrong type.
* A device disconnect (headphones unplugged) ends the stream; the engine opens
  a new one the next time it is asked to play.

## Desktop

`cpal` behind the `cpal-backend` feature, off by default. It exists so a
developer on macOS, Windows or Linux hears what the watch will produce; it runs
the same mixer, so the only difference between platforms is how the callback
arrives.

It is off by default because it links a system audio library. On Linux that is
ALSA, whose development headers (`libasound2-dev` or the distribution
equivalent) must be present at build time. Enable it through Cranpose with
`features = ["audio-desktop"]`.

## Keeping the device shut

A `LowLatency` output stream on Android is an MMAP route with the always-on
audio DSP behind it, and it draws power for as long as it exists — tens of
milliwatts on a phone, whether or not the samples crossing it are zero. So the
engine treats the device as something to borrow, not to hold:

* **Opening it is triggered by sound, not by setup.** `install()` does not open
  it, and neither does `load_clip`. Loading a clip is a push onto the command
  queue, and that queue is created with the engine, so a whole sound bank can be
  resident with no device open at all; the first mixer to start drains the loads
  that were waiting for it. The first `play` is what opens the device.
* **It is given back when nothing is playing.** After two seconds with no voice
  putting samples into a buffer, the mixer stops the stream: the AAudio callback
  returns `AAUDIO_CALLBACK_RESULT_STOP`, and the engine calls
  `AAudioStream_requestStop` to release the route. The next `play` starts it
  again. Two seconds is chosen to sit above the gap between taps in a menu and
  above the length of a one-shot cue, so an active screen never thrashes the
  stream while an idle one goes quiet almost at once.
* **The handover is race-free without locking.** The engine queues its command
  and then reads a shared flag; the mixer clears the flag and then re-reads the
  queue. A `SeqCst` fence on each side puts both pairs into one order, so every
  interleaving leaves exactly one side responsible for the stream. Neither side
  spins, and the real-time thread does two atomic operations for it.

A consequence worth stating: `load_clip` can no longer report that the device is
missing, because finding that out means opening it. Its only error is
`ClipTableFull`; whether sound works is `AudioPlayer::is_available()`, and the
device's own failure is `AudioEngine::take_last_error()` once a play has tried.
That matches `NoopAudioPlayer`, which also hands out real `SoundId`s on a
machine with no audio.

The cpal backend is treated the same way with one honest gap: a cpal data
callback returns nothing, so it cannot stop its own stream. The mixer still
publishes that it has gone idle and the engine pauses the stream on its next
call, which means a desktop app that plays a sound and then never touches the
engine again holds its stream longer than an Android one would. Desktop has no
always-on audio DSP to keep awake, and a timer thread to close that gap would
cost more than it saves.

## Keeping the audio callback real-time

The platform calls back on a thread with a hard deadline. Allocating, locking,
logging or touching a file there causes an audible glitch. Here is how each of
those is kept out:

* **No allocation.** `Mixer::new` allocates everything before the stream
  starts: a 256-entry clip table, a 32-entry voice table, and the queue slots.
  `Mixer::render` only reads and writes those. The cpal integer-format path
  additionally preallocates one scratch buffer and renders in chunks that fit
  it, so a large callback still allocates nothing.
* **No locking.** The only channel between threads is `ring.rs`, a bounded
  single-producer/single-consumer queue built from two `AtomicUsize` indices.
  `push` is a relaxed load, an acquire load, a move and a release store; `pop`
  is the mirror image. Neither can block the other, and neither spins. Handing
  out exactly one non-`Clone` `Producer` and one non-`Consumer`, both of which
  need `&mut self`, is what makes the single-writer/single-reader invariant a
  type-system fact rather than a convention.
* **No logging.** `render` and the command handlers contain no logging. The
  AAudio *error* callback does log, but it runs on an ordinary worker thread,
  not the real-time one.
* **No deallocation either.** This is the subtle one. Clips reach the mixer as
  `Arc<[f32]>` inside a command, so installing one is a pointer move. But
  *dropping* the last reference would call `free` on the real-time thread. So
  the mixer never drops a clip: when a slot is replaced or released, the old
  clip is pushed back over a second queue and the UI thread drops it. The UI
  thread drains that queue on every engine call, and it is as deep as the
  command queue, so it cannot fill while the app is talking to the engine. If
  it somehow did, the mixer leaks that one clip deliberately and counts it in
  `AudioEngine::leaked_clips()` rather than calling the allocator.
* **No decoding.** `AudioPlayer::load` decodes on the calling thread and hands
  over finished PCM. `play` is one queue push. A cue that fires every 45 ms
  costs a handful of atomics.
* **Bounded work per callback.** The mixer drains at most one queue's worth of
  commands (512) and mixes at most 32 voices, whatever the app does.

The UI-thread side never waits on the audio thread either: a full command queue
drops the request and logs at debug level rather than blocking a frame.

## What the mixer does

Per voice: linear-interpolating resample from the clip's rate to the device's,
scaled by the requested playback rate (so pitch and speed move together);
constant-power pan and volume folded into a left/right gain pair on the UI
thread; one of two buses; one-shot or looping. Voices beyond 32 steal the
oldest one-shot, never a loop. The summed output is clamped to ±1.

Clips are mono or stereo `f32`. RIFF/WAVE decoding lives in
`cranpose_services::audio` (PCM 8/16/24/32-bit and IEEE float, any rate); other
containers are reported as `AudioError::UnsupportedFormat` rather than guessed
at.

## Layout

| File | Role |
| --- | --- |
| `ring.rs` | The lock-free SPSC queue. One of the crate's two `unsafe` modules. |
| `mixer.rs` | The real-time mixer: commands, voices, resampling, buses. |
| `engine.rs` | The UI-thread `AudioPlayer`: decode, handles, queue pushes. |
| `backend/aaudio.rs` | Android and Wear OS. The crate's other `unsafe` module. |
| `backend/cpal_device.rs` | Desktop. |
