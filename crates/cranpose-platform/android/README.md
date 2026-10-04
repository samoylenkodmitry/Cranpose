# Cranpose Android Platform Adapter

`cranpose-platform-android` converts Android physical pointer coordinates to
Cranpose logical coordinates. Framework and Android host authors use
`AndroidPlatform` before they send pointer input to the UI shell. App authors
select the `android` feature on
[`cranpose`](https://docs.rs/cranpose/latest/cranpose/).

```rust
use cranpose_platform_android::AndroidPlatform;

let mut platform = AndroidPlatform::new();
platform.set_scale_factor(2.0);
platform.set_input_surface_offset_px(4.0, 8.0);
let position = platform.pointer_position(20.0, 24.0);
assert_eq!((position.x, position.y), (12.0, 16.0));
```

The adapter adds the native-surface offset before density conversion. The
[Android shell](https://github.com/samoylenkodmitry/Cranpose/blob/main/crates/cranpose/src/android.rs)
connects Android events, surface state and the renderer.

The feature set is empty. See the [`AndroidPlatform` API](https://docs.rs/cranpose-platform-android/latest/cranpose_platform_android/struct.AndroidPlatform.html),
the [Android host guide](https://github.com/samoylenkodmitry/Cranpose/blob/main/docs/guide.md#put-a-rust-screen-inside-an-android-compose-screen)
and the [crate source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-platform/android).
