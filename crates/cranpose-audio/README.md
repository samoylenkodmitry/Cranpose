# Cranpose Audio

`cranpose-audio` provides the real-time mixer behind [`cranpose-services::audio`](https://docs.rs/cranpose-services/latest/cranpose_services/audio/index.html). Apps use the service contract for clips, sound IDs, playback parameters, and sound banks; the engine sends mixed samples to the platform output device.

## Setup

For Cranpose apps, enable `cranpose/audio` on Android, watchOS and the web and `cranpose/audio-desktop` on desktop. Android and Wear OS use AAudio, watchOS uses `AVAudioEngine`, and a browser uses Web Audio. The backend loads AAudio when the first stream opens. An app with any `minSdk` links. On Android older than 8.0 (API 26) the app runs with no sound. Desktop uses `cpal` and needs the host audio development libraries; Linux builds require ALSA development headers. Direct users of `cranpose-audio` can select the `aaudio`, `avaudio`, `web-audio` or `cpal-backend` crate feature.

Install the engine once on the UI thread before composition:

```rust,no_run
use cranpose_services::{AudioError, AudioPlayer, PlaybackParams};

fn play_wav(wav_bytes: &[u8]) -> Result<(), AudioError> {
    let player = cranpose_audio::install();
    let clip = player.load(wav_bytes)?;
    player.play(clip, PlaybackParams::DEFAULT);
    Ok(())
}
```

`install` registers an `AudioEngine` as the platform audio player and returns its shared handle. `AudioPlayer::load` decodes WAV bytes on the caller thread. `play` queues audio for the mixer. The engine opens the device on first playback, then releases the stream after `IDLE_GRACE_SECONDS` of silence. `has_device_backend` reports whether the current build includes an output backend. Hosts with an output device use `AudioEngine`; other hosts use the service fallback player.

## Audio behavior

The UI thread decodes supported WAV clips and sends bounded commands to the audio thread. The callback mixes up to 32 voices, applies playback rate, pan, volume, and the effects or music bus, then clamps the output. The callback performs bounded work and uses a lock-free queue. Other containers return `AudioError::UnsupportedFormat`; clips use mono or stereo `f32` samples.

Android uses AAudio from API 26 onward. The backend requests low-latency, 32-bit float stereo output. Device disconnects stop the stream; later playback opens a fresh stream. watchOS renders stereo float at the output's rate into an `AVAudioSourceNode`, in the ambient session category; when the system stops the engine, later playback opens a new one. Desktop `cpal` pauses after the mixer reports idle on its next engine call.

A client that opens a device itself through `backend::open` reads the device's timing from the returned `AudioSink`. `output_latency` gives the time from a render call to the sound. `frames_per_burst` gives the frames of one render call. `underrun_count` gives the times that the device had no frame to play. Each returns `None` where the platform has no value. None of them waits for the audio thread. The latency of a Web Audio stream leaves out `backend::RENDER_LEAD_SECONDS`, which a caller adds. This crate makes AAudio's timestamp call on the thread that owns the stream, so the real-time callback does no extra work. That thread reads the latency every quarter of a second while the stream runs. The first value comes within that interval after the hardware reports its first timestamp.

A browser gives a page no real-time callback on the thread that runs the composition. The Web Audio backend renders on that thread, 1024 frames at a time, and keeps `backend::RENDER_LEAD_SECONDS` of sound scheduled ahead of the context's clock. Each sound is this much later than on the other backends, and a frame that blocks the page for longer leaves a gap. A browser starts a context only after the user has acted on the page; a stream opened before that starts at the first press or key.

## Links

- [API documentation](https://docs.rs/cranpose-audio/latest/cranpose_audio/)
- [Audio service API](https://docs.rs/cranpose-services/latest/cranpose_services/audio/index.html)
- [Source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-audio)
