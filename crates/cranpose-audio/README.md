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

A voice can play from an `Emitter`, which is a point in space. `AudioPlayer::spatial_scene()` gives the `SpatialScene` of the engine. An app sets the pose of the listener with `SpatialScene::set_listener` and makes emitters with `SpatialScene::add_emitter`. It starts a voice with `PlaybackParams::emitter` and moves an emitter with `Emitter::place`. Each ear of such a voice gets its own gain and its own delay. A head shadow filters the voice. The gains and the delays come from where the emitter stands relative to the listener. The `DistanceAttenuation` of the emitter sets the distance gain. The poses reach the callback in cells of atomic words that the callback reads without a lock. A pose never goes through the command queue. A stereo clip plays as the mean of its channels. `PlaybackParams::pan` has no effect on such a voice. The model has no elevation cue. A source ahead sounds like one behind. `cranpose-xr` uses it for the spatial audio of headsets. `SpatialScene::set_paused` stops the voices of emitters. `SpatialScene::tap` reports the level of each channel once a second for a debug log.

Android uses AAudio from API 26 onward. The backend requests low-latency, 32-bit float stereo output. Device disconnects stop the stream; later playback opens a fresh stream. watchOS renders stereo float at the output's rate into an `AVAudioSourceNode`, in the ambient session category; when the system stops the engine, later playback opens a new one. Desktop `cpal` pauses after the mixer reports idle on its next engine call.

A client that opens a device itself through `backend::open` reads the device's timing from the returned `AudioSink`. `output_latency` gives the time from a render call to the sound. `frames_per_burst` gives the frames of one render call. `underrun_count` gives the times that the device had no frame to play. Each returns `None` where the platform has no value. None of them waits for the audio thread. The latency of a Web Audio stream leaves out `backend::RENDER_LEAD_SECONDS`, which a caller adds. This crate makes AAudio's timestamp call on the thread that owns the stream, so the real-time callback does no extra work. That thread reads the latency every quarter of a second while the stream runs. The first value comes within that interval after the hardware reports its first timestamp.

A browser gives a page no real-time callback on the thread that runs the composition. The Web Audio backend renders on that thread, 1024 frames at a time, and keeps `backend::RENDER_LEAD_SECONDS` of sound scheduled ahead of the context's clock. Each sound is this much later than on the other backends, and a frame that blocks the page for longer leaves a gap. A browser starts a context only after the user has acted on the page; a stream opened before that starts at the first press or key.

## Microphone

`backend::open_microphone` records from the platform's default microphone. Jetpack Compose has no API for recording. Android apps use `AudioRecord` or AAudio, and this follows AAudio. The app passes a `Recorder` and an `InputPreset`. The call opens the input device, starts it and returns an `AudioSource`. Dropping the source stops the recording.

- The person must allow the microphone first: `cranpose_services::microphone_permission` reads `Granted`. Otherwise the call opens nothing and returns `AudioError::NotAllowed`. The stream calls `cranpose_services::prepare_microphone`, which sets the audio session of iOS and watchOS to record, and keeps it readied while it runs.
- `Recorder::record` gets interleaved float samples in the device's format and the `Instant` at which the microphone took the first frame. It runs on the thread on which the platform delivers recorded sound, so it may not allocate, lock or block.
- Android uses an AAudio input stream in low-latency mode, with one channel at the device's rate. The thread that owns the stream reads `AAudioStream_getTimestamp`, and the callback adds the frames since to get the capture time. `InputPreset::VoiceRecognition` asks for no echo cancellation. `InputPreset::VoiceCommunication` asks Android to cancel the echo of what the device plays.
- Desktop and iOS use cpal (Core Audio, WASAPI, ALSA) at the default input's own format. The capture time comes from the callback's timestamp. Both presets record the same.
- watchOS taps the input node of an `AVAudioEngine`. `InputPreset::VoiceCommunication` turns on its voice processing where the watch has it.
- A browser asks the person through `getUserMedia` and records with a script processor, one channel in blocks of 1024 frames, with no capture time. `InputPreset::VoiceCommunication` turns on echo cancellation, noise suppression and gain control.
- `AudioSource::input_latency` gives the age of the first frame of the last block. `AudioSource::is_running` turns false when the platform ends the stream, such as when the device goes away.

## Links

- [API documentation](https://docs.rs/cranpose-audio/latest/cranpose_audio/)
- [Audio service API](https://docs.rs/cranpose-services/latest/cranpose_services/audio/index.html)
- [Source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-audio)
