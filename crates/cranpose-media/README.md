# cranpose-media

The desktop media backend behind `cranpose_services::media`.

`cranpose-services` owns the media contract — the observable `PlaybackState`,
seeking, the audio-focus policy, media-session commands, optional analysis
samples. Android, iOS and the web have a platform media stack and the
`cranpose` crate registers a backend for each. Desktop does not, so this crate
supplies one: `rodio` for the output device, `symphonia` for the decoders.

```rust,ignore
fn main() {
    cranpose_media::install();
    // ...
}
```

`install` does nothing on the targets that have their own backend, so calling
it unconditionally at startup is correct.
