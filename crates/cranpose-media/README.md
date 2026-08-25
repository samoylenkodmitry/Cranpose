# cranpose-media

The in-process media backend behind `cranpose_services::media`.

`cranpose-services` owns the media contract — the observable `PlaybackState`,
seeking, the audio-focus policy, media-session commands, optional analysis
samples. This crate is what fulfils it: `symphonia` for the decoders and
`cranpose-audio`'s output device — AAudio on Android, cpal on desktop — fed
through the same wait-free ring the audio engine uses.

```rust,ignore
fn main() {
    cranpose_media::install();
    // ...
}
```

`install` does nothing on the targets that have their own backend — iOS and the
web, and Android, where the platform layer installs this player wrapped in the
media session it alone can provide — so calling it unconditionally at startup is
correct.

## What it plays

Local files as `file:` URIs, in every container `symphonia` reads. Anything else
is opened by the platform through `open_media_source`: on Android that is a
`content://` document, and a provider backed by a network share hands one over
as a pipe rather than a file. Such a stream is spooled to the application's
cache as it arrives, so playback starts at the front while the rest is still
coming and a seek waits only for the offset it needs.
