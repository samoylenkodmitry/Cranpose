# Cranpose Media

`cranpose-media` is the in-process playback backend for [`cranpose-services::media`](https://docs.rs/cranpose-services/latest/cranpose_services/media/index.html). The service API tracks the current item, playback state, progress, volume, seek position, audio focus, and media-session commands. This crate decodes audio through Symphonia and sends samples to the shared Cranpose output engine.

## Setup and playback

Cranpose apps enable the `media` feature for the in-process backend. Native desktop builds use `cpal`; Linux builds need ALSA development headers. Android integrates the decoder with its media session and AAudio output. iOS and browser builds use their platform media stacks. Direct users can install the backend on supported native desktop targets:

```rust,no_run
use cranpose_media::{install, uri_for_path};
use cranpose_services::media::{MediaError, MediaItem, open_media, play_media};
use std::path::Path;

fn play_file(path: &Path) -> Result<(), MediaError> {
    install();
    open_media(MediaItem::new(uri_for_path(path)))?;
    play_media()
}
```

`install` returns `true` when the crate installs its native player. Android, iOS, and web hosts provide their own registration. Supported native desktop files use `file:` URIs. HTTP and HTTPS sources stream through byte ranges when the server supports range requests. Android content URIs stream through the platform source opener and the app cache. Symphonia supplies MP3, AAC/MP4, FLAC, Vorbis, WAV, AIFF, and ALAC decoders.

## Playback controls and limits

The service module exposes `pause_media`, `seek_media`, `set_media_volume`, `set_media_speed`, `set_media_looping`, and media metadata APIs. `media_capabilities` reports the operations available on the active backend. Servers with byte-range support allow remote seek requests; other servers play forward. Analysis samples start disabled and require `set_media_analysis_enabled(true)`. The ten-band equalizer uses octave centers from 31 Hz to 16 kHz.

## Links

- [API documentation](https://docs.rs/cranpose-media/latest/cranpose_media/)
- [Media service API](https://docs.rs/cranpose-services/latest/cranpose_services/media/index.html)
- [Source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-media)
