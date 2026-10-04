# Cranpose Audio

`cranpose-audio` provides the real-time mixer behind [`cranpose-services::audio`](https://docs.rs/cranpose-services/latest/cranpose_services/audio/index.html). Apps use the service contract for clips, sound IDs, playback parameters, and sound banks; the engine sends mixed samples to the platform output device.

## Setup

For Cranpose apps, enable `cranpose/audio` on Android and `cranpose/audio-desktop` on desktop. Android and Wear OS use AAudio. Desktop uses `cpal` and needs the host audio development libraries; Linux builds require ALSA development headers. Direct users of `cranpose-audio` can select the `aaudio` or `cpal-backend` crate feature.

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

Android uses AAudio from API 26 onward. The backend requests low-latency, 32-bit float stereo output. Device disconnects stop the stream; later playback opens a fresh stream. Desktop `cpal` pauses after the mixer reports idle on its next engine call.

## Links

- [API documentation](https://docs.rs/cranpose-audio/latest/cranpose_audio/)
- [Audio service API](https://docs.rs/cranpose-services/latest/cranpose_services/audio/index.html)
- [Source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-audio)
